App Name: todui

Below is an implementation-ready design spec. It is opinionated so a developer can build from it without inventing the architecture first.

This spec assumes Rust + Ratatui + Crossterm + SQLite. That is a good fit here because Ratatui expects the app to own its draw/input loop, Ratatui does not handle input for you, and Crossterm exposes explicit mouse capture and event polling. On the terminal side, Ghostty allows terminal apps to receive mouse events only when they request mouse reporting, and users can disable that behavior with mouse-reporting = false or toggle it at runtime. On storage, SQLite defaults to rollback journal mode, requires PRAGMA journal_mode=WAL; to enable WAL, does not enforce foreign keys unless PRAGMA foreign_keys=ON is set, and STRICT tables enforce declared datatypes. For markdown export, GitHub Flavored Markdown defines task-list checkbox syntax ([ ] / [x]) as an extension, so export should default to GFM and offer a plain fallback for maximum portability.  ￼

1. Product summary

Build a local-first terminal app named todui with:
	•	a full-screen TUI for browsing and editing to-do sessions
	•	a CLI for creating sessions, resuming sessions, adding/editing todos, inspecting history, and exporting markdown
	•	session-specific revision history
	•	optional Pomodoro support with a shared active footer in overview and live session views
	•	mouse and keyboard navigation
	•	Vim-style navigation aliases in addition to arrows
	•	clickable checkboxes for toggling done state
	•	timestamps and colors in the TUI
	•	SQLite-backed persistence in a single local database file

2. Core concepts

2.1 Session

A session is the main container of work. It is a named list of todos.

Examples:
	•	work
	•	writing
	•	errands

A session has:
	•	identity: slug + display name
	•	optional tag for overview grouping and CLI metadata
	•	current live head
	•	immutable revision history
	•	zero or more todos

2.2 Revision

A revision is an immutable snapshot of a session’s todo state at a point in time.

Revisions are:
	•	numbered per session, starting at 1
	•	read-only once created
	•	browsable in CLI and TUI
	•	the target for markdown export

2.3 Todo

A todo belongs to exactly one session.

A todo has:
	•	title
	•	optional notes
	•	completion state
	•	ordering position
	•	timestamps

2.4 Pomodoro run

A Pomodoro run is a global timer event and may optionally be attached to one todo.

A run is one of:
	•	focus
	•	short break
	•	long break

Only one active run may exist globally at any time.

3. Goals

3.1 Functional goals

The app shall:
	•	create and list sessions
	•	open a session overview when launched without a subcommand
	•	resume the most recent session by default
	•	resume a named session
	•	open a previous revision of a session
	•	add todos from CLI and TUI
	•	edit todo title and notes from CLI and TUI
	•	delete todos from CLI and TUI
	•	delete sessions from CLI and TUI
	•	assign or clear an optional session tag from CLI and TUI overview
	•	toggle todo completion from CLI and TUI
	•	display timestamps in the TUI
	•	export a markdown text version of a session
	•	show active Pomodoro status in overview and live session views

3.2 UX goals

The app shall be:
	•	keyboard-first
	•	fully usable without mouse
	•	mouse-complete where supported
	•	modeless for navigation
	•	scriptable from CLI
	•	fast on startup and after writes
	•	readable in narrow terminals

4. Non-goals for v1

Do not implement these in v1:
	•	cloud sync
	•	multi-user collaboration
	•	due dates
	•	recurring tasks
	•	task dependencies
	•	natural-language parsing
	•	restore/fork-from-revision write workflows

Revisions are viewable in v1, not writable.

5. UX principles

5.1 Keyboard-first, mouse-complete

Because Ghostty can stop reporting mouse input if the user disables mouse reporting, the app must remain fully usable by keyboard at all times. Mouse support is a convenience layer, not a dependency.  ￼

5.2 Modeless navigation

Do not build a full Vim modal editor.

