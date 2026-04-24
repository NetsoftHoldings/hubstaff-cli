# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
