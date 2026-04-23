# Hubstaff CLI

`hubstaff` is a command-line interface for the [Hubstaff Public API](https://developer.hubstaff.com/docs/hubstaff_v2). Use it to work with organizations, projects, members, invites, tasks, and activity data directly from your terminal.

## Cheat sheet

```bash
# One-time auth
hubstaff config set-pat YOUR_PERSONAL_ACCESS_TOKEN
hubstaff config set organization 12345        # optional default org

# Discover what you can do
hubstaff list                                  # every command, grouped by resource
hubstaff projects --help                       # group help with summaries
hubstaff projects list --help                  # full flag reference for one operation

# Read operations
hubstaff users me
hubstaff projects list
hubstaff projects list --page_limit 10
hubstaff projects get 123
hubstaff -p projects list                      # pretty, colorized JSON

# Write operations (side-effecting)
hubstaff projects create --body-json '{"name":"New","status":"active"}'
hubstaff projects update 123 --body-file patch.json
hubstaff teams update_members 42 --body-json '{"members":[{"user_id":7,"role":"member"}]}'

# Per-invocation org override
hubstaff -o 999 projects list

# Health check (exit 0 = all OK, exit 1 = any FAIL)
hubstaff check

# Non-interactive / CI
export HUBSTAFF_API_TOKEN="..."                # takes precedence over stored tokens
hubstaff -j projects list                      # minified JSON, pipeable to jq
```

Exit codes: `0` success · `1` API error · `2` auth error · `3` config / usage error · `4` network error.

---

## Authentication

The CLI supports two authentication paths. Only one is used per invocation, with the following precedence:

| Source | Precedence | Auto-refresh | When to use |
|---|---|---|---|
| `HUBSTAFF_API_TOKEN` environment variable | Highest | No | CI, automation, ephemeral sessions |
| Tokens stored by `hubstaff config set-pat` | Used if env var is unset | Yes (120 s skew) | Interactive / developer machines |

If `HUBSTAFF_API_TOKEN` is set and non-empty, stored tokens are ignored entirely. `hubstaff check` surfaces a `WARN` on the **Env token shadowing** check when both sources are configured.

### Interactive: personal access token exchange

```bash
hubstaff config set-pat YOUR_PERSONAL_ACCESS_TOKEN
```

`set-pat` treats the value as a refresh token and POSTs `grant_type=refresh_token` to `<auth_url>/access_tokens`. The response's `access_token`, `refresh_token`, and `expires_in` are saved to the `[auth]` section of the config file. After this, every API call auto-refreshes the access token when it is within 120 seconds of expiry, and transparently refreshes once on a 401 response.

### Non-interactive: environment variable

```bash
export HUBSTAFF_API_TOKEN="your-access-token"
hubstaff users me
```

Env-var tokens never auto-refresh. On 401 the CLI exits with `error: invalid token. Check your HUBSTAFF_API_TOKEN` and exit code `2`.

### Other auth-related config

- `hubstaff config set token RAW_ACCESS_TOKEN` — stores a raw access token directly and clears any `refresh_token` / `expires_at`. Will not auto-refresh. Use `set-pat` unless you have a specific reason to bypass the exchange.
- `hubstaff config unset token` — clears the whole `[auth]` section.
- `hubstaff config reset` — resets every key including tokens.

---

## Global flags

Global flags work with any subcommand and can appear before or after it.

| Flag | Short | Type | Effect | Notes |
|---|---|---|---|---|
| `--json` | `-j` | bool | Emit minified single-line JSON | Overrides config `format`. Conflicts with `-p`. |
| `--pretty` | `-p` | bool | Emit pretty-printed, colorized JSON | Overrides config `format`. Conflicts with `-j`. |
| `--organization N` | `-o N` | u64 | Override the default organization for this invocation | Takes precedence over `config.organization`. |
| `--help` | `-h` | bool | Print context-appropriate help | See [Help system](#help-system). |
| `--version` | — | — | Print `hubstaff <version>` | From `CARGO_PKG_VERSION` at build time. |

If neither `-j` nor `-p` is passed, the config `format` value applies (default `json`).

---

## Commands

Top-level commands are fixed: `config`, `list`, `check`, plus a dynamic subcommand that dispatches anything else to the schema-driven API layer.

### `hubstaff config`

Manage configuration. All subcommands read and write `$CONFIG_DIR/hubstaff/config.toml` — see [Configuration](#configuration) for the resolution of `$CONFIG_DIR`.

```bash
hubstaff config set KEY VALUE       # set a single key
hubstaff config unset KEY           # clear KEY / restore its default
hubstaff config reset               # wipe everything, including [auth]
hubstaff config set-pat TOKEN       # exchange a PAT for access + refresh tokens
hubstaff config show                # print current config (tokens masked as ****)
```

Valid keys for `set` and `unset`:

| Key | Type | Default | Notes |
|---|---|---|---|
| `organization` | u64 | *(unset)* | Must parse as `u64`. Used as default org id when a required `organization_id` is omitted. |
| `api_url` | URL | `https://api.hubstaff.com/v2` | Base URL for API calls. |
| `auth_url` | URL | `https://account.hubstaff.com` | OAuth server for `set-pat` and refresh. |
| `schema_url` | URL | *(derived from `api_url` + `/docs`)* | Explicit override for the schema source. |
| `token` | string | *(unset)* | Raw access token; clears `refresh_token` and `expires_at`. |
| `format` | `json` \| `pretty` | `json` | Default output format when neither `-j` nor `-p` is passed. |

`set token` and `set-pat` both mask the echoed value as `****`. `show` never prints raw token material and omits `auth_url` when it equals the default.

### `hubstaff list`

Prints every API command the local schema knows about, grouped by resource, summaries aligned. Stable as long as the cached schema does not change.

### `hubstaff check`

Runs nine diagnostics and prints a table with one of four status markers per row: `OK`, `WARN`, `FAIL`, `SKIP`. Exit code is `1` if any row is `FAIL`, else `0`. Warnings do not fail the check. See [Diagnostics reference](#diagnostics-reference) for per-check behavior.

### Dynamic API commands

Any positional token that is not `config`, `list`, or `check` is matched against the cached schema. General shape: `hubstaff <words...> [path_id...] [query-flags...] [body-flags]`.

- **`<words...>`** — command words derived from the URL path (see below).
- **`[path_id...]`** — one positional per visible path parameter, in URL-template order, excluding the implicit `organization_id`.
- **Query flags** — `--<param> VALUE`, `--<param>=VALUE`, or the generic `--query NAME=VALUE`.
- **Body flags** — `--body-json '<JSON>'` or `--body-file <PATH>` (mutually exclusive), or an operation-specific body alias like `--<param> <JSON>` when the schema declares a body parameter with that name.

#### Command synthesis

Commands are derived from the OpenAPI path and HTTP method at index-build time. A leading `/organizations/{organization_id}/NEXT/...` is rewritten to `/NEXT/...` **unless** `NEXT` starts with `update_` (so `/organizations/{organization_id}/update_members` is preserved verbatim — `update_*` paths take an explicit organization path argument). Static segments become command words; `{xxx}` segments become positional arguments. A terminal action (`list` / `create` / `get` / `update` / `delete`) is appended based on the HTTP method and whether the path ends in a static segment or a `{param}`; non-standard action paths (e.g. `.../update_members`) use the literal last segment as the action with no synthesized suffix.

| Path template | Method | Command |
|---|---|---|
| `/organizations/{organization_id}/users/me` | `GET` | `hubstaff users me` |
| `/organizations/{organization_id}/projects` | `GET` | `hubstaff projects list` |
| `/organizations/{organization_id}/projects` | `POST` | `hubstaff projects create` |
| `/organizations/{organization_id}/projects/{project_id}` | `GET` | `hubstaff projects get <project_id>` |
| `/organizations/{organization_id}/projects/{project_id}` | `PATCH` | `hubstaff projects update <project_id>` |
| `/organizations/{organization_id}/projects/{project_id}` | `DELETE` | `hubstaff projects delete <project_id>` |
| `/organizations/{organization_id}/update_members` | `PUT` | `hubstaff update_members <organization_id>` |
| `/teams/{team_id}/update_members` | `PUT` | `hubstaff teams update_members <team_id>` |

#### Path arguments vs query flags

- Path parameters are **always positional**, in the order they appear in the path template.
- `organization_id` is hidden: it is filled from `-o`/`--organization` or `config.organization`. If no source provides it, the CLI errors with `missing required path parameter 'organization_id'`.
- Providing a path parameter as a flag errors with `error: --<param> is a path parameter; provide it positionally: <usage>`.
- Query parameters are flags:

  ```bash
  hubstaff projects list --page_limit 10
  hubstaff projects list --query page_limit=10        # generic form
  hubstaff activities list \
    --time_slot[start] 2026-04-01T00:00:00Z \
    --time_slot[stop]  2026-04-08T00:00:00Z
  ```

- Duplicate query names error with `query parameter '<name>' was provided multiple times`.

#### Request bodies

Three ways to supply a JSON body (the first two are mutually exclusive):

```bash
hubstaff projects create --body-json '{"name":"New","status":"active"}'      # inline JSON
hubstaff projects update 123 --body-file ./patch.json                         # from file
hubstaff webhooks create --webhook '{"url":"https://example.com/hook"}'       # body alias (if schema declares one)
```

The argument parser treats a token starting with `--` as the next flag. To pass a value that begins with `--`, attach it with `=`:

```bash
hubstaff foo bar --body-json='--literal'
```

### Help system

`-h` / `--help` anywhere in the arguments triggers context-appropriate help. Four flavors:

- **Global help** — bare `hubstaff` or an unknown first word: usage shape, examples, and `hubstaff list` for discovery. Unknown-prefix matches get up to 8 `Suggestions:`.
- **Group help** — `hubstaff <prefix> --help` (e.g. `projects`, `activities daily`): lists subcommands with summaries aligned.
- **Operation help** — `hubstaff <full-command> --help`: method, path, summary, description, tags, and every query option with its type and description. If descendant subcommands exist, they are appended.
- **Shape-mismatch help** — input resolves to a command but with the wrong number of positional path arguments: `'<cmd>' expects N path argument(s): <param names>`.

---

## Configuration

### Config file location

`$CONFIG_DIR/hubstaff/config.toml`, where `$CONFIG_DIR` is resolved as:

1. `$XDG_CONFIG_HOME` if set.
2. Else the OS default (macOS: `~/Library/Application Support`; Linux: `~/.config`; Windows: `%APPDATA%` via `dirs::config_dir()`).
3. Else `$HOME/.config`.

On Unix, `$CONFIG_DIR/hubstaff` is created with mode `0o700`. `hubstaff check` raises a `WARN` if the mode drifts; fix with `chmod 700 <dir>`.

### Config file format (TOML)

```toml
api_url = "https://api.hubstaff.com/v2"
auth_url = "https://account.hubstaff.com"       # omitted from disk when equal to default
organization = 12345                             # optional
schema_url = "https://api.hubstaff.com/v2/docs"  # optional; derived from api_url when absent
format = "json"                                  # omitted from disk when equal to default

[auth]                                           # entire section omitted when all fields are None
access_token = "..."
refresh_token = "..."
expires_at = 1775347200                          # unix timestamp
```

Writes are atomic: the CLI writes to a sibling temp file and renames, so a crash mid-write cannot corrupt the file. Keys and their defaults are documented in [`hubstaff config`](#hubstaff-config).

### Environment variables

| Variable | Effect |
|---|---|
| `HUBSTAFF_API_TOKEN` | Overrides stored tokens. Must be non-empty to take effect. No auto-refresh. |
| `XDG_CONFIG_HOME` | Overrides the config base directory. |

### Output format

| Source of format decision | Winner |
|---|---|
| `-j` / `--json` on the command line | minified single-line JSON |
| `-p` / `--pretty` on the command line | pretty-printed colorized JSON (via `colored_json`) |
| `config.format = "json"` | minified |
| `config.format = "pretty"` | pretty |
| default | minified |

Pretty output is colorized on TTYs and degrades gracefully on pipes.

---

## Schema management

The CLI does not ship a static list of endpoints. It fetches the Hubstaff OpenAPI schema and builds an in-memory command index on first use.

### Cache files

All under `$CONFIG_DIR/hubstaff/schema/v2/`:

| File | Content |
|---|---|
| `docs.json` | Raw OpenAPI schema JSON. |
| `meta.toml` | Cache metadata: `fetched_at`, `etag`, `schema_hash`, `source_url`. |
| `command_index.json` | Pre-built command index (`version`, `schema_hash`, `entries`). |

### Load / refresh flow

On every invocation that needs the schema:

1. Conditional `GET` against `config.effective_schema_url()` with a 40 s timeout, sending `If-None-Match: <cached etag>` when the cached `source_url` matches the current `schema_url`.
2. **`200 OK`** — parse and replace the cache; write fresh `meta.toml` with new etag, `fetched_at = now`, `source_url = <current schema_url>`.
3. **`304 Not Modified`** — keep the cached schema; update `fetched_at` only.
4. **Fetch failed** — fall back to the cached schema **only if** the cached `source_url` matches the current `schema_url`. Otherwise error with `schema fetch failed: <cause>`.

The `source_url` match is the cross-environment safety check: a `docs.json` fetched against staging cannot accidentally serve production commands after you `config set api_url` back to production.

### Command index invalidation

`command_index.json` is keyed on `INDEX_VERSION` (currently `1`) and the schema hash. When either changes, the index is rebuilt on the next run. Manual invalidation is almost never necessary.

### Forcing a refresh

There is **no `hubstaff schema refresh` command**. To force a re-fetch, delete the cache directory:

```bash
# macOS (default config dir)
rm -rf "$HOME/Library/Application Support/hubstaff/schema/v2"
# Linux / XDG
rm -rf "${XDG_CONFIG_HOME:-$HOME/.config}/hubstaff/schema/v2"
```

The next command that needs the schema will refetch and rebuild the index.

---

## Diagnostics reference

`hubstaff check` runs the following checks in order. Status markers: `OK`, `WARN`, `FAIL`, `SKIP`. Exit code is `1` if any check is `FAIL`, else `0`.

| # | Check | `OK` when | `WARN` when | `FAIL` when | `SKIP` when | Remediation on failure |
|---|---|---|---|---|---|---|
| 1 | CLI version | always | — | — | — | — |
| 2 | Config file | file loads (or is absent → defaults used) | — | TOML parse error | — | Fix TOML or delete the file to reset. |
| 3 | Config dir perms (Unix only) | mode is `0o700` | mode is anything else, or `stat` fails | — | directory does not exist; non-Unix platform | `chmod 700 <dir>` |
| 4 | Credentials | `HUBSTAFF_API_TOKEN` set **or** stored access/refresh token | — | neither source present | — | `hubstaff config set-pat <TOKEN>` or set `HUBSTAFF_API_TOKEN`. |
| 5 | Env token shadowing | only one of env / stored is configured (or neither) | both are configured | — | — | Unset one source (usually `unset HUBSTAFF_API_TOKEN`). |
| 6 | Token validity | token is fresh, or near-expiry with a successful refresh, or expired with a successful refresh | near expiry (within 300 s) **and** no refresh token | lacks `expires_at`; expired without refresh token; or refresh attempt failed | using `HUBSTAFF_API_TOKEN` (no expiry tracked); no credentials | `hubstaff config set-pat <TOKEN>` |
| 7 | API reachability | `GET <api_url>/users/me` returns 2xx (RTT reported) | — | transport error or auth error | no credentials | Fix connectivity / `api_url`, or re-auth. |
| 8 | Organization access | `GET /organizations/<org_id>` succeeds | — | request fails | no credentials, API unreachable, or no default organization | Verify the id with `hubstaff config show`, or pass `--organization`. |
| 9 | Schema cache | cache loads and is ≤ 30 days old | cache loads but `fetched_at` is > 30 days old | cache is missing or fails to load | — | Delete the cache dir to force refetch. |

Each row prints a `detail` string and, where applicable, a `remediation` hint and a `notes:` block. The Schema cache row always includes notes: `operations`, `url`, `fetched_at`, `etag` (if any), and the three cache file paths.

Token-validity internals: near-expiry threshold is 300 s; proactive refresh skew during normal API calls is 120 s; during `check`, a near-expiry or expired token with a refresh token is refreshed in-place and the result is recorded.

---

## Errors and exit codes

All errors are written to `stderr` prefixed with `error:` (followed by a space). The process exits with a variant-specific code:

| Error variant | Exit code | Display format | Typical triggers |
|---|---|---|---|
| `Api { status, message }` | `1` | `[<status>] <message>` | API returned 4xx/5xx; 429 rate limit; invalid JSON response. |
| `Auth(msg)` | `2` | `<msg>` | 401 responses; missing credentials; failed PAT exchange. |
| `Config(msg)` | `3` | `<msg>` | Unknown command; invalid flags; TOML parse/write; IO; unsupported formData/header params; invalid JSON body. |
| `Network(msg)` | `4` | `<msg>` | Timeout (40 s on API / schema / auth); connection refused; DNS failure; 5xx from auth service. |

HTTP timeouts are 40 seconds for API requests, schema fetches, and auth/token refresh. See [Troubleshooting](#troubleshooting) for the errors whose cause/fix is non-obvious from the message text.

---

## Troubleshooting

### `error: not authenticated. ...`

No `HUBSTAFF_API_TOKEN` env var and no stored tokens.

```bash
hubstaff config set-pat YOUR_PERSONAL_ACCESS_TOKEN
# or for CI
export HUBSTAFF_API_TOKEN="..."
```

### `error: session expired. Run 'hubstaff config set-pat <TOKEN>' to re-authenticate`

The stored access token was 401, refresh was attempted, and the retried request was still 401 (refresh token invalid or revoked). Re-run `hubstaff config set-pat YOUR_PERSONAL_ACCESS_TOKEN`.

### `error: personal token exchange failed (<status>): <body>`

The `set-pat` POST to `<auth_url>/access_tokens` returned non-2xx. Most commonly a revoked/expired PAT, or a mismatched `auth_url`.

1. Verify the PAT in the Hubstaff developer portal.
2. Confirm `auth_url` with `hubstaff config show`. If customized for staging, make sure it matches.
3. Re-run `hubstaff config set-pat <TOKEN>`.

### `error: Couldn't refresh your session right now. ...`

The refresh POST did not return a usable 2xx. Two flavors:

- **"...Check your internet connection and try again."** — transport failure (timeout, DNS, connection refused). Check connectivity and `auth_url`.
- **"...The auth service is unavailable; retry shortly."** — auth service returned 5xx. Your credentials are fine; wait and retry.

In either case, stored tokens are untouched; no need to re-run `set-pat`.

### `error: schema fetch failed: <detail>`

The schema GET failed and there is no usable cached schema whose `source_url` matches the current `schema_url`. This is also what you'll see when you expect the CLI to fall back to an existing `schema/v2/docs.json` from a different environment — the `source_url` mismatch is deliberate, to prevent prod runs against a staging schema.

1. Verify `api_url` / `schema_url` with `hubstaff config show`.
2. If you are intentionally offline, confirm that `source_url` in `schema/v2/meta.toml` equals the current `schema_url`. If it doesn't, the cache will not be used by design.
3. Align `schema_url` with the cached `source_url`, or restore defaults:

   ```bash
   hubstaff config unset api_url
   hubstaff config unset schema_url
   ```

### Values starting with `--` are misparsed as flags

The argument parser treats `--` prefix as the next flag. Attach the value with `=`:

```bash
# wrong — '--literal' becomes the next flag
hubstaff foo bar --body-json --literal
# right
hubstaff foo bar --body-json='--literal'
```

---

## License

MIT. See [LICENSE](./LICENSE).