Instead:
	•	arrows work everywhere sensible
	•	Vim movement keys are aliases
	•	text input only occurs inside explicit text-entry widgets
	•	no “normal vs insert mode” for general navigation

5.3 Separate selection from completion
	•	click row = select todo
	•	click checkbox = toggle done
	•	Enter = open selected todo details
	•	e = edit selected todo
	•	d = delete selected todo with confirmation
	•	D = delete current session with confirmation
	•	Space or x = toggle done on selected todo

Do not make whole-row click toggle completion.

5.4 Historical revisions are read-only

When viewing an old revision:
	•	show a banner
	•	disable all mutations
	•	disable checkbox toggles
	•	disable timer controls
	•	allow return to live head quickly

6. Recommended stack

6.1 Runtime
	•	Rust stable
	•	Tokio is optional, not required
	•	Prefer synchronous SQLite access in v1

6.2 Crates

Recommended:
	•	ratatui
	•	crossterm
	•	rusqlite
	•	clap
	•	serde
	•	toml
	•	time or chrono
	•	anyhow or thiserror

6.3 Why this stack

Ratatui explicitly expects the app to render each frame and handle events itself, and its own docs recommend using backend event APIs directly. Crossterm exposes mouse capture and event polling explicitly, which fits a centralized action/reducer design well.  ￼

7. Filesystem layout

Default paths:
	•	config: ~/.config/todui/config.toml
	•	database: ~/.local/share/todui/todui.db

Environment overrides:
	•	TODO_TUI_CONFIG
	•	TODO_TUI_DB

8. CLI specification

Binary name: todui

8.1 Command summary

todui
todui session new <name> [--slug <slug>] [--tag <tag>]
todui session delete [<session>]
todui session list
todui session history [<session>]
todui session tag [<session>] [--set <tag> | --clear]

todui add <title> [--session <session>] [--note <text>]
todui delete <todo-id> [--session <session>]
todui edit <todo-id> [--session <session>] [--title <text>] \
  [--note <text> | --clear-note]
todui done <todo-id> [--session <session>]
todui undone <todo-id> [--session <session>]

todui resume [<session>] [--revision <n>]

todui export md [<session>] [--revision <n>] [--output <file>] \
  [--format gfm|plain] [--timestamps full|compact|none] \
  [--include-notes] [--open-only]

8.2 Resolution rules

todui resume
	•	no args: open the live head of the most recently opened session
	•	with <session>: open the live head of that session
	•	with --revision N: open that session revision read-only

todui add
	•	if --session is omitted, add to the most recently opened session head
	•	if no session exists, return an error

todui delete
	•	if --session is provided, the todo must belong to that session
	•	deletes immediately in CLI
	•	compacts remaining todo ordering positions

todui edit
	•	requires at least one of --title, --note, or --clear-note
	•	if --session is provided, the todo must belong to that session
	•	--note and --clear-note are mutually exclusive
	•	omitted fields keep their current value

todui session delete
	•	with <session>: deletes that session
	•	no args: deletes the most recently opened session
	•	deletes the live session plus todos, revision snapshots, and Pomodoro runs

todui session tag
	•	with <session>: updates that session tag
	•	no args: updates the most recently opened session
	•	`--set` normalizes the tag to slug-style text
	•	`--clear` removes the tag and returns the session to the `untagged` overview section

todui export md
	•	defaults to most recent session head
	•	with --revision N, exports that revision
	•	output goes to stdout by default
	•	--output writes to file instead

8.3 Session identifier rules

CLI accepts session slug, not numeric DB id.

Slug rules:
	•	lowercase
	•	alphanumeric plus -
	•	unique
	•	auto-generate from name if not provided

8.4 CLI stdout/stderr rules
	•	command result data goes to stdout
	•	diagnostics and errors go to stderr
	•	success returns exit code 0
	•	failure returns non-zero

8.5 CLI examples

