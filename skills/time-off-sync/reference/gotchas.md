# Time off sync gotchas

Most entries here fail **quietly** — wrong data, no error. A few surface a real API error but are
easy to misdiagnose, so they are included too. The error-code table in `SKILL.md` covers the codes
themselves.

Verification status is noted per entry: **live** means observed against a real organization on
2026-07-31; **schema** means taken from the OpenAPI document without a live write.

---

## An under-privileged token gets filtered results, not a 403

**Symptom.** A sync run completes cleanly and reports that almost nothing needed mirroring.

**Cause.** `time_off_requests list` and `time_off_policies list` return `200` with results
narrowed to the acting member's own records and memberships. Only `time_off_balances` fails loudly
(`403`, `error_code 14705`). An organization access token authenticates *as the member it is
assigned to* and inherits that member's permissions.

**Fix.** Gate on `time_off_balances list` returning a successful response. Treat the spread of
`user_id` values in the request list as a hint only — a single `user_id` is normal in a small
organization. Never treat a `200` from the list endpoints as proof of access.

*Verified: live. A plain-member token received `200` from both list endpoints, with the results
confined to that member's own records and policy memberships.*

---

## A readable request can reference an unreadable policy

**Symptom.** Resolving policy names per request throws on a `404`.

**Cause.** `time_off_requests list` returns records whose `time_off_policy_id` the same token
cannot `get` — the policy list is membership-scoped while the request record is not.

**Fix.** Treat policy resolution as fallible. Carry the id through unresolved rather than failing
the run.

*Verified: live. A request returned by the list endpoint referenced a policy that
`time_off_policies get` then rejected with `404` / `error_code 12000` for the same token.*

---

## Writing balances before approving requests corrupts them

**Symptom.** Balances are short by exactly the amount of leave the run just approved.

**Cause.** Approving a request deducts from the balance. Writing the authoritative balance first
means every subsequent approval subtracts from it.

**Fix.** Write balances **last**: policies → requests → balances. Note this contradicts the order
the concepts are usually explained in.

*Verified: live, as a controlled A/B on two policies. Approving `86400` and then replacing to
`288000` left the balance at `288000` (correct). Replacing to `288000` and then auto-approving
`57600` left it at `230400` — short by exactly the approved amount.*

---

## `modify_balance: "add"` double-counts on retry

**Symptom.** Balances drift upward by a whole run's worth after a timeout or redeploy.

**Cause.** `"add"` is a delta. Any retry of a partially-completed run applies it twice. There is
no idempotency key to protect you.

**Fix.** Use `"replace"` for mirroring — it sets the balance to `amount`, so a repeat run is a
no-op. Reserve `"add"` for one-off corrections a human explicitly asked for. When reconciliation
finds a discrepancy, fix it with another `"replace"`, not an `"add"` of the difference.

*Verified: schema. Enum is exactly `add` | `replace`, lowercase.*

---

## `replace` sets the remaining balance, not the entitlement

**Symptom.** Every member who has taken leave shows more balance than they should, by exactly the
amount they used.

**Cause.** `modify_balance: "replace"` sets the **remaining** figure and back-solves the starting
balance to `amount + already_used`. Writing an annual entitlement into `amount` therefore credits
back everything already consumed.

**Fix.** Write the remaining balance from your source of truth. The `amount` returned by
`time_off_balances list` is the same remaining figure, so read → compare → write is consistent
provided both sides mean "remaining".

*Verified: live. Approving `86400` then replacing with `288000` produced `starting_balance 374400`,
`amount_used 86400`, `amount_left 288000` — the used time was credited back into the starting
balance rather than reducing the result.*

---

## A missing date in `time_off_request_days` is a 400

**Symptom.** Request creation rejects a multi-day range that looks complete.

**Cause.** The array needs one entry for **every** date from `starts_at` through `stops_at`
inclusive. Weekends and holidays you are excluding still need an entry, with `amount_used: 0`.

**Fix.** Generate the full inclusive date range, then set `amount_used` to `0` on excluded days
rather than dropping them. Note that `exclude_weekends: true` does **not** excuse you from sending
the weekend entries.

*Verified: live, by omitting two weekend days on purpose:*

```text
error: [400] Validation failed: the number of allocated days does not match the expected total
```

*CLI exit code was `1`. Real multi-day requests contain explicit `amount_used: 0` weekend entries.*

---

## The balance deduction is the day array, not the date span

**Symptom.** A request covering five days deducts an unexpected amount.

**Cause.** The deduction is `sum(time_off_request_days[].amount_used)`. The wall-clock span
between `starts_at` and `stops_at` does not determine it.

**Fix.** Treat the day array as the authoritative cost of the request. Partial days are normal —
amounts smaller than a full working day appear alongside the usual `28800`.

*Verified: live.*

---

## Units are seconds in some places and hours in others

**Symptom.** A balance is off by a factor of 3600, or an accrual cap is absurd.

**Cause.** Balance `amount` and every `amount_used` are in **seconds**. Policy accrual settings
(`hours_per_year`, `hours_per_month`, `maximum_to_accrue`, `starting_balance`) are in **hours**.

**Fix.** Convert at the boundary and label variables with the unit. An 8-hour day is `28800`
seconds.

*Verified: schema and live — policy accrual read back in hours while request amounts were in
seconds.*

---

## `remove_shifts` is ignored on create when the policy requires approval

**Symptom.** Overlapping attendance shifts survive a request that asked to remove them.

