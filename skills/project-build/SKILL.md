---
name: project-build
description: Build a project from an existing project-plan pack under docs/ while using todui as the long-running execution ledger.
---

# Project Build Skill

Execute a project strictly from an existing project-plan pack, and track the run inside `todui` so another agent can resume from the current worklist without rereading the whole thread.

## Pack Resolution

Resolve the pack files in this order:

1. `$ARGUMENTS` if a usable docs directory or pack file path is provided
2. `./docs/prompt.md`
3. `./docs/plan.md`
4. `./docs/implement.md`
5. `./docs/documentation.md`

If the required pack files are missing, stop and ask for the pack location or for the `project-plan` skill to be used first.

## Authority Order

Treat these files in this order of authority:

1. `docs/prompt.md`
2. `docs/plan.md`
3. `docs/implement.md`
4. `docs/documentation.md`

If they conflict, follow the higher-authority file and record the conflict in `docs/documentation.md`.

## `todui` Session Rules

Use one deterministic repo session throughout execution.

1. Derive the session name as `agent-<repo-dir-slug>`.
2. Use session tag `agent`.
3. Reuse the existing session if present; otherwise create it.
4. Refresh the session repo from git remote metadata when available.
5. Keep all milestone and blocker tracking in this one session.

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

## Execution Rules

- Use `docs/plan.md` as the execution source of truth.
- Work one milestone at a time.
- Do not expand scope beyond the pack.
- Before editing, restate the current milestone, acceptance criteria, and validation steps.
- After each milestone, run the listed validations immediately.
- If validation fails, stop and fix before moving on.
- Keep diffs small and tightly scoped to the current milestone.
- Update `docs/documentation.md` continuously with status, decisions, commands run, validation results, and remaining work.
- If anything is unclear, make the best informed choice yourself from the pack, repo context, and existing patterns.
- Surface ambiguity or spec conflicts explicitly and record the choice you made and why.
- Do not silently change architecture, interfaces, or deliverables defined in the pack.
- Do not return early; continue until the full plan is implemented or you are blocked by a real external dependency.

## `todui` Worklist Protocol

Use `todui` throughout execution, not just at the start or end.

### 1. Sync Milestones Into Todos

Before implementation begins:

- read `docs/plan.md`
- create one milestone todo for each pending milestone if a matching open todo does not already exist
- use stable titles derived from the milestone heading
- write notes containing:
  - acceptance criteria
  - exact validation commands
  - known constraints or spec conflicts

Because current `todui` has no dedicated `todo list` command, inspect current work with:

```bash
todui export md "$session" --open-only --include-notes
```

Rewrite notes with `todui edit <id> --note ...` when the context changes. Current `todui` does not support note append.

### 2. Before Each Milestone

- read current open work from `todui export md "$session" --open-only --include-notes`
- restate the milestone from `docs/plan.md`
- update the active milestone todo note with:
  - current intent
  - narrowed acceptance criteria if needed
  - exact validation command(s) about to run

### 3. After Each Milestone

- run the listed validation commands immediately
- if validation passes:
  - mark the milestone todo done
  - refresh `docs/documentation.md`
- if validation fails:
  - do not lose the state in prose only
  - create or update blocker todos in the same session
  - capture the exact failing command and error in the blocker note
  - continue only after the blocker is resolved

### 4. Handoff State

At any pause or final handoff, `todui` should answer:

- what is open now
- what just finished
- what command should run next
- what blockers exist

Keep the open worklist small and current.

## Clean Code

- Load and follow the synced `clean-code` skill during implementation and refactor passes.
- Follow repo clean-code guidance from `AGENTS.md` or equivalent project instructions when present.
- Prefer clear names, small functions, low-duplication changes, and readable tests over speed-only delivery.
- Leave touched code cleaner than you found it.

## Start Sequence

Start with:

1. A short summary of the target
2. The current milestone you will execute first
3. The exact validation commands for that milestone
4. A `hibiki` checkpoint after the initial `todui` session and milestone sync are in place

Then begin implementation.

## Finish Sequence

Before handoff:

1. Run the final verification described in `docs/plan.md`
2. Confirm the done-when criteria in `docs/prompt.md`
3. Confirm the full plan has been implemented, not just the current milestone
4. Mark the final completed milestone todo done
5. Leave any known gap or follow-up as an open `todui` todo with a concrete note
6. Send a final `hibiki` checkpoint that the repo session is current
7. Summarize:
   - milestones completed
   - files changed
   - validation results
   - known issues or follow-ups