todui session new "Writing Sprint"
todui add "Draft design spec" --session writing-sprint
todui delete 1 --session writing-sprint
todui edit 1 --session writing-sprint --title "Draft final design spec" --clear-note
todui add "Review keybindings" --session writing-sprint --note "Ghostty + mouse"
todui session delete writing-sprint
todui resume writing-sprint
todui resume writing-sprint --revision 3
todui export md writing-sprint --revision 3 > sprint.md

9. TUI specification

9.1 Entry points

todui without a subcommand opens the primary TUI session overview.

todui resume remains the direct session-entry command.

No separate todui tui command is required in v1.

9.2 Screens

9.2.1 Session overview

Default screen when launched as `todui`.

Shows:
	•	all sessions ordered by last_opened_at descending
	•	display name and slug
	•	current revision
	•	open/done counts

Enter or Right opens the selected session head.

9.2.2 Session view

Shows live head or a historical revision of one session.

Layout:
	•	top bar
	•	main list pane
	•	Pomodoro pane when width allows
	•	footer

9.2.3 Revision history overlay

Open from session view.

Shows:
	•	revision number
	•	created timestamp
	•	mutation reason
	•	todo count
	•	done count

9.2.4 Todo editor modal

Simple modal for:
	•	add todo
	•	edit title
	•	edit notes

9.3 Session view layout

Top bar

Shows:
	•	app name
	•	session display name
	•	session slug
	•	revision indicator (HEAD or r17)
	•	active timer badge if present
	•	filter/search indicator if future support is added

Main pane

Todo list.

Each row contains:
	•	checkbox [ ] or [x]
	•	title
	•	small timestamp text
	•	optional badges:
	•	FOCUS
	•	BREAK
	•	HEAD not shown per row
	•	DONE

Row examples:

[ ] Draft design spec              created 09:20
[x] Review CLI commands            done 10:05
[ ] Write markdown exporter   FOCUS created 10:12

Details overlay
	•	title
	•	status
	•	notes
	•	created / updated / completed timestamps
	•	internal todo id

Pomodoro footer
	•	phase label: FOCUS, SHORT BREAK, LONG BREAK
	•	remaining time
	•	progress bar
	•	linked todo title or “No linked todo”
	•	controls live in help text: Start / Pause / Resume / Cancel

Footer

Shows key hints:
	•	j/k move
	•	space toggle
	•	n new
	•	d delete todo
	•	D delete session
	•	p Pomodoro
	•	H history
	•	o overview
	•	q quit

9.4 Narrow terminal behavior

Width >= 100 columns
	•	list takes the main pane
	•	Pomodoro stays visible below the list

Width 50–99 columns
	•	list takes full width
	•	Pomodoro stays visible below the list

Width < 50 columns
	•	single-pane list
	•	i or Right opens detail modal
	•	Pomodoro badge remains in top bar

9.5 Keyboard bindings

Global
	•	q quit screen / close overlay / quit app
	•	Esc close modal or overlay
	•	? help overlay
	•	Ctrl-c hard exit

List navigation
	•	Up / k move selection up
	•	Down / j move selection down
	•	Home / g g go top
	•	End / G go bottom
	•	PageUp / Ctrl-u page up
	•	PageDown / Ctrl-d page down

Session actions
	•	n create new todo in current session
	•	e edit selected todo
	•	d delete selected todo after confirmation
	•	D delete current session after confirmation
	•	Space / x toggle done
	•	i or Right open details
	•	H open revision history
	•	Left closes details first, otherwise returns to the session overview
	•	r when in revision mode, return to head
	•	p open or trigger Pomodoro action on the selected todo, or start unlinked if no todo is selected

Pomodoro actions
	•	p start focus if idle
	•	p pause if running
	•	p resume if paused
	•	b start short break
	•	B start long break
	•	c cancel current run

9.6 Mouse behavior

If mouse reporting is available:
	•	left click checkbox → toggle todo completion
	•	left click row → select todo
	•	mouse wheel → scroll focused pane
	•	click history entry → select revision
	•	double-click behavior: not used

