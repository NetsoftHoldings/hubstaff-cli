---
name: time-off-sync
description: >-
  Mirror time off from an external system of record (HRIS, payroll, spreadsheet) into Hubstaff
  using the `hubstaff` CLI — policies, balances, leave requests, and approvals. Use this whenever
  the user talks about syncing, importing, backfilling, or reconciling PTO, vacation, sick leave,
  holidays, or leave balances with Hubstaff, even if they don't say "time off" or name an endpoint.
  The API has no webhooks, no idempotency keys, and no external-ID field for time off, so a
  hand-rolled sync silently corrupts balances; read this before writing any sync code or issuing
  any write call.
license: MIT
metadata:
  cli-min-version: "0.5.0"
  scopes: "hubstaff:read + hubstaff:write; acting member needs owner or manager permission"
  verified: "2026-07-31 against Hubstaff API v2 — every command executed, including policy and
    request creation, approval, denial, and balance adjustment. Repeat runs confirmed replace is
    idempotent. The examples/ payloads were executed with real ids substituted for the
    placeholders."
  commands:
    - time_off_policies list
    - time_off_policies get
    - time_off_policies create
    - time_off_policies update
    - time_off_policies delete
    - time_off_policies user_policies
    - time_off_policies archive create
    - time_off_policies restore create
    - time_off_balances list
    - time_off_balances create
    - time_off_requests list
    - time_off_requests get
    - time_off_requests create
    - time_off_requests update
    - time_off_requests delete
    - time_off_requests status
---

# Time off sync

Mirror externally-owned time off into Hubstaff so members and managers see leave in the same
place they see tracked time, without double entry.

This skill covers the **decisions** — ordering, idempotency, reconciliation — because those are
the parts that go wrong silently. It deliberately does not restate field-level documentation:
the CLI prints that from the live schema, and duplicating it here would create a second copy to
drift.

**Look up fields with the CLI, not from memory:**

```bash
hubstaff time_off_requests create --help    # every body field, current as of the cached schema
hubstaff time_off_policies create --help    # the accrual_type → accrual_policy requirement table
hubstaff list                               # every command the schema knows about
```

Read `reference/gotchas.md` before the first write call. It is short, and most entries are things
that can fail quietly rather than loudly.

**The files in `examples/` are templates, not runnable payloads.** Their ids are placeholders
(`user_id: 1001`, `time_off_policy_id: 512`). Copy one, substitute real ids and dates, write it to
your own file, and pass that to `--body-file`. Paths below are written relative to the skill
directory; use a real path when you invoke them.

## Scope

This skill is for a **one-way mirror into Hubstaff**, with an external system as the source of
truth. It is not for exporting Hubstaff time off outward (read the same endpoints directly), and
it cannot modify paid or partially paid requests — the API freezes those.

Assume a human is supervising. Before any call that writes — create, update, status, delete,
archive or restore — show the exact command and its body, or just the command where there is no
body, and get confirmation. A wrong `"replace"` adjustment silently overwrites a real person's leave
balance and there is no undo, so the cost of asking is far below the cost of being wrong.

## Preflight

Work through these four before writing anything. Skipping them is how a sync job reports success
while having done nothing. Where a check is a hard gate rather than a signal to weigh, it says so.

**1. CLI is authenticated and pointed at the right place.**

```bash
hubstaff check
hubstaff config show                 # confirm api_url and organization
```

Read the individual rows rather than gating on the exit code. The rows that matter here are
**API reachability** and **Organization access** — those two prove the credential works against the
org you think it does. A `WARN` on **Token validity** is expected for any non-expiring credential —
an organization access token (`hsoat_…`) or a raw token stored with `config set token`. Both are
long-lived and non-refreshable by design, so treat that `WARN` as normal rather than rejecting the
credential.

On a CLI older than `0.5.0` that same token makes **Token validity** report `FAIL` and forces exit
`1` even when everything else passes, and the suggested `config set-pat` is wrong for this token
type. Upgrade, or read the rows.

**2. The token can see the whole organization, not just itself.**

