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
  <a href="https://roberttlange.com/blog/04-todui"><img alt="Blog" src="https://img.shields.io/badge/Blog-read%20post-FF6B35" /></a>
</p>

When work spans shell scripts, scratch markdown, and half-finished terminal notes, preserving a clean session history gets noisy fast. `todui` gives you one local place to capture todo sessions, browse immutable revisions, keep notes close to the work, and stay in the terminal from quick CLI automation to full-screen planning.

<p align="center">
  <img src="docs/release-three-views.png" alt="todui overview, live session, and historical session views shown side by side with different themes" width="1100" />
</p>

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

Running `todui` without a subcommand opens the overview screen. If you mostly work from scripts or agent loops, you can stay in CLI mode with `session`, `add`, `done`, `repo`, and `export`.

## 60-Second Usage

```bash
# Create a new session for the work you want to track.
todui session new "Writing Sprint" --tag work
# Add a concrete drafting task with an attached note.
todui add "Draft design spec" --session writing-sprint --note "cover CLI and TUI"
# Attach a todo to a GitHub repo so it can be queried later.
todui add "Review keybindings" --session writing-sprint --repo @exampleorg/todui-keymove
# Record a human-authored item instead of the default agent provenance.
todui add "Interview notes" --session writing-sprint --human
# Jump straight into the live session in the TUI.
todui resume writing-sprint
# Open the immutable revision history for that session.
todui session history writing-sprint
# Export the current session, including notes, as markdown.
todui export md writing-sprint --include-notes
```

CLI todo mutations default to agent provenance. Pass `--human` on `add` or `done` when the action should be recorded as human-authored.

## What You Get

- Session-based todo lists with one canonical session name per workspace, so project work stays grouped instead of getting scattered across ad hoc scratch files.
- A full-screen TUI and a scriptable CLI backed by the same local SQLite database, which makes it practical to mix interactive planning with shell automation.
- Immutable revision history for sessions, including read-only resume and export flows for older states when you need to audit what changed.
- Human versus agent provenance on todo creation and completion, which is useful when multiple tools or collaborators are touching the same session.
- Overview notes, per-todo notes, repo metadata, and markdown export so the session can double as a lightweight working log.
- A built-in Pomodoro timer surfaced in both overview and live session screens rather than living in a separate app.

## Config

`todui` looks for a config file and database path independently. The config file location decides which TOML file gets loaded; the database path is then resolved from CLI or env overrides first, then from the loaded config file.

| Setting | Default | Override order | Notes |
| --- | --- | --- | --- |
| Config file path | `~/.todui/config.toml` | `todui --config /absolute/path/to/config.toml` -> `TODO_TUI_CONFIG` -> default path | If the file does not exist, `todui` falls back to built-in defaults. |
| Database path | `~/.local/share/todui/todui.db` | `TODO_TUI_DB` -> `[database].path` in the selected config file -> default path | Use an absolute path in config if you want the DB somewhere else. |

The example config exposes four areas:

| Section | Purpose | Example keys |
| --- | --- | --- |
| `[database]` | Point `todui` at a specific SQLite file. | `path` |
| `[theme]` | Pick the overall color mode and accent. | `mode`, `accent` |
| `[pomodoro]` | Tune timer lengths and completion notifications. | `focus_minutes`, `short_break_minutes`, `long_break_minutes`, `notify_on_complete` |
| `[keys]` | Override default key bindings for core navigation and actions. | `up`, `down`, `toggle_done`, `history`, `pomodoro` |

From a source checkout or unpacked npm tarball, seed a config file from the example:

```bash
mkdir -p ~/.todui
cp config.example.toml ~/.todui/config.toml
```

Minimal example:

```toml
[theme]
mode = "dark"
accent = "cyan"
```

If you only want a quick visual change, setting `[theme]` is enough. If you are integrating `todui` into a larger local workflow, the most common next step is pinning `[database].path` so scripts and the TUI both point at an explicit database location.

## Common TUI Keys

- `j` / `k`, arrows: move selection
- `n`: create a session or todo
- `e`: edit the selected session or todo
- `f`: cycle provenance filter (`all`, `human`, `agent`)
- `space` / `x`: toggle completion
- `H`: open session history
- `r`: return from a revision to the live head
- `m`: edit overview notes
- `q` / `Esc`: close overlay or quit

## Skills

This repo ships two workflow skills for long-running agent work. They are meant for Codex or Claude-style coding agents that need a repeatable planning/execution loop and a shared `todui` session as their ledger.

### Install the bundled skills

```bash
npx skills add RobertTLange/todui --skill '*' -a claude-code -a codex -y
```

That command installs both bundled skills from this repository for supported agents.

### `project-plan`

See [skills/project-plan/SKILL.md](skills/project-plan/SKILL.md).

Use this when you have a spec and want an agent to turn it into an execution pack under `docs/`. The skill reads a source spec, creates or reuses a deterministic `todui` session for the repo, writes `docs/prompt.md`, `docs/plan.md`, `docs/implement.md`, and `docs/documentation.md`, and keeps fixed planning todos in sync while it does so.

In practice, `project-plan` is the right starting point when the work is still ambiguous and you want the agent to freeze scope, milestones, assumptions, and validation steps before touching code.

### `project-build`

See [skills/project-build/SKILL.md](skills/project-build/SKILL.md).

Use this after a plan pack already exists under `docs/`. The skill treats the planning pack as the source of truth, creates or reuses one repo session in `todui`, mirrors milestones into todos, executes them one at a time, records blockers, and leaves the current repo state resumable for another agent.

In practice, `project-build` is the execution companion to `project-plan`: first generate the pack, then ship against it without losing milestone state in a long chat thread.

## Verification

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
TODOUI_SKIP_DOWNLOAD=1 npm ci
npm run build
npm test
```
