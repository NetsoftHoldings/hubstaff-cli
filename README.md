# Hubstaff CLI

`hubstaff` is a command-line interface for the [Hubstaff Public API](https://developer.hubstaff.com/docs/hubstaff_v2).
Use it to work with organizations, projects, members, invites, tasks, and activity data directly from your terminal.

- Auth/config are explicit and stable (`login`, `logout`, `config`)
- API operations are loaded dynamically from `https://api.hubstaff.com/v2/docs`
- Schema is cached locally and reused when refresh fails

## Installation

**One-liner** (macOS / Linux):

```bash
curl -fsSL https://raw.githubusercontent.com/NetsoftHoldings/hubstaff-cli/master/install.sh | sh
```

**Cargo**:

```bash
cargo install --git https://github.com/NetsoftHoldings/hubstaff-cli
```

Or download binaries from [Releases](https://github.com/NetsoftHoldings/hubstaff-cli/releases).

## Authentication

### Personal Access Token (recommended)

```bash
hubstaff config set-pat YOUR_PERSONAL_TOKEN
```


### OAuth Browser Flow

```bash
hubstaff config setup-oauth
```

This will prompt you for a Client ID and Client Secret. To get those:

1. Go to [developer.hubstaff.com](https://developer.hubstaff.com) > OAuth Apps > Create
2. Set the redirect URI to `http://localhost:19876/callback`
3. Copy the Client ID and Client Secret

Then authenticate:

```bash
hubstaff login
```

Opens your browser to authenticate with Hubstaff. Tokens are saved and auto-refresh.

Alternatively, set the credentials via environment variables or a `.env` file:

```bash
export HUBSTAFF_CLIENT_ID="your_client_id"
export HUBSTAFF_CLIENT_SECRET="your_client_secret"
```

### CI / Automation

```bash
export HUBSTAFF_API_TOKEN="your-access-token"
```

`HUBSTAFF_API_TOKEN` takes precedence over saved tokens.

## Quick Start

```bash
# Authenticate
hubstaff config set-pat YOUR_PERSONAL_TOKEN

# Optional default organization for operations requiring organization_id
hubstaff config set organization 12345

# Dynamic API commands (schema-derived)
hubstaff users me
hubstaff projects list
hubstaff teams update_members 42
```

## Commands

### Dynamic API Commands

```bash
hubstaff <schema_command> [path_ids...]
hubstaff <schema_command> [path_ids...] [--query name=value ...]
hubstaff <schema_command> [path_ids...] [--<query_param> value ...]
hubstaff <schema_command> [path_ids...] [--body-json '{...}' | --body-file request.json]
hubstaff <schema_command> --help
```

Examples:

```bash
hubstaff users me
hubstaff projects list --page_limit 10
hubstaff projects get 123
hubstaff teams update_members 42
hubstaff time_off_policies archive 77
```

Notes:

- Path IDs are positional.
- Query parameters can be passed with operation-specific flags (for example `--page_limit 10`) or generic `--query name=value`.
- If `organization_id` is required and omitted from command shape, the CLI uses `config.organization`.

### Schema Commands

```bash
hubstaff schema show
hubstaff schema refresh
hubstaff schema refresh --force
```

### Config/Auth Commands

```bash
hubstaff config set organization 12345
hubstaff config set api_url URL
hubstaff config set auth_url URL
hubstaff config set schema_url URL
hubstaff config set token TOKEN
hubstaff config set format compact
hubstaff config set-pat TOKEN
hubstaff config setup-oauth
hubstaff config show

hubstaff login
hubstaff logout
```

## Global Flags

```bash
--json   Pretty-print JSON output
```

## Schema Cache Location

The schema cache is stored under your Hubstaff config directory:

- `schema/v2/docs.json`
- `schema/v2/meta.toml`
- `schema/v2/command_index.json`

On macOS this is typically:

- `~/Library/Application Support/hubstaff/schema/v2/docs.json`
- `~/Library/Application Support/hubstaff/schema/v2/meta.toml`

## Staging / Custom Environments

```bash
hubstaff config set api_url <api_url>
hubstaff config set auth_url <auth_url>
# Optional if schema endpoint differs from <api_url>/docs
hubstaff config set schema_url <api_url>/v2/docs
```

## License

MIT