If mouse reporting is unavailable, all behavior must still work via keyboard. Ghostty exposes this as terminal-controlled behavior, so the app must never depend on mouse delivery.  ￼

9.7 Revision mode behavior

When viewing a historical revision:
	•	top bar shows READ ONLY
	•	footer replaces mutating hints with r return to head
	•	clicking a checkbox shows toast: Historical revisions are read-only
	•	delete actions show toast: Historical revisions are read-only
	•	no Pomodoro UI is shown

Revision banner:

Viewing session writing-sprint @ r17 — 2026-03-24 11:48 — read-only

10. Color and style system

Use semantic style tokens, not hard-coded state colors in business logic.

Required tokens
	•	fg_default
	•	fg_muted
	•	fg_success
	•	fg_warning
	•	fg_error
	•	fg_accent
	•	bg_panel
	•	bg_selected
	•	bg_overlay
	•	border_default
	•	border_focus

State mapping
	•	open todo → fg_default
	•	done todo → fg_muted
	•	selected row → bg_selected
	•	active focused todo → fg_accent
	•	read-only revision banner → fg_warning
	•	error toast → fg_error

Accessibility rules
	•	never rely on color alone
	•	always keep status text or symbol visible
	•	do not require strikethrough support
	•	use [ ] and [x] as the primary completion markers

11. Timestamp rules

Store all timestamps as UTC Unix epoch integers in SQLite. SQLite has no dedicated datetime datatype, and its documented time-value forms include Unix timestamps, ISO-8601 text, and Julian day numbers.  ￼

Stored timestamps

sessions
	•	created_at
	•	updated_at
	•	last_opened_at

todos
	•	created_at
	•	updated_at
	•	completed_at nullable

revisions
	•	created_at

pomodoro runs
	•	started_at
	•	paused_at nullable
	•	ended_at nullable
	•	updated_at

Render rules
	•	list rows show compact local-time timestamps
	•	details overlay shows full local datetime
	•	CLI markdown export uses ISO-like human-readable text
	•	database never stores localtime strings

12. Persistence design

Use one SQLite database file. SQLite WAL mode allows readers and writers to proceed concurrently, but there is still only one writer at a time. WAL also requires all processes to be on the same machine and does not work over a network filesystem. Set a busy timeout on every connection so brief write contention between CLI and TUI does not immediately fail.  ￼

12.1 Connection policy

Each process opens one SQLite connection for its lifetime.

On open, execute:

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

Also set a busy timeout of 5000 ms in the Rust binding.

12.2 Migration policy

Use SQL migrations at startup.

Track schema version with:
	•	PRAGMA user_version
or
	•	a dedicated migrations table

Either is acceptable; pick one and keep it simple.

13. Database schema

Use STRICT tables for core entities.

13.1 Live tables

CREATE TABLE sessions (
  id                INTEGER PRIMARY KEY,
  slug              TEXT NOT NULL UNIQUE,
  name              TEXT NOT NULL,
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL,
  last_opened_at    INTEGER NOT NULL,
  current_revision  INTEGER NOT NULL
) STRICT;

CREATE TABLE todos (
  id                INTEGER PRIMARY KEY,
  session_id        INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  title             TEXT NOT NULL,
  notes             TEXT NOT NULL DEFAULT '',
  status            TEXT NOT NULL CHECK (status IN ('open', 'done')),
  position          INTEGER NOT NULL,
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL,
  completed_at      INTEGER,
  UNIQUE(session_id, position)
) STRICT;

CREATE INDEX idx_todos_session_position
  ON todos(session_id, position);

13.2 Revision tables

CREATE TABLE session_revisions (
  id                INTEGER PRIMARY KEY,
  session_id        INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  revision_number   INTEGER NOT NULL,
  created_at        INTEGER NOT NULL,
  reason            TEXT NOT NULL,
  todo_count        INTEGER NOT NULL,
  done_count        INTEGER NOT NULL,
  UNIQUE(session_id, revision_number)
) STRICT;

