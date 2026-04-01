# hubstaff-cli

Token-efficient CLI for the [Hubstaff Public API v2](https://developer.hubstaff.com). Built for LLM agents and power users.

## Installation

**One-liner** (macOS / Linux — auto-detects your platform):

```bash
curl -fsSL https://raw.githubusercontent.com/NetsoftHoldings/hubstaff-cli/master/install.sh | sh
```

**Cargo** (build from source):

```bash
cargo install --git https://github.com/NetsoftHoldings/hubstaff-cli
```

Or download binaries directly from [Releases](https://github.com/NetsoftHoldings/hubstaff-cli/releases).

## Authentication

### Personal Access Token (recommended for agents)

Get a token from [developer.hubstaff.com](https://developer.hubstaff.com) > Personal access tokens, then:

```bash
hubstaff-cli config set-pat YOUR_PERSONAL_TOKEN
```

This exchanges the token automatically and saves credentials that auto-refresh. No OAuth app needed.

### OAuth Browser Flow (interactive)

OAuth login requires a Hubstaff OAuth app (one-time setup):

```bash
hubstaff-cli config setup-oauth
```

This will prompt you for a Client ID and Client Secret. To get those:

1. Go to [developer.hubstaff.com](https://developer.hubstaff.com) > OAuth Apps > Create
2. Set the redirect URI to `http://localhost:19876/callback`
3. Copy the Client ID and Client Secret

Then authenticate:

```bash
hubstaff-cli login
```

Opens your browser to authenticate with Hubstaff. Tokens are saved and auto-refresh.

Alternatively, set the credentials via environment variables or a `.env` file:

```bash
export HUBSTAFF_CLIENT_ID="your_client_id"
export HUBSTAFF_CLIENT_SECRET="your_client_secret"
```

### Environment Variable (CI/automation)

```bash
export HUBSTAFF_API_TOKEN="your-access-token"
```

The env var takes priority over saved tokens.

## Quick Start

```bash
# Authenticate
hubstaff-cli config set-pat YOUR_PERSONAL_TOKEN

# Set a default organization
hubstaff-cli config set org 12345

# See who you are
hubstaff-cli users me

# List your organizations
hubstaff-cli orgs list

# List projects
hubstaff-cli projects list

# List members with names and emails
hubstaff-cli members list

# Invite someone
hubstaff-cli invites create --email new@hire.com --role project_user
```

## Commands

### Users

```bash
hubstaff-cli users me                  # Show authenticated user
hubstaff-cli users show <id>           # Show user by ID
```

### Organizations

```bash
hubstaff-cli orgs list                 # List organizations
hubstaff-cli orgs show <id>            # Show organization details
```

### Projects

```bash
hubstaff-cli projects list             # List projects (requires --org)
hubstaff-cli projects show <id>        # Show project details
hubstaff-cli projects create --name N  # Create project (requires --org)
```

### Members

```bash
hubstaff-cli members list                     # List org members (requires --org)
hubstaff-cli members list --project <id>      # List project members
hubstaff-cli members list --search-email E    # Filter by email
hubstaff-cli members list --search-name N     # Filter by name
hubstaff-cli members list --include-removed   # Include removed members

hubstaff-cli members create \                 # Create member (requires --org)
  --email user@co.com \
  --first-name Jane \
  --last-name Smith \
  --role project_user \                       # organization_manager, project_manager, project_user, project_viewer
  --project-ids 1,2,3 \                      # Comma-separated project IDs
  --team-ids 4,5                              # Comma-separated team IDs
  # Password: --password P, --password-stdin, or auto-generated

hubstaff-cli members remove --user-id <id>    # Remove member (requires --org)
```

### Invites

```bash
hubstaff-cli invites list                     # List invites (requires --org)
hubstaff-cli invites list --status pending    # Filter: all, pending, accepted, expired
hubstaff-cli invites show <id>                # Show invite details

hubstaff-cli invites create \                 # Send invite (requires --org)
  --email user@co.com \
  --role project_user \                       # organization_manager, project_manager, project_user, project_viewer
  --project-ids 1,2,3                         # Comma-separated project IDs

hubstaff-cli invites delete <id>              # Delete pending/expired invite
```

### Tasks

```bash
hubstaff-cli tasks list --project <id>        # List tasks for a project

hubstaff-cli tasks show <id>                  # Show task details

hubstaff-cli tasks create \                   # Create task
  --project <id> \
  --summary "Fix the login bug" \
  --assignee-id <user_id>                     # Optional assignee
```

### Activities

```bash
hubstaff-cli activities list \                # List activities (requires --org)
  --start 2026-03-20 \                        # ISO 8601 date or datetime (required)
  --stop 2026-03-27                           # Defaults to now if omitted
```

### Daily Activities

```bash
hubstaff-cli daily-activities list \          # List daily summaries (requires --org)
  --start 2026-03-01 \                        # Date YYYY-MM-DD (required)
  --stop 2026-03-31                           # Defaults to today if omitted
```

### Teams

```bash
hubstaff-cli teams list                       # List teams (requires --org)
hubstaff-cli teams show <id>                  # Show team details
```

### Notes

```bash
hubstaff-cli notes list \                     # List notes (requires --org)
  --start 2026-03-20 \                        # ISO 8601 date or datetime (required)
  --stop 2026-03-27                           # Defaults to now if omitted

hubstaff-cli notes create \                   # Create note
  --project <id> \
  --description "Finished the migration" \
  --recorded-time 2026-03-27
```

### Time Entries

```bash
hubstaff-cli time-entries create \            # Create manual time entry
  --project <id> \
  --start 2026-03-27T09:00:00Z \
  --stop 2026-03-27T17:00:00Z
```

### Configuration

```bash
hubstaff-cli config set org 12345             # Set default organization
hubstaff-cli config set api_url URL           # Set API URL (e.g., staging)
hubstaff-cli config set auth_url URL          # Set auth URL (e.g., staging)
hubstaff-cli config set token TOKEN           # Set access token directly
hubstaff-cli config set format compact        # Set output format: compact or json
hubstaff-cli config set-pat TOKEN             # Exchange personal access token
hubstaff-cli config setup-oauth               # Set up OAuth app credentials
hubstaff-cli config show                      # Show current configuration
hubstaff-cli login                            # OAuth browser login (requires setup-oauth)
hubstaff-cli logout                           # Clear saved tokens
```

## Global Flags

All commands support these flags:

```
--org <id>          Override default organization
--json              Full JSON output (default: compact)
--page-start <id>   Pagination cursor (record ID)
--page-limit <n>    Results per page (default: 100, max: 500)
```

## Output Formats

**Compact (default)** — token-efficient for agents:

```
USER_ID  NAME            EMAIL               ROLE   STATUS
101      Alice Johnson   alice@example.com   owner  active
102      Bob Smith       bob@example.com     admin  active
2 members | org:12345
```

**JSON (`--json`)** — full API response:

```json
{"members": [{"user_id": 130, ...}], "users": [...], "pagination": {...}}
```

## Staging / Custom Environments

```bash
hubstaff-cli config set api_url https://api.staging.hbstf.co/v2
hubstaff-cli config set auth_url https://account.staging.hbstf.co
```

## Agent Usage

Agents work best with a personal access token and compact output:

```bash
# One-time setup
hubstaff-cli config set-pat YOUR_PERSONAL_TOKEN
hubstaff-cli config set org 12345

# Agents can now run commands with minimal tokens
hubstaff-cli users me
hubstaff-cli members list
hubstaff-cli projects list
hubstaff-cli activities list --start 2026-03-20
hubstaff-cli members list --search-email john@company.com
```

## License

MIT
