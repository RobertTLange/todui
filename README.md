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

An example config lives in [`config.example.toml`](config.example.toml).

Create the config directory and copy it into place:

```bash
mkdir -p ~/.config/todui
cp config.example.toml ~/.config/todui/config.toml
```

## TUI Keys

Movement:

- `j` / `k`, arrows: move selection up and down
- `PageUp` / `PageDown`: jump by a page
- `g` / `Home`: jump to the first row
- `G` / `End`: jump to the last row
- `Enter`, `Right`, `l`: open the selected session from overview
- `Left`, `o`: return from a session to overview
- `i`, `Right`: open todo details inside a session

Session actions:

- `space` / `x`: toggle done
- `n`: create a session or todo, depending on the current screen
- `e`: edit the selected todo
- `t`: edit the selected session tag/repo from overview
- `u`: open the selected session/todo GitHub repo in the browser
- `d`: delete selected todo
- `D`: delete selected session
- `H`: open revision history
- `r`: return from revision to head
- `p`: focus start / pause / resume
- `b`: short break
- `B`: long break
- `c`: cancel timer
- `h`: help overlay
- `q` / `Esc`: quit or close overlay

Pomodoro completion emits a terminal bell by default while the TUI is open.