CREATE TABLE session_revision_todos (
  revision_id       INTEGER NOT NULL REFERENCES session_revisions(id) ON DELETE CASCADE,
  todo_id           INTEGER NOT NULL,
  title             TEXT NOT NULL,
  notes             TEXT NOT NULL,
  status            TEXT NOT NULL CHECK (status IN ('open', 'done')),
  position          INTEGER NOT NULL,
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL,
  completed_at      INTEGER,
  PRIMARY KEY (revision_id, todo_id)
) STRICT;

CREATE INDEX idx_revision_todos_position
  ON session_revision_todos(revision_id, position);

13.3 Pomodoro tables

CREATE TABLE pomodoro_runs (
  id                  INTEGER PRIMARY KEY,
  session_id          INTEGER REFERENCES sessions(id) ON DELETE CASCADE,
  todo_id             INTEGER REFERENCES todos(id) ON DELETE SET NULL,
  kind                TEXT NOT NULL CHECK (kind IN ('focus', 'short_break', 'long_break')),
  state               TEXT NOT NULL CHECK (state IN ('running', 'paused', 'completed', 'cancelled')),
  planned_seconds     INTEGER NOT NULL,
  started_at          INTEGER NOT NULL,
  paused_at           INTEGER,
  accumulated_pause   INTEGER NOT NULL DEFAULT 0,
  ended_at            INTEGER,
  updated_at          INTEGER NOT NULL
) STRICT;

/* Enforce only one active or paused timer globally */
CREATE UNIQUE INDEX idx_one_active_pomodoro
  ON pomodoro_runs(1)
  WHERE state IN ('running', 'paused');

13.4 App state table

CREATE TABLE app_state (
  key               TEXT PRIMARY KEY,
  value             TEXT NOT NULL
) STRICT;

Required keys:
	•	last_session_slug

14. Revision model

14.1 Revision semantics

A revision is created after every successful session mutation:
	•	create session
	•	add todo
	•	edit title
	•	edit notes
	•	delete todo
	•	toggle done / undone
	•	reorder todos

A revision is not created for:
	•	timer ticks
	•	timer pause/resume
	•	selection changes
	•	scrolling
	•	opening a revision
	•	updating last_opened_at

14.2 Initial revision

When a session is created:
	•	create session row
	•	create revision 1
	•	snapshot zero todos into that revision

14.3 Snapshot strategy

Use full-session snapshotting, not diff-based history, in v1.

On each session mutation:
	1.	start transaction
	2.	update live tables
	3.	calculate next revision number
	4.	insert into session_revisions
	5.	copy all current todos for that session into session_revision_todos
	6.	update sessions.current_revision
	7.	commit

This is intentionally simple and reliable.

14.4 Why not event sourcing in v1

Event sourcing and row-diff history add complexity without clear value for a lightweight app. SQLite can support more elaborate undo/redo patterns, but this spec chooses immutable per-session snapshots because they are easier to reason about and easier to export.  ￼

15. Session resume behavior

15.1 Most recent session

The “most recent session” is the session with the latest last_opened_at.

15.2 Updating last-opened

Update last_opened_at when:
	•	resuming a session head
	•	opening a session by name
	•	opening a historical revision of a session

Do not update it on background refreshes.

15.3 resume behavior

todui resume:
	•	finds the most recent session
	•	opens the live head of that session

todui resume <session>:
	•	opens that session head

todui resume <session> --revision N:
	•	opens revision N read-only

16. Todo behavior

16.1 Todo lifecycle

Only two statuses in v1:
	•	open
	•	done

16.2 Completion rules

When toggling to done:
	•	set status = 'done'
	•	set completed_at = now
	•	set updated_at = now

When toggling back to open:
	•	set status = 'open'
	•	set completed_at = NULL
	•	set updated_at = now

