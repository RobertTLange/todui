# todui

Local-first terminal todo app.

Features:

- session-based todo lists
- full-screen TUI via `resume`
- immutable session revision history
- embedded Pomodoro timer
- SQLite persistence
- markdown export

## Setup

Prereqs:

- Rust stable
- Cargo

Build:

```bash
cargo install --path .
```

If `todui` is not found after install, add Cargo bin to `PATH`:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Run tests + checks:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Coverage:

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked
cargo llvm-cov --workspace --all-features --lcov --output-path coverage/lcov.info --fail-under-lines 80
```

## Paths

Default paths:

- config: `~/.config/todui/config.toml`
- database: `~/.local/share/todui/todui.db`

Overrides:

- `TODO_TUI_CONFIG`
- `TODO_TUI_DB`

Example:

```bash
export TODO_TUI_DB=/tmp/todui.db
export TODO_TUI_CONFIG=/tmp/todui-config.toml
```

## Run

Create a session + add work:

```bash
todui session new "Writing Sprint"
todui add "Draft design spec" --session writing-sprint --note "cover CLI, TUI, DB, pomodoro"
```

Open the TUI:

```bash
todui resume writing-sprint
```

Open a historical revision read-only:

```bash
todui resume writing-sprint --revision 1
```

Inspect history:

```bash
todui session history writing-sprint
```

Export markdown:

```bash
todui export md writing-sprint --format gfm
todui export md writing-sprint --revision 1 --timestamps full --include-notes
```

## Config

Minimal example:

```toml
[theme]
mode = "dark"
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
```

## TUI Keys

- `j` / `k`, arrows: move
- `space` / `x`: toggle done
- `H`: history
- `r`: return from revision to head
- `p`: focus start / pause / resume
- `b`: short break
- `B`: long break
- `c`: cancel timer
- `q` / `Esc`: quit or close overlay

Pomodoro completion emits a terminal bell by default while the TUI is open.
