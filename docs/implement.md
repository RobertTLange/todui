# Implement

Execution runbook for Codex.

Primary references:

- [plan.md](plan.md)
- [spec.md](spec.md)
- [documentation.md](documentation.md)

## Operating Rules

### Plan Is Source Of Truth

- Follow [plan.md](plan.md) milestone by milestone.
- Do not skip ahead because a later feature feels convenient.
- If implementation pressure conflicts with the plan, prefer the plan.
- If the plan and spec disagree, prefer the spec and update the plan plus documentation in the same diff.

### Validate After Every Milestone

- Run every validation command listed under the active milestone in [plan.md](plan.md).
- If any command fails, stop.
- Repair immediately before touching the next milestone.
- Do not leave failing lint/tests as “known for later”.

### Keep Diffs Scoped

- One milestone at a time.
- Only edit files needed for the current milestone.
- Do not mix opportunistic refactors with feature work unless needed to keep the milestone shippable.
- Keep files focused; split modules before they become large or muddled.
- Preserve fixed decisions from the spec:
  - session primary object
  - read-only historical revisions
  - full-snapshot revisioning
  - Pomodoro inside session view
  - keyboard-first, mouse-complete, modeless navigation

### Update Documentation Continuously

- Update [documentation.md](documentation.md) during execution, not at the end only.
- Record:
  - current milestone status
  - decisions made and why
  - new run/demo commands
  - known issues or follow-ups
- If the plan changes, update both [plan.md](plan.md) and [documentation.md](documentation.md).

## Standard Loop

1. Read active milestone in [plan.md](plan.md).
2. Re-read relevant spec sections in [spec.md](spec.md).
3. Implement the smallest end-to-end slice that satisfies the milestone.
4. Run milestone validations.
5. Fix failures until clean.
6. Update [documentation.md](documentation.md).
7. Move to next milestone only after clean validation.

## Execution Priorities

- Root-cause fixes over band-aids.
- Deterministic behavior over clever abstractions.
- Keyboard path first; mouse as additive.
- CLI and repository correctness before TUI polish.
- Terminal cleanup and DB correctness are release blockers.

## Non-Negotiables

- No writable historical revisions.
- No whole-row click completion toggle.
- No per-second DB writes for Pomodoro ticks.
- No hard-coded state colors in business logic.
- No broken stdout/stderr split in CLI.
- No milestone completion claim without running validations.