16.3 Ordering

Todos are ordered by position.

Insertion rule:
	•	append to bottom by default

Reordering:
	•	out of scope in CLI v1
	•	optional in TUI v1
	•	if implemented, create a revision

Deletion:
	•	deleting a todo compacts later positions
	•	deleting a todo creates a revision snapshot
	•	deleting a session is a hard delete of the session and all related rows
	•	deleting a session does not create a final revision

17. Pomodoro design

17.1 Placement

Pomodoro has no dedicated app-global screen.

The active footer appears:
	•	at the bottom of the overview when a run is active or paused
	•	at the bottom of a live session view when a run is active or paused
	•	nowhere in historical revision mode

17.2 Attach model

A Pomodoro run is session-agnostic and may optionally belong to a selected todo.

Behavior:
	•	if a todo is selected when starting focus, attach to that todo
	•	if no todo is selected, start an unlinked global run

17.3 State machine

States:
	•	idle
	•	running
	•	paused
	•	completed
	•	cancelled

Kinds:
	•	focus
	•	short_break
	•	long_break

Valid transitions:
	•	idle → running
	•	running → paused
	•	paused → running
	•	running → completed
	•	running → cancelled
	•	paused → cancelled

17.4 Default durations

Configurable defaults:
	•	focus: 25 minutes
	•	short break: 5 minutes
	•	long break: 15 minutes

17.5 Timer calculation

Do not decrement a counter in storage.

Persist:
	•	started_at
	•	paused_at
	•	accumulated_pause
	•	planned_seconds
	•	state

In memory:
	•	compute remaining time from timestamps
	•	redraw at a fixed cadence while active

Implementation note:
	•	use monotonic time for in-process redraw math
	•	use persisted UTC epoch for restart recovery

Rule:
	•	timer ticks must not write to the database every second
	•	DB writes occur only on start / pause / resume / cancel / complete

17.6 Pomodoro footer UI

Idle:

No Pomodoro box is shown by default.

Running:

Pomodoro
FOCUS · 12:41 remaining
██████████░░░░░░░░░░ 51%
Linked: Draft design spec

Paused:

Pomodoro
FOCUS · paused · 12:41 remaining
Linked: Draft design spec

Historical revision mode:

No Pomodoro UI is shown.

18. Markdown export spec

GFM defines task-list syntax and renders those markers as checkbox elements, but does not prescribe interactive behavior. That makes GFM the correct default format for exported todo lists.  ￼

18.1 Formats

Default: gfm
Use checkbox list items:
	•	[ ]
	•	[x]

Fallback: plain
Use plain bullets with status words:
	•	TODO:
	•	DONE:

18.2 Default command behavior

todui export md defaults to:
	•	most recent session
	•	live head revision
	•	gfm
	•	compact timestamps
	•	include notes only when non-empty
	•	include completed items

18.3 Output structure

Example:

# Session: writing-sprint

- slug: writing-sprint
- revision: 17
- exported-at: 2026-03-24 12:03
- session-updated-at: 2026-03-24 11:48

## Todos

- [ ] Draft design spec
  - created: 2026-03-24 09:20
  - updated: 2026-03-24 11:48
  - notes: cover CLI, TUI, DB, pomodoro

- [x] Review CLI commands
  - created: 2026-03-24 09:30
  - completed: 2026-03-24 10:05

18.4 Export options

--timestamps full
Include full per-item timestamps.

--timestamps compact
Include only one relevant line per item:
	•	open item → created
	•	done item → completed

--timestamps none
Omit timestamps entirely.

--open-only
Exclude completed todos.

--include-notes
Include notes blocks when non-empty.

18.5 Historical export

If --revision N is provided:
	•	export that snapshot only
	•	include revision number in header
	•	do not include active timer controls
	•	optionally include Pomodoro summary up to that revision timestamp

19. Application architecture

