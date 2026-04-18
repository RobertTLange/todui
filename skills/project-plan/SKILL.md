---
name: project-plan
description: Turn a project spec into a four-file planning pack under docs/ while tracking the work in todui.
---

# Project Plan Skill

Turn a project spec into a planning and execution document pack, and mirror the planning run inside `todui` so long-running agent work stays observable.

## Goal

Read the source spec and generate these files under `docs/`:

1. `prompt.md`
2. `plan.md`
3. `implement.md`
4. `documentation.md`

Use `todui` as the planning ledger throughout the run:

- create or reuse one deterministic session for this repo
- create fixed planning todos
- keep todo notes current with source path, assumptions, and done-when criteria
- mark planning work done as the pack lands
- leave unresolved assumptions as open follow-up todos

## Source Spec Resolution

Resolve the source spec in this order:

1. `$ARGUMENTS` if a usable path is provided
2. `./spec.md`
3. `./docs/spec.md`

If none exists, stop and ask for the spec path.

## `todui` Session Bootstrap

Before writing docs, bootstrap `todui`.

If `todui` is not installed yet, install it with `npm install -g @roberttlange/todui` or run it ad hoc with `npx -y @roberttlange/todui ...`.

1. Derive the session name from the repo directory:
   - session name: `agent-<repo-dir-slug>`
   - example for `todui`: `agent-todui`
2. Use session tag `agent`.
3. If git remote metadata is available, normalize it and set it as the session repo:
   - prefer `origin`
   - accept `https://github.com/owner/repo`, `git@github.com:owner/repo.git`, or `owner/repo`
   - store via `todui session repo <session> --set <owner/repo>`
4. Create the session if it does not exist yet.
5. Reuse the session if it already exists.

Suggested bootstrap flow:

```bash
repo_slug="$(basename "$PWD" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9]/-/g; s/-\\{2,\\}/-/g; s/^-//; s/-$//')"
session="agent-${repo_slug}"
repo_ref="$(git remote get-url origin 2>/dev/null || true)"

if ! todui session list | cut -f1 | grep -Fxq "$session"; then
  todui session new "$session" --tag agent
fi

if [ -n "$repo_ref" ]; then
  todui session repo "$session" --set "$repo_ref"
fi
```

## Planning Todo Set

Create or reuse exactly these planning todos in the repo session:

1. `Freeze target in docs/prompt.md`
2. `Break work into milestones in docs/plan.md`
3. `Write execution runbook in docs/implement.md`
4. `Record status in docs/documentation.md`

Todo notes must be rewritten to include the current planning context:

- source spec path
- key assumptions relevant to that todo
- done-when criteria relevant to that todo

Current `todui` does not support note append. When updating a todo, rewrite the full note body with `todui edit <id> --note ...`.

## Output Rules

- Create `docs/` if it does not exist.
- Overwrite the four target files if they already exist.
- Ground all content in the spec; do not invent product scope.
- Be explicit about assumptions when the spec is incomplete.
- Keep wording crisp and operational.
- Do not start implementation unless explicitly requested separately.

## File Requirements

### `docs/prompt.md`

Purpose: freeze the target so the agent does not build the wrong thing.

Include:

- Goals
- Non-goals
- Hard constraints
- Deliverables
- Done-when criteria
- Demo or smoke-test flow

### `docs/plan.md`

Purpose: turn the spec into milestones with concrete validation.

Include:

- Small milestones
- Acceptance criteria per milestone
- Validation commands per milestone
- Stop-and-fix rule if validation fails
- Decision notes that prevent oscillation
- Intended architecture

### `docs/implement.md`

Purpose: execution runbook for the coding agent.

Include:

- `plan.md` is the source of truth
- Execute milestone by milestone
- Run validation after each milestone
- Fix failures before moving on
- Keep diffs scoped
- Update docs continuously while shipping
- Load and follow the synced `clean-code` skill during implementation and refactor passes
- Follow repo clean-code guidance from `AGENTS.md` or equivalent project instructions when present

### `docs/documentation.md`

Purpose: shared memory and audit log for the project as it ships.

Include:

- Current milestone status
- What is done now
- What is next
- Decisions made and why
- How to run and demo
- Quick smoke tests
- Known issues
- Follow-ups

## Working Method

1. Read and summarize the source spec internally.
2. Bootstrap or reuse the repo `todui` session.
3. Create or refresh the four planning todos and their notes.
4. Identify missing details and record them as explicit assumptions.
5. Generate all four docs so they agree on scope, milestones, and constraints.
6. Mark completed planning todos done once the corresponding file is written and checked.
7. Convert unresolved assumptions or follow-ups into open `todui` todos in the same session.
8. Announce a planning checkpoint to Rob with `hibiki` after the pack is generated and the session state reflects it.
9. Report which spec path was used, which files were written, and which `todui` session tracks the work.

## Quality Bar

- No vague filler.
- No generic architecture unless the spec supports it.
- Milestones should be small enough for one focused execution loop.
- Validation steps should be executable whenever possible.
- The generated implementation guidance should reinforce clean-code expectations, not just delivery speed.
- `documentation.md` should be usable as handoff state after hours away.
- `todui` should mirror the planning state closely enough that another agent can resume from the open todos plus their notes.
