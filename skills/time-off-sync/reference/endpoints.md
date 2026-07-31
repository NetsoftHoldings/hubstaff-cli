# Command reference

Command-to-endpoint map for the time off surface. Field-level detail is deliberately omitted — get
it from the live schema, which is always current:

```bash
hubstaff <command> --help
```

Every command below was resolved against Hubstaff API v2 on 2026-07-31.

## Conventions

- **`organization_id` is implicit** on organization-scoped routes, written `{org}` in the tables
  below. It comes from `-o N` / `--organization N` or `config.organization` and is never passed
  positionally. Omit it on a route that needs it and you get
  `missing required path parameter 'organization_id'`. Single-record routes are not
  organization-scoped and take only the record id.
- **Path parameters are positional**, in URL-template order. Passing one as a flag is an error.
- **Query parameters are flags**: `--name value`, `--name=value`, or `--query name=value`.
- **Array parameters repeat the flag**: `--include users --include time_off_policies`.
- **Bracketed filters keep their brackets and must be quoted**:
  `'--starts_at[start]' 2026-08-01T00:00:00Z`. Unquoted, the brackets are a shell glob — zsh refuses
  to run the command at all (`no matches found`), so the call never reaches the API.
- **Bodies** come from `--body-json '<JSON>'` or `--body-file <PATH>`. A value starting with `--`
  must be attached with `=`.
- Add `-j` for minified JSON (pipeable) or `-p` for pretty colorized output.

Note the routing asymmetry: **collection and create routes are organization-scoped, single-record
routes are not.** So `time_off_requests list` needs an org, while `time_off_requests get 42` does
not — it takes the record id alone.

## Policies

| Command | Method & path | Arguments |
|---|---|---|
| `time_off_policies list` | `GET /organizations/{org}/time_off_policies` | `--status active\|archived\|all` (default `active`), `--page_limit`, `--page_start_id` |
| `time_off_policies get <id>` | `GET /time_off_policies/{id}` | positional id |
| `time_off_policies create` | `POST /organizations/{org}/time_off_policies` | body |
| `time_off_policies update <id>` | `PUT /time_off_policies/{id}` | positional id, body — **partial** update, send only what changes |
| `time_off_policies archive create <id>` | `POST /time_off_policies/{id}/archive` | positional id, no body |
| `time_off_policies restore create <id>` | `POST /time_off_policies/{id}/restore` | positional id, no body |
| `time_off_policies delete <id>` | `DELETE /time_off_policies/{id}` | positional id |
| `time_off_policies user_policies` | `GET /organizations/{org}/time_off_policies/user_policies` | **`--user_id` required**, `--page_limit`, `--page_start_id` |

The `archive create` / `restore create` shapes look odd but are correct — the CLI derives command
words from the path and appends an action from the HTTP method, so `POST …/archive` becomes
`archive create`. Don't guess these from the path; confirm with `hubstaff list`.

Prefer archive over delete. `DELETE` is refused with `400 This time off policy cannot be deleted.`
once any member has approved or paid requests, or used hours. Note the inconsistency: the same class
of refusal on a *request* returns `422`, not `400`.

**Restoring renames the policy.** It appends `(Restored YYYY-MM-DD)` to the existing name, so a policy named
`PTO (EU)` returns as `PTO (EU) (Restored 2026-03-14)`. If your external mapping keys policies by
name rather than by id, a restore silently breaks the match — key on `time_off_policy_id`.

`accrual_type` and `membership_rules` are immutable after creation. A policy built from
`membership_rules` cannot have its membership changed through the API at all.

## Balances

| Command | Method & path | Arguments |
|---|---|---|
| `time_off_balances list` | `GET /organizations/{org}/time_off_balances` | `--year`, `--user_ids`, `--time_off_policy_ids`, `--search`, `--include users\|time_off_policies`, `--page_limit`, `--page_start_id` |
| `time_off_balances create` | `POST /organizations/{org}/time_off_balances` | body |

There is no update or delete. `create` is how you set a balance — `modify_balance: "replace"` makes
it an upsert.

`--year` defaults to the current year **in the organization's timezone**; pass it explicitly rather
than relying on that. Response `amount` is in **seconds** and reflects usage already deducted by
approved requests.

The POST returns `{"success":true}`, not a balance object. Re-read the list endpoint to confirm a
write landed.

## Requests

| Command | Method & path | Arguments |
|---|---|---|
| `time_off_requests list` | `GET /organizations/{org}/time_off_requests` | `--created[start]`, `--created[stop]`, `--starts_at[start]`, `--starts_at[stop]`, `--approved_at[start]`, `--approved_at[stop]`, `--user_ids`, `--include`, `--page_limit`, `--page_start_id` |
| `time_off_requests get <id>` | `GET /time_off_requests/{id}` | positional id |
| `time_off_requests create` | `POST /organizations/{org}/time_off_requests` | body |
| `time_off_requests update <id>` | `PUT /time_off_requests/{id}` | positional id, body — **full replacement**, resend every field including the whole day array |
| `time_off_requests status <id>` | `PUT /time_off_requests/{id}/status` | positional id, body: `status: approved\|denied`, `response` (required when denying), `remove_shifts` |
| `time_off_requests delete <id>` | `DELETE /time_off_requests/{id}` | positional id |

The approval command is `status`, **not** `update_status` — derived from the trailing path segment.

All `[stop]` bounds are **exclusive**. There is no `updated_at` filter, so no true
"changed since last run" query exists; `updated_at` is present on the response and must be compared
client-side.

`get` and `create` both return `balance_preview` as a **sibling** top-level key alongside
`time_off_request`, with five sub-objects each carrying only `current`, all in seconds:
`starting_balance`, `pending_approval`, `amount_left`, `amount_used`, `holiday_hours`. It is absent
from the schema, so treat its shape as unstable.

`delete` is refused with `422` for approved or paid requests.

## Pagination

Every list endpoint is cursor-paginated. Send `--page_limit`, then feed the response's
`pagination.next_page_start_id` back as `--page_start_id` until it stops appearing. On a `429`,
resume from the **same** `page_start_id` rather than restarting.

## Exit codes

`0` success · `1` API error · `2` auth · `3` config or usage · `4` network.

Treat `4` as retryable with backoff and `3` as a bug in the caller.

An unknown command on its own exits `3` with `error: unknown command '<name>'`. But **add `--help`
and an unknown command exits `0`**, printing global help instead of an error. So when checking
whether a command exists, run `hubstaff <command> --help` and test that stdout begins with
`Command:` — the exit code cannot distinguish the two.