19.1 High-level architecture

Use this flow:

Terminal Event
  -> Input Mapper
  -> Action
  -> Reducer / Command Handler
  -> Domain Service
  -> SQLite Repository
  -> Updated App State
  -> Render

19.2 Major modules

Suggested layout:

src/
  main.rs
  cli.rs
  config.rs
  app.rs
  action.rs
  reducer.rs
  error.rs

  domain/
    session.rs
    todo.rs
    revision.rs
    pomodoro.rs

  db/
    mod.rs
    connection.rs
    migrations.rs
    sessions.rs
    todos.rs
    revisions.rs
    pomodoro.rs

  tui/
    mod.rs
    screen.rs
    layout.rs
    theme.rs
    widgets/
      todo_list.rs
      details.rs
      pomodoro.rs
      history.rs
      editor.rs
      footer.rs

  export/
    markdown.rs

19.3 Action enum

Representative actions:

enum Action {
    OpenRecentSession,
    OpenSession { slug: String },
    OpenRevision { slug: String, revision: u32 },
    CloseOverlay,
    Quit,

    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    GoTop,
    GoBottom,
    SelectTodo { id: i64 },

    NewTodo,
    EditTodo { id: i64 },
    SaveTodo { id: i64, title: String, notes: String },
    AddTodo { session_slug: String, title: String, notes: String },
    ToggleTodo { id: i64 },

    OpenHistory,
    SelectRevision { revision: u32 },

    StartPomodoro { kind: PomodoroKind, todo_id: Option<i64> },
    PausePomodoro,
    ResumePomodoro,
    CancelPomodoro,

    Tick,
    MouseClick { x: u16, y: u16 },
    MouseScrollUp,
    MouseScrollDown,
}

19.4 App state struct

Representative fields:

struct AppState {
    current_session: Option<SessionView>,
    current_revision_mode: RevisionMode, // Head | Historical(u32)
    selected_todo_id: Option<i64>,
    focused_pane: FocusedPane,
    overlay: Option<Overlay>,
    toast: Option<Toast>,
    theme: Theme,
    now: TimeAnchor,
}

TimeAnchor should hold both:
	•	wall time anchor
	•	monotonic anchor

19.5 Event loop

Ratatui requires the app to drive render and event handling itself. Implement a centralized event loop with action dispatch. Use backend event polling from Crossterm and ignore key-release events.  ￼

Loop behavior:
	•	poll input every 250 ms
	•	if active timer exists, dispatch Tick on timeout
	•	otherwise redraw only on events, toast expiry, or overlay changes

Pseudo-flow:

loop {
    render(&app_state)?;

    if poll(timeout)? {
        let event = read()?;
        let action = map_event_to_action(event, &app_state);
        reducer.dispatch(action)?;
    } else if app_state.has_active_timer() {
        reducer.dispatch(Action::Tick)?;
    }
}

20. Startup and shutdown behavior

20.1 Startup
	1.	load config
	2.	open DB
	3.	apply PRAGMAs
	4.	run migrations
	5.	resolve command
	6.	if TUI mode:
	•	initialize terminal
	•	enable raw mode
	•	enter alternate screen
	•	enable mouse capture
	7.	run app loop

20.2 Shutdown

Always:
	•	disable raw mode
	•	leave alternate screen
	•	disable mouse capture
	•	show cursor
	•	close DB cleanly

This cleanup matters because Ratatui/Crossterm apps that fail to restore terminal state leave the terminal in a broken-looking state.  ￼

21. Repository methods

Minimum repository API:

create_session(name, slug) -> Session
list_sessions() -> Vec<SessionSummary>
get_session_by_slug(slug) -> Session
mark_session_opened(session_id, now)

add_todo(session_id, title, notes, now) -> Todo
update_todo(todo_id, title, notes, now)
toggle_todo(todo_id, now)