This is the check that matters most, because failure here is invisible. An under-privileged token
does **not** reliably get a `403` — `time_off_requests list` and `time_off_policies list` return
`200` with results silently narrowed to what the acting member can see. A sync job then concludes
the org has almost no time off to mirror and exits clean.

Use the balances endpoint as the authoritative probe, because it is the one that fails loudly:

```bash
hubstaff -j time_off_balances list --year "$(date +%Y)" --page_limit 1
```

`403` with `error_code 14705` means the acting member cannot manage time off balances — stop and fix
the permission. Treat **only a successful response** as proof of access: a `401`, `429`, `5xx`, or
transport failure says nothing about permission, so stop and retry rather than pressing on. Note that
an organization access token authenticates **as the member it is assigned to** and inherits that
member's permissions rather than carrying its own.

As a secondary sanity check, list requests and see whether they span more than one `user_id`:

```bash
hubstaff -j time_off_requests list --page_limit 50
```

Treat this as a hint, not a gate. A single `user_id` is expected in a small org where only one person
has taken leave, so it produces false alarms; but a single `user_id` that is *also* the acting member,
in an org you know has broader usage, is a strong signal the results are being filtered.

**3. Every target member is on the target policy.**

Request creation only accepts policies the member actually belongs to:

```bash
hubstaff -j time_off_policies user_policies --user_id <USER_ID>
```

Balance adjustments are more forgiving — they auto-assign an unassigned member to the policy
before applying — but requests are not.

**4. Hubstaff is not also accruing.**

If both systems accrue, balances drift apart and neither is trustworthy. Sync-managed policies
should be created with `accrual_type: "none"` so Hubstaff never accrues on its own, and the
authoritative number gets written in on every run. Check existing policies before adopting them:
a policy returning `policy_type` other than `"none"` is accruing.

## The write order, which is not the reading order

Documentation and mental models both present time off as *policies → balances → requests*. That
is the right order to **read** in, and the wrong order to **write** in.

**Write in this order: policies → requests → balances.**

Approving a request deducts from the balance. If the authoritative balance is written first and
requests are approved afterwards, every approval subtracts from the number just written, and the
run finishes with balances wrong by the sum of everything it approved — with no error anywhere.
Writing balances last makes the run converge regardless of what the approvals did.

## Step 1 — Resolve policies

Build a map from each external policy to a Hubstaff `time_off_policy_id`, and cache it. It
changes rarely, and every balance and request is keyed by it.

```bash
hubstaff -j time_off_policies list --status all --page_limit 100
```

Use `--status all`, not `--status active`. An upstream policy that matches an **archived** Hubstaff
policy is invisible to an active-only listing, so the run treats it as new and creates a duplicate
instead of restoring the original. Restore the archived match, or update it, and create only when no
record exists at all. Mutable fields — `name`, `requires_approval`, `paid`, membership — also need
comparing on an existing match; a one-way mirror that only ever creates will drift on every upstream
edit.

Beware a naming asymmetry that will not show up until you look for it: **the read model and the
write model use different names for the same two concepts.**

| Concept | On create/update | In responses |
|---|---|---|
| Accrual strategy | `accrual_type` | `policy_type` |
| Accrual settings | `accrual_policy` | `policy_config` |

There is no `accrual_type` field in any response. Code that round-trips a policy has to translate.

Create a policy only when a new one appears upstream. `hubstaff time_off_policies create --help`
prints the required fields and the full `accrual_type` → `accrual_policy` requirement table —
read it from there rather than guessing, since the requirements differ per accrual type.

Set `requires_approval: false` when approval already happened upstream; requests you create are
then approved on the way in and Step 3 becomes a single call instead of two.

`accrual_type` and `membership_rules` are immutable after creation. A policy using
`membership_rules` cannot have its membership changed through the API at all, so build
sync-managed policies from `time_off_policy_user_ids` instead. To retire one, archive rather than
delete:

```bash
hubstaff time_off_policies archive create <POLICY_ID>
```

## Step 2 — Mirror requests and approvals

Read current state first so the run can skip what already matches. **There is no idempotency
key, and nothing prevents a duplicate** — deduplication is entirely the caller's job:

