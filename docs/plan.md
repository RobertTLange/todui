# Plan

Source of truth for execution: follow this file milestone by milestone.

Source spec: [spec.md](spec.md)

## Stop-And-Fix Rule

- After each milestone, run every validation command listed for that milestone.
- If any validation fails, stop.
- Fix root cause before starting the next milestone.
- Do not defer broken tests, lint failures, schema mismatches, or UX regressions.

## Intended Architecture

High-level flow:

`Terminal Event -> Input Mapper -> Action -> Reducer / Command Handler -> Domain Service -> SQLite Repository -> Updated App State -> Render`

Suggested layout:

```text
src/
  main.rs
  cli.rs
  config.rs
  app.rs
  action.rs
  reducer.rs
  error.rs
  domain/
  db/
  tui/
  export/
```

Design notes:

- Session = primary object.
- Head state lives in live tables; history lives in immutable snapshot tables.
- Keep business rules in domain/services, not in widgets.
- Use semantic theme tokens; no business-logic color branching.
- Keep files focused; split before they exceed ~500 LOC.

## Decision Notes

Fixed decisions; do not re-open unless spec changes:

- `todui resume` defaults to most recent session head.
- bare `todui` opens the session overview and does not mutate last-opened state by itself.
- Historical revisions are view-only and read-only.
- Revision strategy = full snapshot per successful session mutation.
- Todo deletion is a successful session mutation and creates a new snapshot revision.
- Session deletion is a hard delete and does not create a final revision.
- Markdown export default = GFM.
- Sessions may carry one optional tag; overview groups by tag and shows `untagged` last.
- Pomodoro lives inside session view only.
- Keyboard-first, mouse-complete, modeless navigation.
- SQLite config = WAL + foreign keys on + busy timeout + STRICT tables.
- Timestamps stored as UTC Unix epoch integers.

## Milestones

### Milestone 0: Scaffold + quality gate

Scope:

- Initialize Rust crate for `todui`.
- Add crate deps from spec.
- Set up module skeleton matching intended architecture.
- Add baseline config/path resolution for config + DB env overrides.
- Add `cargo fmt`, `clippy`, and test gate plumbing.

Acceptance criteria:

- Project builds as `todui`.
- Module tree exists and compiles.
- No placeholder code breaks lint/test gate.
- Default config/DB path resolution matches spec.

Validation commands:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

### Milestone 1: SQLite schema + basic CLI session/todo flows

Scope:

- Open DB, apply PRAGMAs, run migrations.
- Implement schema for sessions, todos, revisions, Pomodoro, app_state.
- Implement session create/list/open tracking.
- Implement `add`, `done`, `undone`.
- Implement most-recent-session resolution.
- Implement initial markdown export for head revisions.

Acceptance criteria:

- Sessions created by slug/name rules.
- `todui add` without `--session` uses most recent session; errors cleanly if none exists.
- Toggle done/undone updates timestamps correctly.
- CLI stdout/stderr and exit codes follow spec.
- Export emits valid GFM/plain text for live head.

Validation commands:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
todui session new "Writing Sprint"
todui add "Draft design spec" --session writing-sprint
todui done 1 --session writing-sprint
todui export md writing-sprint --format gfm
```

### Milestone 2: Head TUI session view

Scope:

- Implement bare `todui` overview flow that lists sessions and opens the selected session head.
- Implement `todui resume` head flow.
- Build top bar, list pane, details overlay, footer.
- Implement selection, keyboard navigation, Vim aliases, mouse row select, checkbox toggle hitbox.
- Render timestamps and semantic styles.
- Implement narrow-layout behavior.

Acceptance criteria:

- `todui` opens the overview TUI, including an empty state when no sessions exist.
- TUI opens the most recent session or requested session head.
- Keyboard-only flow covers all v1 head interactions.
- Mouse support works where terminal reports it, but app remains fully usable without it.
- Whole-row click selects only; checkbox click toggles only.
- Terminal state restores cleanly on exit and error.

Validation commands:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
todui resume writing-sprint
```

### Milestone 3: Revision snapshots + history UI + read-only revision mode

Scope:

- Create immutable snapshot revision on every successful session mutation.
- Add `session history` CLI.
- Add `resume --revision <n>`.
- Build history overlay in TUI.
- Add read-only banner, disabled mutations, and read-only toast behavior.

Acceptance criteria:

- Revision numbers increment per session starting at `1`.
- Snapshot rows match live state at mutation time.
- Historical CLI and TUI views show correct snapshot and block writes.
- Returning to head from revision view is one action away.

Validation commands:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
todui session history writing-sprint
todui resume writing-sprint --revision 1
```

### Milestone 4: Pomodoro model + session-view card

Scope:

- Implement Pomodoro persistence, state transitions, and global active-run uniqueness.
- Add session-linked and optional todo-linked runs.
- Build idle/running/paused summary UI in the session view.
- Dispatch `Tick` without per-second DB writes.
- Disable controls and show summary card in historical revision mode.

Acceptance criteria:

- Start/pause/resume/cancel/complete state machine obeys spec.
- Only one active or paused timer exists globally.
- Remaining time derives from persisted timestamps + in-process monotonic time.
- Historical view never offers mutating timer controls.

Validation commands:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
todui resume writing-sprint
```

### Milestone 5: Config, theme tokens, export polish, tests

Scope:

- Load config TOML for theme + Pomodoro defaults + key overrides.
- Finalize semantic theme tokens and status rendering.
- Complete export options: `--timestamps`, `--include-notes`, `--open-only`, `--revision`.
- Add/expand unit, repo, CLI, and TUI tests from spec.
- Add hard-delete support for todos and sessions in CLI and TUI, with TUI confirmation.
- Tighten error messages and final UX polish.

Acceptance criteria:

- Config overrides default values cleanly.
- Export behavior matches option matrix in spec.
- Test suite covers critical v1 flows.
- Todo delete preserves ordering invariants and revision history.
- Session delete removes the session and cascaded data cleanly.
- Full gate passes clean.

Validation commands:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
todui export md writing-sprint --revision 1 --timestamps full --include-notes
```

## Demo Sequence After Final Milestone

```bash
todui session new "Writing Sprint"
todui add "Draft design spec" --session writing-sprint
todui add "Review keybindings" --session writing-sprint --note "Ghostty + mouse"
todui delete 1 --session writing-sprint
todui resume writing-sprint
todui session history writing-sprint
todui session delete writing-sprint
todui resume writing-sprint --revision 2
todui export md writing-sprint --revision 2 --format gfm
```