get_live_todos(session_id) -> Vec<Todo>
get_revision_todos(session_id, revision_number) -> Vec<RevisionTodo>
list_revisions(session_id) -> Vec<RevisionSummary>
create_revision_snapshot(session_id, reason, now) -> RevisionSummary

start_pomodoro(todo_id, kind, planned_seconds, now)
pause_pomodoro(run_id, now)
resume_pomodoro(run_id, now)
cancel_pomodoro(run_id, now)
complete_pomodoro(run_id, now)
get_active_pomodoro() -> Option<PomodoroRun>

22. Transaction rules

Add todo

One transaction shall:
	•	insert todo
	•	update session timestamps
	•	create revision snapshot

Toggle todo

One transaction shall:
	•	update todo fields
	•	update session timestamps
	•	create revision snapshot

Edit todo

One transaction shall:
	•	update title/notes
	•	update session timestamps
	•	create revision snapshot

Pomodoro write actions

One transaction per action:
	•	start
	•	pause
	•	resume
	•	cancel
	•	complete

Pomodoro actions do not create session revisions in v1.

23. Error handling

User-facing errors

Show concise messages for:
	•	session not found
	•	revision not found
	•	no recent session
	•	database busy
	•	historical revision is read-only
	•	cannot start timer because another timer is active

DB busy policy

If SQLite returns busy after timeout:
	•	show toast in TUI
	•	return non-zero with clear stderr message in CLI

Corrupt or incompatible DB

Fail fast with:
	•	path
	•	migration version info
	•	suggested backup action

24. Configuration file

config.toml

Example:

[theme]
mode = "dark" # dark | light
accent = "cyan"

[pomodoro]
focus_minutes = 25
short_break_minutes = 5
long_break_minutes = 15
notify_on_complete = true

[keys]
up = ["up", "k"]
down = ["down", "j"]
toggle_done = ["space", "x"]
history = ["H"]
pomodoro = ["p"]

v1 only needs:
	•	theme mode
	•	Pomodoro durations
	•	optional Pomodoro completion bell toggle
	•	optional key overrides

25. Testing plan

25.1 Unit tests
	•	slug generation
	•	timestamp formatting
	•	markdown export formatting
	•	timer remaining-time math
	•	action-to-reducer transitions

25.2 Repository tests

Use temp SQLite DB and verify:
	•	session creation
	•	revision counts
	•	revision snapshots
	•	toggle done timestamps
	•	Pomodoro state transitions
	•	active timer uniqueness

25.3 CLI tests

Golden tests for:
	•	session new
	•	add
	•	history
	•	export md
	•	resume --revision

25.4 TUI tests

At minimum:
	•	key mapping tests
	•	mouse hitbox tests
	•	read-only revision behavior
	•	render snapshot tests for:
	•	head session view
	•	historical revision view
	•	active focus timer
	•	narrow layout

26. Implementation milestones

Milestone 1
	•	SQLite schema
	•	session create/list/resume
	•	add todo
	•	toggle done
	•	live TUI session view
	•	markdown export

Milestone 2
	•	revision snapshots
	•	history overlay
	•	open read-only revision

Milestone 3
	•	Pomodoro footer
	•	start/pause/resume/cancel
	•	unlinked and todo-linked runs

Milestone 4
	•	config file
	•	theme tokens
	•	tests and polish

27. Final implementation decisions to keep fixed

These should not be re-debated during implementation:
	•	session is the primary object
	•	resume defaults to the most recent session head
	•	old revisions are viewable through --revision and history UI
	•	revisions are immutable and read-only
	•	markdown export defaults to GFM
	•	Pomodoro uses a shared active footer in overview and live session views
	•	keyboard-first, mouse-complete
	•	modeless navigation with Vim aliases
	•	SQLite + WAL + foreign keys on + STRICT tables
	•	full snapshot revisioning in v1

If you want, I can turn this into a tighter RFC-style document next, with sections like Background, Requirements, Architecture, Schema, and Acceptance Criteria.
