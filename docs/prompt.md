# Prompt

Source spec: [spec.md](spec.md)

## Goals

- Build a local-first terminal app named `todui`.
- Ship a full-screen TUI for browsing and editing session-scoped todos.
- Ship a CLI for session create/list/history, to-do add/toggle, resume, and markdown export.
- Persist all app state in one local SQLite database file.
- Support immutable per-session revision history and read-only revision browsing.
- Support optional Pomodoro runs inside the session view.
- Keep the app keyboard-first, with mouse support as an additive layer.

## Non-Goals

- No cloud sync.
- No multi-user collaboration.
- No due dates, recurring tasks, deletion, dependencies, or NL parsing.
- No writable historical revisions.
- No separate app-global timer screen.
- No event-sourced history model in v1.

## Hard Constraints

### Platform

- Rust stable.
- Ratatui + Crossterm for TUI and terminal events.
- SQLite single-file persistence.
- Binary name: `todui`.

### Persistence

- Open one SQLite connection per process lifetime.
- On open: `PRAGMA journal_mode = WAL;` and `PRAGMA foreign_keys = ON;`.
- Set SQLite busy timeout to `5000 ms`.
- Use STRICT tables for core entities.
- Store timestamps as UTC Unix epoch integers only.
- Revision model: immutable full-session snapshots, numbered per session starting at `1`.
- Only one active or paused Pomodoro may exist globally at once.

### UX

- Keyboard-first at all times.
- Mouse-complete where terminal supports reporting.
- Modeless navigation; Vim keys are aliases, not a modal editor.
- Row click selects; checkbox click toggles; whole-row click must not toggle completion.
- Historical revisions are read-only with visible banner and disabled mutations.
- Must remain usable in narrow terminals:
  - `>=100` cols: two-pane.
  - `70-99` cols: list + drawer.
  - `<70` cols: single pane with modal details.

### Performance and Reliability

- Fast startup and fast post-write response.
- Poll terminal events on a bounded cadence.
- Do not write Pomodoro timer ticks to the DB every second.
- Restore terminal state cleanly on shutdown every time.
- Keep CLI stdout/stderr split cleanly and use non-zero exit on failure.

## Deliverables

- Rust crate building a `todui` binary.
- SQLite schema + migrations for sessions, todos, revisions, Pomodoro runs, and app state.
- CLI commands:
  - `todui session new <name> [--slug <slug>] [--tag <tag>]`
  - `todui session list`
  - `todui session history [<session>]`
  - `todui session tag [<session>] [--set <tag> | --clear]`
  - `todui add <title> [--session <session>] [--note <text>]`
  - `todui done <todo-id> [--session <session>]`
  - `todui undone <todo-id> [--session <session>]`
  - `todui resume [<session>] [--revision <n>]`
  - `todui export md [<session>] [--revision <n>] [--output <file>] [--format gfm|plain] [--timestamps full|compact|none] [--include-notes] [--open-only]`
- TUI session view with top bar, todo list, details overlay, footer, overlays/modals, timestamps, semantic theme tokens, keyboard + mouse support.
- Revision history overlay and read-only historical revision mode.
- Pomodoro card inside session view with start/pause/resume/cancel and summary behavior in historical mode.
- Markdown export for head and historical revisions.
- Tests for domain logic, repositories, CLI flows, and core TUI behavior.
- Project docs:
  - [plan.md](plan.md)
  - [implement.md](implement.md)
  - [documentation.md](documentation.md)

## Done When

### Checks

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`
- `cargo test -- --nocapture` for any flaky/interactive diagnostics needed
- Targeted smoke runs for CLI and export flows

### Demo Flow

1. Create session:
   `todui session new "Writing Sprint" --tag work`
2. Add todos:
   `todui add "Draft design spec" --session writing-sprint`
3. Open TUI at head:
   `todui resume writing-sprint`
4. Toggle a todo from keyboard and verify timestamps/details update.
5. Open revision history overlay, select an old revision, verify read-only banner and blocked mutation.
6. Start, pause, resume, and cancel a Pomodoro from the session view.
7. Export markdown for head and a historical revision:
   - `todui export md writing-sprint`
   - `todui export md writing-sprint --revision 2 --format gfm`
8. Verify stdout/stderr behavior and non-zero exits for missing session / revision errors.