```bash
# quote the bracketed flags — unquoted they are glob patterns and zsh refuses to run at all
hubstaff -j time_off_requests list \
  '--starts_at[start]' 2026-08-01T00:00:00Z \
  '--starts_at[stop]'  2026-09-01T00:00:00Z \
  --user_ids <USER_ID> \
  --include users --include time_off_policies \
  --page_limit 100
```

Identify the existing record by your own mapping table first, or by the reference you wrote into
`message`. Fall back to `user_id` + `time_off_policy_id` + date range only when you have neither —
that tuple is not unique (two half-days on one date under one policy collide), so if it matches more
than one record, stop and surface it rather than guessing which to update.

Then compare the existing record against the state your source expects. **Skip only when it already
matches.** A match still sitting at `submitted` when your source says approved is unfinished work
from an earlier run, not a duplicate — finish the transition instead of skipping it. Create what is
genuinely missing:

```bash
# body built from examples/request-create.json with real ids and the full date range
hubstaff time_off_requests create --body-file ./request-to-create.json
```

Two things about the body deserve care. `time_off_request_days` needs **one entry for every date**
from `starts_at` through `stops_at` inclusive — excluded weekends and holidays included, at
`amount_used: 0` rather than omitted. A missing date is a `400`. And the balance deduction is the
**sum of `amount_used` across those days**, not the wall-clock span, so that array is the field
that actually determines the cost of the request.

Carry a stable external reference in `message`. It is the only place to put one, and a human
investigating a discrepancy has nothing else to go on.

### Getting to approved

Which path applies depends on the policy:

- **`requires_approval: false`** — the request is approved on creation. Nothing further to do.
- **`requires_approval: true`** — the request lands as `submitted` and needs a second call.

```bash
hubstaff time_off_requests status <REQUEST_ID> --body-json '{"status":"approved","remove_shifts":true}'
```

`remove_shifts` belongs **here, on the approval**, for any policy that requires approval — passed
on create it is silently ignored. Denials use the same endpoint with `status: "denied"` and a
`response`. Omitting it fails with `[422] A response is required when denying a time off request.`

Note that the justification field is named `response` here but `reason` on balance adjustments.

Allowed transitions are `submitted → approved`, `submitted → denied`, `approved → denied`, and
`denied → approved`, so a reversal upstream can be mirrored. Paid and partially paid requests are
frozen and rejected by this endpoint.

## Step 3 — Mirror balances, last

```bash
# body built from examples/balance-adjustment.json with real policy and user ids
hubstaff time_off_balances create --body-file ./balances-to-apply.json
```

Use `modify_balance: "replace"`. It sets the balance to `amount`, so a second run writes the same
number and a retry after a half-failed run is harmless. `"add"` is a delta: retry it and the
balance double-counts. Keep `"add"` for genuine one-off corrections a human asked for.

**`amount` is the member's *remaining* balance, not their annual entitlement.** This is the single
easiest way to corrupt a sync. `replace` sets the remaining figure directly and back-solves the
starting balance to `amount + already_used`, so if you write the entitlement instead, every member
who has taken leave is over-credited by exactly the amount they used.

Concretely, for a member with `288000` remaining who has already used `86400`:

```text
replace amount=288000  →  starting_balance 374400, amount_used 86400, amount_left 288000  ✓
replace amount=374400  →  starting_balance 460800, amount_used 86400, amount_left 374400  ✗ over-credited
```

The value in the `time_off_balances` list response is this same remaining figure, so a read →
compare → write loop is consistent as long as both ends mean "remaining".

`reason` is required and is stored with the adjustment. Put a run identifier in it — it is the
only audit trail pointing back at the sync job.

Leave `apply_accruals` at its default `false` when the external system owns accruals.

**Units differ across this API**: balance `amount` and every request `amount_used` are in
**seconds** (an 8-hour day is `28800`), while policy accrual settings are in **hours**. Getting
this wrong is a factor-of-3600 error that looks plausible in a diff.

## Reconciliation, and why it costs a full scan

Two constraints shape every sync schedule here:

- **No webhooks.** Hubstaff emits no webhook events for time off policies, balances, or requests.
  Polling is the only option.
- **No `updated_at` filter.** Records *carry* `created_at` and `updated_at`, but you cannot query
  on `updated_at`. So there is no true "everything that changed since last run" request.

