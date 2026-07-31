# Contributing a skill

This guide covers how to add a skill to `skills/`, including the tooling used to build the first
one and the verification we expect before a PR is opened.

If you solved something real with the `hubstaff` CLI and it took you more than an afternoon to get
right, that is the shape of a good skill. The parts that were hard to figure out are exactly the
parts worth writing down.

## What a skill is, and what it isn't

A skill is a Markdown playbook that teaches an AI agent to accomplish a task with this CLI. It is
**not** API reference documentation.

`hubstaff <command> --help` already prints every parameter, type, and description straight from the
live OpenAPI schema. Anything you copy out of it becomes a second copy that drifts. What the schema
*cannot* express is what makes a skill valuable:

- **Ordering** — what has to happen before what, and what breaks silently if you get it wrong
- **Idempotency** — which operations are safe to retry, and which double-count
- **Change detection** — how to find what moved since the last run when there are no webhooks
- **Silent failures** — the calls that return `200` and still leave you with wrong data

That last category is worth more than everything else combined. A loud error teaches itself. A
silent one costs somebody a weekend.

Explain **why**, not just what. An agent that understands the reason handles the case you didn't
anticipate; one following a rule blindly doesn't.

## How the first skill was built

`time-off-sync` was built with three things, and it's worth knowing which did what.

