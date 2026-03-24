# Documentation

Shared status log and shipping notes.

Primary references:

- [spec.md](spec.md)
- [plan.md](plan.md)
- [implement.md](implement.md)

## Current Milestone Status

- Current state: planning/docs only.
- Completed:
  - source spec captured in [spec.md](spec.md)
  - execution docs created
- Next:
  - Milestone 0: scaffold Rust crate, quality gate, path/config baseline
- After that:
  - Milestone 1: schema + basic CLI flows
  - Milestone 2: head TUI
  - Milestone 3: revisions/history/read-only mode
  - Milestone 4: Pomodoro
  - Milestone 5: config/theme/export polish/tests

## Decisions Made

- Source-of-truth spec path for this repo is [spec.md](spec.md); no top-level `specs.md` exists.
- User-facing app name and CLI name are `todui`.
- Execution source of truth is [plan.md](plan.md).
- Runbook for future implementation lives in [implement.md](implement.md).
- Documentation file stays live and should be updated after each milestone, not only at release.
- Fixed product decisions inherited from spec:
  - session-centric model
  - immutable full-snapshot revisions
  - read-only historical mode
  - Pomodoro embedded in session view
  - GFM default export
  - keyboard-first plus additive mouse support

## How To Run + Demo

Current repo state:

- No Rust crate yet.
- No runnable binary yet.
- Demo commands below are target smoke tests once Milestone 1+ lands.

Target smoke commands:

```bash
todui session new "Writing Sprint"
todui add "Draft design spec" --session writing-sprint
todui resume writing-sprint
todui session history writing-sprint
todui export md writing-sprint --format gfm
```

Final demo target:

```bash
todui session new "Writing Sprint"
todui add "Draft design spec" --session writing-sprint
todui add "Review keybindings" --session writing-sprint --note "Ghostty + mouse"
todui resume writing-sprint
todui resume writing-sprint --revision 2
todui export md writing-sprint --revision 2 --timestamps full --include-notes
```

## Known Issues / Follow-Ups

- App implementation not started yet.
- Validation commands in [plan.md](plan.md) are future-facing until crate scaffold exists.
- Need to convert this log from planning state to execution log once Milestone 0 starts.
