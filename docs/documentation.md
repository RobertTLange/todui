# Documentation

Shared status log and shipping notes.

Primary references:

- [spec.md](spec.md)
- [plan.md](plan.md)
- [implement.md](implement.md)

## Current Milestone Status

- Current state: implementation complete.
- Completed:
  - source spec captured in [spec.md](spec.md)
  - execution docs created
  - Milestone 0: Rust crate scaffold, module tree, config/path resolution, baseline gate
  - Milestone 1: SQLite schema, session/todo CLI flows, live markdown export, revision snapshots under the hood
  - Milestone 2: head TUI session view, navigation, mouse hitbox behavior, terminal cleanup
  - Milestone 3: revision history CLI/TUI flow, read-only revision mode, return-to-head behavior
  - Milestone 4: Pomodoro persistence, state machine, session card, active-run uniqueness
  - Milestone 5: config-driven theme/durations, export option matrix, expanded tests
- Next:
  - optional UX refinement only; plan scope is complete

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
- Milestone 0 implementation choices:
  - binary + library crate split so CLI, DB, export, and TUI layers are testable
  - config defaults and path resolution follow spec exactly, with env overrides for config + DB
  - module tree mirrors the target architecture so later milestones can fill behavior without structural churn
- Milestone 1 implementation choices:
  - SQLite schema follows the spec tables and PRAGMAs, using `PRAGMA user_version` for migration tracking
  - session mutations already create immutable full snapshots, which keeps later history/read-only work simple
  - CLI outputs stay compact and scriptable: identifiers / tab-separated summaries to stdout, errors via process exit path
- Milestone 2-5 implementation choices:
  - bare `todui` now opens a real ratatui+crossterm session overview, while `resume` stays the direct session opener
  - the overview is browse-only; session recency changes only when a session is actually entered, and `o` returns from a session back to the overview
  - revision viewing reuses the same screen with immutable snapshot data and a read-only banner/toast path
  - Pomodoro math is derived from persisted timestamps plus in-process redraw cadence; no per-second DB writes
  - config currently drives theme mode/accent, Pomodoro durations, and additive key aliases for the configured v1 actions
  - in-TUI creation now uses modal forms: `n` in overview creates a session from its display name, and `n` in a live session creates a todo with title + notes
  - todo editing now reuses that modal path: `e` edits the selected live todo in TUI, and `todui edit` performs partial title/note updates from CLI
  - delete is now supported end-to-end: `todui delete <id>` removes one todo with a new snapshot revision, `todui session delete [session]` hard-deletes a session, and TUI uses explicit confirmation modals for both
  - historical revisions remain mutation-blocked, including both delete actions
  - overview/session navigation now supports arrow traversal across screens: `Right` opens the selected session from overview, and `Left` returns from a session to overview

## How To Run + Demo

Current repo state:

- Rust crate initialized.
- `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` pass at milestone 0.
- Milestone 1 smoke commands now pass:

```bash
target/debug/todui session new "Writing Sprint"
target/debug/todui add "Draft design spec" --session writing-sprint
target/debug/todui done 1 --session writing-sprint
target/debug/todui export md writing-sprint --format gfm
```

Final validation commands run clean:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
target/debug/todui
target/debug/todui session history writing-sprint
target/debug/todui resume writing-sprint
target/debug/todui resume writing-sprint --revision 1
target/debug/todui edit 1 --session writing-sprint --title "Draft final design spec" --clear-note
target/debug/todui delete 1 --session writing-sprint
target/debug/todui session delete writing-sprint
target/debug/todui export md writing-sprint --revision 1 --timestamps full --include-notes
```

Target smoke commands:

```bash
todui session new "Writing Sprint"
todui add "Draft design spec" --session writing-sprint
todui
todui resume writing-sprint
todui session history writing-sprint
todui edit 1 --session writing-sprint --title "Draft final design spec" --clear-note
todui delete 1 --session writing-sprint
todui session delete writing-sprint
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

TUI create flow:

- `todui`
- `n` to create a session from the overview
- `Enter` to open the new session head
- `n` again to add a todo with optional notes inside the session view
- `e` on the selected todo to edit title and notes in the same modal
- `d` on the selected live todo to open a delete confirmation
- `D` in overview or a live session to open a session delete confirmation

## Known Issues / Follow-Ups

- No open blockers against [plan.md](plan.md).
- Follow-up space, if wanted later: add deeper render/input coverage around modal focus movement, cancel paths, and no-op edits.