The available filters are `created[start]`/`[stop]`, `starts_at[start]`/`[stop]`, and
`approved_at[start]`/`[stop]` — all ISO 8601, all with an **exclusive** `[stop]` bound.

A workable strategy:

1. Watermark on `created[start]` to pick up new requests, and `approved_at[start]` to pick up
   newly approved ones.
2. Overlap the window by a day or two so a record created just after the last cutoff isn't missed.
3. Periodically re-scan an outer window by `starts_at` — current and next month — and compare the
   returned `updated_at` against your stored value client-side. This is the only way to catch edits
   and deletions, which no filter surfaces.
4. Handle deletions in both directions explicitly, and confirm one before acting on it. A record
   your rescan stops returning has not necessarily been deleted — its `starts_at` may simply have
   moved outside the window. Fetch it by its stored id with `time_off_requests get <id>`: only a
   `404` is a deletion, while a successful response means the request moved and should be
   reconciled at its new dates. Treating absence from the window as deletion is how you recreate a
   request that still exists, or drop a mapping you still need. A record deleted upstream needs `time_off_requests delete` here,
   which is refused for approved or paid requests: those must be denied via the status endpoint
   instead.

That third step is O(window) on every run. Choose the window deliberately; there is no cheaper
correct option.

To verify convergence, re-read balances for the affected members and compare against the source
of truth. **If they disagree, fix with another `"replace"` — never an `"add"` of the difference.**

Every list endpoint is cursor-paginated: send `--page_limit`, then feed the response's
`pagination.next_page_start_id` back as `--page_start_id` until it stops coming.

## Failures

Branch on the numeric `error_code` in the response body, never on the human-readable `error`
string. Codes are grouped in ranges — 10000s auth, 11000s validation, 12000s resource, 13000s
rate limiting, 14000s time and activity. Time off permission failures arrive in the 14000 band
(`14705` = cannot manage time off balances). `GET /v2/error_codes` returns the full list.

| HTTP | Meaning | Action |
|---|---|---|
| 400 | Invalid parameters — often a missing date in `time_off_request_days` | Do not retry unchanged |
| 401 | Token expired or revoked | A PAT session refreshes automatically. Non-refreshable credentials — organization access tokens and raw tokens from `config set token` — cannot, so a 401 means revoked |
| 403 | Not on an active plan, or acting member lacks permission | Alert; do not retry |
| 404 | Unknown or non-visible ID, often a stale cached `time_off_policy_id` | Refresh the policy map |
| 422 | Refused — either a frozen record or a conditionally-required field | Read the message before deciding; see below |
| 429 | Rate limited | Honour `Retry-After`; resume from the same `page_start_id` |

`422` is easy to miss: it is absent from the published error table, and it covers two situations that
need opposite responses. A handler that branches only on 400/401/403/404/429 falls through entirely.

- **Frozen record — do not retry.** `time_off_requests delete` on an approved request returns
  `[422] This time off request cannot be deleted.` The same refusal on a *policy* returns `400`
  instead, so don't key the decision on the status code alone.
- **Conditionally-required field — retry with it supplied.** Denying without a reason returns
  `[422] A response is required when denying a time off request.`

Both verified live. Treating every `422` as permanent silently drops the second kind.

CLI exit codes map cleanly: `1` API error, `2` auth, `3` config or usage, `4` network. Treat `4`
as retryable with backoff and `3` as a bug in the caller.

Partial failure leaves real state behind. A created request whose approval call failed is a live
`submitted` request, not a rollback — the next run must recognise and finish it, which is another
reason to check before creating.

Exit code `4` deserves particular care: it means *you* never got a response, not that the write
didn't happen. The server may well have created the request. Since there is no idempotency key,
re-read the current state before retrying any non-idempotent write — otherwise the retry is how you
create the duplicate. `time_off_balances create` with `"replace"` is the exception, being safe to
repeat by construction.

## Reference

- `reference/gotchas.md` — the traps, each as symptom → cause → fix. Read before the first write.
- `reference/endpoints.md` — command-to-endpoint map with the arguments each takes.
- `examples/` — synthetic request bodies to copy and adapt. No real member data.