**1. The `skill-creator` skill** (from Anthropic's official Claude Code plugins) handled the
scaffolding and the parts that are easy to get subtly wrong:

```text
/skill-creator
```

It supplies the directory convention, the frontmatter contract, and two things you would otherwise
have to invent:

- **Description optimization.** The `description` field is the *only* thing that decides whether an
  agent reaches for your skill. `skill-creator` generates a set of should-trigger and
  should-not-trigger queries, then iterates on the description against them. This matters more than
  it sounds: a skill nobody triggers is a skill nobody has.
- **An eval harness.** It runs your test prompts through agents with and without the skill so you
  can see whether the skill actually changed the outcome, rather than assuming it did.

**2. Research subagents** to extract decision rules from the existing source material — in this case
a developer-portal guide and the OpenAPI document — without hand-copying and introducing errors.

**3. Verification against a running API.** This is the part that produced the real content. Roughly
half the material in `time-off-sync/reference/gotchas.md` came from *observing behaviour that the
documentation did not describe*, including four places where the published schema and the live API
disagree. None of it came from the generator.

So: use the tooling for structure and triggering. Get the content from the API.

## Why skills live in this repo

A skill is only as good as the commands it names, and those commands are synthesized from the OpenAPI
schema rather than hand-written. Keeping skills here lets CI assert that every command a skill names
still resolves. Hosted anywhere else they would rot silently the first time a path changed.

For the same reason, please **don't** copy a skill's content into another site or wiki. Link to it.
Two copies drift within a month.

## Workflow

### 1. Scope it

One skill, one job. Resist "a skill per endpoint group" — a skill earns its keep by encoding a
workflow, not by listing operations.

### 2. Draft

```text
skills/<skill-name>/
├── SKILL.md          # required — frontmatter + the playbook
├── reference/        # optional — detail loaded only when needed
└── examples/         # optional — request bodies for --body-file
```

`SKILL.md` loads into the agent's context every time the skill triggers, so keep it under about 500
lines. Push lookup tables into `reference/` and say when to read them. Long skills get skimmed.

Frontmatter:

```yaml
---
name: your-skill-name           # must match the directory name
description: >-                 # what it does AND when to use it — this is what triggers the skill
  One or two sentences. Use the words a person would actually say, not endpoint names.
license: MIT
metadata:
  cli-min-version: "0.5.0"
  scopes: "hubstaff:read + hubstaff:write; plus any permission level the task needs"
  verified: "when you checked and what you actually executed — say plainly what you couldn't verify"
  commands:                     # every CLI command the skill tells the agent to run
    - time_off_policies list
    - time_off_requests create
---
```

`metadata.commands` is a machine-checked contract, not decoration. CI resolves each entry against
the committed schema snapshot in `tests/fixtures/schema.json` and fails if one no longer exists. It
runs offline and does not call the API, so it catches changes to command synthesis rather than live
API drift — the fixture is refreshed with `just refresh-schema-fixture`, and that diff is where API
changes surface.

### 3. Verify against a real API

This is the step that separates a useful skill from a plausible one.

**Get the command names from the CLI, never from a URL.** Command words are derived from the path
plus HTTP method, and the result is not always what you'd guess — `PUT /v2/time_off_requests/{id}/status`
becomes `time_off_requests status <id>`, and `POST /v2/time_off_policies/{id}/archive` becomes
`time_off_policies archive create <id>`.

```bash
hubstaff list                          # every command the schema knows about
hubstaff <command> --help              # method, path, and every parameter
```

**Verify writes somewhere you can afford to be wrong.** Point the CLI at a local or sandbox
organization using an isolated config directory, so your production profile is untouched:

```bash
export XDG_CONFIG_HOME=~/hubstaff-sandbox-config
hubstaff config set api_url <YOUR_SANDBOX_API_URL>
hubstaff config set token <SANDBOX_TOKEN>
hubstaff config set organization <SANDBOX_ORG_ID>
hubstaff check
```

Any shell without that export still sees production. The schema cache is keyed on its source URL, so
a sandbox schema can never serve production commands.

**Test the failure paths too, deliberately.** Send the malformed body. Delete the thing that can't be
deleted. Run the same write twice and check whether it double-counted. Those results are the most
valuable lines in the finished skill, and you cannot get them by reading.

**If you can only reach production, say so.** Verify the reads, leave the writes unverified, and
record that in `metadata.verified`. An honest gap is more useful than a confident claim that turns
out to be wrong — a reader can work around a known gap.

### 4. Evaluate the skill (recommended)

`skill-creator` will run your test prompts with and without the skill and show the difference. Two
notes specific to this repo:

- **Make eval prompts plan-only.** Ask for the command sequence and payloads, not for execution.
  Otherwise the eval performs live writes against whatever org your credentials point at.
- **Assert on decisions, not prose.** Good checks look like "uses `time_off_requests status`, not
  `update_status`", "puts `remove_shifts` on the approval rather than the create", "chooses `replace`
  over `add` for a full-state sync".

Then run the description optimizer, so the skill triggers on what users actually type.

### 5. Self-check, then open the PR

```bash
just test                              # includes the skills lint and command-resolution check
hubstaff <command> --help              # once per entry in metadata.commands
```

Fill the repo's PR template. In the description, say **how you verified** — which organization type,
which operations you actually executed, and what you left unverified.

## Acceptance checklist

Reviewers check these:

- [ ] `name` matches the directory name and is unique across `skills/`
- [ ] `description` says both what the skill does and when to use it
- [ ] Every command in `metadata.commands` resolves — `hubstaff <command> --help` prints `Command:`
- [ ] `metadata.verified` states how reads and writes were checked, including any gaps
- [ ] Required scopes and permission level are stated
- [ ] **No tokens, PATs, or secrets** anywhere, including in example output
- [ ] **No real customer or employee data.** Time off messages, member names, and email addresses are
      personal data — invent them. Examples use synthetic ids.
- [ ] Every `examples/*.json` is valid JSON
- [ ] Destructive or irreversible operations tell the agent to confirm with the user first
- [ ] `cli-min-version` is a version you actually tested with
- [ ] `SKILL.md` doesn't restate what `--help` already prints

## A note on safety

Skills get executed by agents, sometimes with less supervision than you'd like. If a skill performs
writes that are hard to undo — overwriting a balance, approving leave, deleting a record — say so
explicitly and instruct the agent to show the payload and confirm before sending. Assume it will be
run by someone who has not read the API docs, because that is the point.
