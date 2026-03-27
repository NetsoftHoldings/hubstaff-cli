# hubstaff-cli

Token-efficient CLI for the [Hubstaff Public API v2](https://developer.hubstaff.com). Built for LLM agents and power users.

## Installation

Download the latest binary from [Releases](https://github.com/chocksy/hubstaff-cli/releases):

```bash
# macOS (Apple Silicon)
curl -L https://github.com/chocksy/hubstaff-cli/releases/latest/download/hubstaff-cli-aarch64-apple-darwin.tar.gz | tar xz
mv hubstaff-cli /usr/local/bin/

# macOS (Intel)
curl -L https://github.com/chocksy/hubstaff-cli/releases/latest/download/hubstaff-cli-x86_64-apple-darwin.tar.gz | tar xz
mv hubstaff-cli /usr/local/bin/

# Linux (x86_64)
curl -L https://github.com/chocksy/hubstaff-cli/releases/latest/download/hubstaff-cli-x86_64-unknown-linux-musl.tar.gz | tar xz
mv hubstaff-cli /usr/local/bin/
```

Or build from source:

```bash
cargo install --git https://github.com/chocksy/hubstaff-cli
```

## Authentication

### OAuth Browser Flow (interactive)

```bash
hubstaff-cli login
```

Opens your browser to authenticate with Hubstaff. Tokens are saved automatically.

### API Token (agents/automation)

```bash
export HUBSTAFF_API_TOKEN="your-oauth-token"
```

Or persist it:

```bash
hubstaff-cli config set token "your-oauth-token"
```

## Quick Start

```bash
# Set a default organization
hubstaff-cli config set org 12345

# See who you are
hubstaff-cli users me

# List your organizations
hubstaff-cli orgs list

# List projects
hubstaff-cli projects list

# List members
hubstaff-cli members list

# Invite someone
hubstaff-cli invites create --email new@hire.com --role project_user
```

## Commands

| Command | Description |
|---------|-------------|
| `users me` | Show authenticated user |
| `users show <id>` | Show user by ID |
| `orgs list` | List organizations |
| `orgs show <id>` | Show organization |
| `projects list` | List projects (requires `--org`) |
| `projects show <id>` | Show project |
| `projects create --name N` | Create project |
| `members list` | List org members (or `--project P` for project members) |
| `members create --email E --first-name F --last-name L` | Create member |
| `members remove --user-id U` | Remove member |
| `invites list` | List invites |
| `invites show <id>` | Show invite |
| `invites create --email E` | Send invite |
| `invites delete <id>` | Delete invite |
| `tasks list --project P` | List tasks |
| `tasks show <id>` | Show task |
| `tasks create --project P --summary S` | Create task |
| `activities list --start DATE` | List activities |
| `daily-activities list --start DATE` | List daily summaries |
| `teams list` | List teams |
| `teams show <id>` | Show team |
| `notes list --start DATE` | List notes |
| `notes create --project P --description D --recorded-time T` | Create note |
| `time-entries create --project P --start T --stop T` | Create time entry |
| `config set KEY VALUE` | Set config value |
| `config show` | Show config |
| `login` | OAuth browser login |
| `logout` | Clear tokens |

## Global Flags

```
--org <id>          Override default organization
--json              Full JSON output (default: compact)
--page-start <id>   Pagination cursor
--page-limit <n>    Results per page (default: 100, max: 500)
```

## Output Formats

**Compact (default)** -- token-efficient for agents:

```
ID      NAME            EMAIL               ROLE              STATUS
884421  Jane Smith      jane@acme.co        owner             active
884422  Bob Jones       bob@acme.co         project_manager   active
2 members | org:123
```

**JSON (`--json`)** -- full API response:

```json
{"members": [{"id": 884421, ...}], "pagination": {"next_page_start_id": 884423}}
```

## Configuration

Config file: `~/.config/hubstaff-cli/config.toml` (or `$XDG_CONFIG_HOME/hubstaff-cli/config.toml`)

```bash
hubstaff-cli config set org 12345
hubstaff-cli config set api_url https://staging.api.hubstaff.com/v2
hubstaff-cli config show
```

## Agent Usage

Agents get the best experience with an env var token and compact output:

```bash
export HUBSTAFF_API_TOKEN="..."
hubstaff-cli config set org 12345

# Now agents can run commands with minimal tokens
hubstaff-cli members list
hubstaff-cli projects list
hubstaff-cli activities list --start 2026-03-20
```

## License

MIT
