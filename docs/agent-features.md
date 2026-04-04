# Agent Features

How `todui` fits long-running coding-agent workflows today, and which small CLI additions would make multi-agent coordination substantially cleaner.

## Current Agent-Friendly Features

These commands already make `todui` usable as an execution ledger:

- `todui session list`
  - tab-separated session summary for scripts and agents
- `todui session history [<session>]`
  - tab-separated revision summary for audit and rewind
- `todui session new`, `todui session tag`, `todui session repo`
  - stable repo session bootstrap and metadata refresh
- `todui add`, `todui edit`, `todui done`, `todui delete`
  - core todo lifecycle management
- `todui repo <repo>`
  - lookup across todo-level and session-level GitHub repo association
- `todui export md [<session>] --open-only --include-notes`
  - current best CLI read path for open work plus note bodies

Current strengths for agents:

- local-first and fast
- one session can act as a durable work ledger for a repo
- notes can hold acceptance criteria, commands, blockers, and handoff context
- revision history gives a lightweight audit trail for session mutations

## Current Pain Points

`todui` is usable for agents today, but the read/update ergonomics are still shaped more for humans than for multi-agent automation.

Main gaps:

- no dedicated per-session todo listing command
  - agents currently read open work through markdown export
- no single-todo inspect command
  - agents must infer todo ids from creation output, repo output, or exported markdown
- no structured JSON output on the script-facing commands
  - current tab-separated and markdown outputs are workable but brittle
- no note append operation
  - agents must rewrite the whole note body to add status or command traces
- no explicit claim, owner, heartbeat, or lease concept
  - safe multi-agent coordination needs better read primitives first

## Recommended Next Feature Slice

Prioritize structured read APIs and one small write improvement before any claim/lock design.

### 1. Add `todui todo list`

Proposed command:

```bash
todui todo list [<session>] [--open-only] [--format tsv|json]
```

Minimum behavior:

- default to the most recently opened session when `<session>` is omitted
- emit one row per todo
- include enough fields to let agents reconcile work without markdown parsing

Minimum fields:

- `id`
- `session`
- `title`
- `status`
- `note`
- `repo`
- `position`
- `created_at`
- `updated_at`
- `done_at`

### 2. Add `todui todo show`

Proposed command:

```bash
todui todo show <todo-id> [--session <session>] [--format json|md]
```

Minimum behavior:

- return one todo with full note body and metadata
- if `--session` is provided, verify membership
- support `json` for agents and `md` for human inspection

### 3. Add `todui edit --append-note`

Proposed extension:

```bash
todui edit <todo-id> [--session <session>] --append-note <text>
```

Minimum behavior:

- append a newline-delimited block to the existing note
- preserve empty-note behavior cleanly
- remain mutually exclusive with `--note` only if that keeps parsing simpler

This would remove the most awkward current pattern for agents: rewriting the full note body just to add one checkpoint.

### 4. Add `--format json` To Existing Read Commands

Priority commands:

- `todui session list`
- `todui session history`
- `todui repo`

Minimum requirement:

- keep current human-readable defaults unchanged
- add stable JSON output for automation

## Deferred Until After Structured Read APIs

Do not start with claim or locking features yet.

Defer:

- explicit claim owner
- heartbeat or lease renewal
- claimed/unclaimed filters
- stale-claim recovery

Reason:

- these features add coordination policy
- today the harder problem is reliable read/write ergonomics for agents
- once `todo list`, `todo show`, `--append-note`, and JSON outputs exist, ownership semantics can be designed against a cleaner CLI foundation

## Recommended Agent Workflow Today

Until the follow-on CLI slice lands, use this pattern:

1. bootstrap one repo session with `todui session new ... --tag agent`
2. set repo metadata with `todui session repo ... --set ...`
3. create milestone or task todos with `todui add`
4. store acceptance criteria, commands, and blockers in notes
5. read open work with `todui export md <session> --open-only --include-notes`
6. rewrite notes with `todui edit <id> --note ...`
7. close completed work with `todui done <id>`

This is good enough for one agent or a disciplined small swarm, but the structured read APIs above are the highest-leverage next improvement.
