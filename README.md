<p align="center">
  <img src="docs/logo.png" alt="todui logo" width="240" height="240" />
</p>

<h1 align="center">todui</h1>

<p align="center">
  Local-first terminal todo sessions with a full-screen TUI, immutable revisions, and SQLite persistence.
</p>

<p align="center">
  <img alt="Node.js 18+" src="https://img.shields.io/badge/Node.js-18%2B-339933?logo=node.js&logoColor=white" />
  <img alt="Rust stable" src="https://img.shields.io/badge/Rust-stable-000000?logo=rust&logoColor=white" />
  <img alt="SQLite" src="https://img.shields.io/badge/SQLite-local--first-003B57?logo=sqlite&logoColor=white" />
  <img alt="macOS and Linux" src="https://img.shields.io/badge/macOS%20%26%20Linux-x64%20%2F%20arm64-4CAF50" />
</p>

When work spans shell scripts, scratch markdown, and half-finished terminal notes, preserving a clean session history gets noisy fast. `todui` gives you one local place to capture todo sessions, browse immutable revisions, keep notes close to the work, and stay in the terminal from quick CLI automation to full-screen planning.

## Quick Start

### Fastest: run with `npx`

```bash
npx -y @roberttlange/todui --help
```

### Install globally

```bash
npm install -g @roberttlange/todui
todui
```

The npm package downloads a prebuilt binary for macOS/Linux on `x64` and `arm64`; a local Rust toolchain is not required for the npm install path.

### From source

```bash
cargo install --path .
todui --help
```

## 60-Second Usage

```bash
todui session new "Writing Sprint" --tag work
todui add "Draft design spec" --session writing-sprint --note "cover CLI and TUI"
todui add "Review keybindings" --session writing-sprint --repo @sakanaai/todui-keymove
todui resume writing-sprint
todui session history writing-sprint
todui export md writing-sprint --include-notes
```

## What You Get

- Session-based todo lists with one canonical session name per workspace.
- Full-screen TUI plus scriptable CLI for the same local SQLite data.
- Immutable revision history with read-only historical resume/export flows.
- App-wide overview notes, todo notes, repo-aware metadata, and markdown export.
- Global Pomodoro timer surfaced in overview and live session views.

## Config

Default paths:

- config: `~/.config/todui/config.toml`
- database: `~/.local/share/todui/todui.db`

Overrides:

- `TODO_TUI_CONFIG`
- `TODO_TUI_DB`

Seed a config file from the example:

```bash
mkdir -p ~/.config/todui
cp config.example.toml ~/.config/todui/config.toml
```

## Common TUI Keys

- `j` / `k`, arrows: move selection
- `n`: create a session or todo
- `e`: edit the selected session or todo
- `space` / `x`: toggle completion
- `H`: open session history
- `r`: return from a revision to the live head
- `m`: edit overview notes
- `q` / `Esc`: close overlay or quit

## Docs

- [Configuration example](config.example.toml)
- [Development and release workflow](docs/development.md)
- [Agent workflow notes](docs/agent-features.md)

## Verification
```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
TODOUI_SKIP_DOWNLOAD=1 npm ci
npm run build
npm test
```
