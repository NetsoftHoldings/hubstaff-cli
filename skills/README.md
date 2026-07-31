# Hubstaff CLI agent skills

Markdown playbooks that teach an AI agent to accomplish a task with the `hubstaff` CLI. Plain files —
no MCP server, no extra credentials.

| Skill | What it does | Needs CLI |
|---|---|---|
| [`time-off-sync`](./time-off-sync/) | Mirror time off policies, balances, requests, and approvals from an external HR system into Hubstaff | `0.5.0` |

## Requirements

These skills run the `hubstaff` CLI, so they need **a shell with the CLI installed and network access
to the Hubstaff API**. That means they work in agents running on your own machine. They will not work
in sandboxed hosted surfaces: skills on the Claude API run in a container with no network access, and
claude.ai's sandbox has no `hubstaff` binary.

## Install

**Claude Code** — copy the directory to `~/.claude/skills/` (personal) or `.claude/skills/`
(shared with a repo), then describe your task; Claude matches on the skill's `description`.

```bash
cp -r skills/time-off-sync ~/.claude/skills/
```

**Any other agent** — a skill is just Markdown. If your tool has no skill format, point it at
`SKILL.md` as an instruction or rules file and keep `reference/` alongside so it can read the detail
on demand.

Skills don't sync between surfaces; each one is installed separately.

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) — authoring workflow, required verification, acceptance
checklist, and why skills live in this repo rather than in the docs site.