**Cause.** On create, `remove_shifts` is honoured **only** when the policy does not require
approval (so the request is auto-approved). For approval-required policies it is silently
dropped.

**Fix.** Pass `remove_shifts` on the status call when approving instead. It is ignored when
denying.

*Verified: schema — stated in both the create and status operation descriptions.*

---

## `accrual_type` does not exist in any response

**Symptom.** Code that reads a policy back cannot find the field it just wrote.

**Cause.** The write model uses `accrual_type` and `accrual_policy`; responses use `policy_type`
and `policy_config`. Same concepts, different names, and the response fields carry no enum
declaration.

**Fix.** Translate at the boundary. Also note that policy `PUT` is a **partial** update while
request `PUT` is a **full replacement** — the two resources behave oppositely.

*Verified: live (`policy_type: "annual"`, `policy_config: {...}`, no `accrual_type` anywhere).*

---

## Legacy policies can violate the documented create contract

**Symptom.** Reading a policy and writing it back fails validation.

**Cause.** `maximum_to_accrue` is required when creating an `annual` policy, but policies created
years ago may lack it entirely.

**Fix.** Don't assume a policy read from the API is valid as a create/update payload. Send only
the fields you intend to change.

*Verified: live. An `annual` policy created years earlier read back with no `maximum_to_accrue`,
which the create endpoint requires for that accrual type.*

---

## The justification field has two different names

**Symptom.** A write is rejected for a missing field that appears to be present.

**Cause.** Balance adjustments require `reason`. Request approval and denial use `response`, which
is required when denying.

**Fix.** Keep them straight per endpoint; `hubstaff <command> --help` prints the body fields.

*Verified: schema, plus live — a denied request carried its reason in `response`.*

---

## Editing a denied request silently resubmits it

**Symptom.** A request that was denied is suddenly `submitted` again, or approved.

**Cause.** Updating a denied request resets its status to `submitted`. If the policy does not
require approval, the update auto-approves it.

**Fix.** Don't use update to correct a denied record unless resubmission is what you want.
Approved, paid, and partially paid requests cannot be edited or deleted at all.

The `response` from the original denial **survives the resubmission**, so a non-empty `response` is
not evidence that a request is currently denied. Branch on `status`, never on the presence of
`response`.

*Verified: live. Updating a denied request returned it to `status: "submitted"` while the original
denial `response` remained on the record.*

---

## There is no external ID field

**Symptom.** No way to reliably match a Hubstaff record to an upstream one.

**Cause.** Time off resources carry only numeric internal ids and foreign keys. There is no
external-id, correlation, or idempotency field anywhere in the model.

**Fix.** Keep your own mapping table — it is the only reliable identity. Write a stable upstream
reference into `message` (requests) or `reason` (balance adjustments) so a human can trace a
discrepancy. `user_id` + `time_off_policy_id` + date range is a fallback only: it can match more than
one record, so treat multiple matches as a conflict to surface rather than picking one.

*Verified: schema — a field-name scan across all time off definitions found no
external/source/uuid/ref field.*

---

## Two different timezones are in play

**Symptom.** A balance lands in the wrong year, or a day boundary is off by hours.

**Cause.** Request `starts_at`/`stops_at` resolve in the **target member's** timezone, and any UTC
offset in the value you send is ignored. The balances `year` parameter defaults to the current
year in the **organization's** timezone.

**Fix.** Send request timestamps as naked local datetimes and pass `year` explicitly rather than
relying on the default.

*Verified: schema (offsets ignored, org-timezone default) and live (stored values carry the
member's offset).*

---

## Known schema defects

The cached OpenAPI document is wrong in several places. Trust the CLI's `--help` for field names,
but not for these:

| Where | What the schema says | Reality |
|---|---|---|
| `time_off_requests create` description | `all_day: true` normalizes times to `09:00–17:00` | Describes **internal storage**, which no API consumer observes. What you read back is the member's whole local day, `00:00:00.000` → `23:59:59.999` at their offset. Build day windows from the whole day. *Verified live.* |
| `TimeOffRequest.time_off_request_days` | A single object | An array. The request-side counterpart is correctly typed. |
| `TimeOffRequest.id` | "Time edit log ID" | The request id. Copy-paste error. |
| `TimeOffRequest.time_off_policy_id` | "Time off request ID" | The policy id. Copy-paste error. |
| `balance_preview` | Promised in prose on two operations, with no schema definition | Real, and a **sibling** top-level key of `time_off_request` — not nested inside it. Five sub-objects, each carrying only `current`, all in seconds: `starting_balance`, `pending_approval`, `amount_left`, `amount_used`, `holiday_hours`. *Verified live on both create and approve.* |
| `POST time_off_balances` response | `201` with `{"time_off_balance": {...}}` | Returns `{"success":true}`. Do not parse a balance out of it — re-read the list endpoint to confirm. *Verified live.* |
| `membership_rules` | Type `"Array[String]"` | Not a valid JSON Schema type; breaks strict codegen. |
| Response `status` / `policy_type` | No enum | Values are constrained in practice; the request-side enums are the reliable list. |
| `include[]` on list endpoints | Parameter accepts `users`, `time_off_policies` | The response schema omits the sideloaded top-level keys. |
| Security | Global `["hubstaff:read", "hubstaff:write"]` on every operation | No per-operation split is declared, so the schema cannot tell you which calls are read-only. |
| `DELETE` on a request | Declares `204` **and** a response body | Expect no body. |
