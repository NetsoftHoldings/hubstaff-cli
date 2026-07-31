# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-07-31

### Added
- `skills/` — agent skills for this CLI, published as Markdown so customers can contribute their own
  by PR. First skill is `time-off-sync`, for mirroring time off from an external HR system into
  Hubstaff. See [`skills/CONTRIBUTING.md`](skills/CONTRIBUTING.md).
- A test guard asserting every command a skill declares still resolves against the schema fixture,
  so a change in path-to-command derivation fails CI instead of silently breaking published skills.

### Changed
- `hubstaff check` now exits `0` instead of `1` when the only problem was a stored token without an
  `expires_at`. Scripts that gate on the exit code will see previously-failing setups start passing.

### Fixed
- `Token validity` no longer reports `FAIL` for a long-lived token with no `expires_at`. Raw tokens
  (`config set token`) and organization access tokens (`hsoat_…`) are non-refreshable by design, so
  the row failed on a working setup and suggested `config set-pat`, which would have replaced the
  credential. It is now `WARN`: with no refresh token there is nothing to remediate, and with one the
  detail explains that proactive refresh is disabled. `check` never refreshes for this row, so it has
  no side effect on stored credentials.

## [0.4.0] - 2026-05-01

### Removed
- **Breaking:** `HUBSTAFF_API_TOKEN` environment variable. Use `hubstaff config
  set-pat` instead.
- `Env token shadowing` row from `hubstaff check` (no longer applicable). The
  diagnostic table is now 8 rows.

### Fixed
- `hubstaff check` perms remediation now quotes the path, so config dirs
  containing spaces (e.g. macOS `~/Library/Application Support/hubstaff`) can be
  copy-pasted directly.
- `hubstaff check` perms diagnostic prints bare octal (`700` / `755`) instead of `0o700` / `0o755`.

## [0.3.1] - 2026-04-30

### Added
- Homebrew distribution — the release workflow now publishes to a Homebrew tap.

## [0.3.0] - 2026-04-24

### Added
- `hubstaff list` command — prints every available API command grouped by resource.
- `hubstaff check` command — diagnostic checks for config, credentials, and API reachability.
- `--pretty` / `-p` global flag for colorized, pretty-printed JSON output; honored via the
  `format` config key as well.
- Proactive OAuth token refresh before expiry, so long-running sessions don't fail mid-call.
- Dynamic API command surface: endpoints are discovered from the live Hubstaff OpenAPI schema
  rather than hardcoded, so the CLI tracks the API without a rebuild.

### Changed
- Dependency bumps: `sha2` 0.10 → 0.11, `toml` 0.8 → 1.1, `rand` 0.9 → 0.10, plus patch-level
  updates across the tree.
