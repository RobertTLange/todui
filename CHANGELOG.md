# Changelog

All notable changes to this project will be documented in this file.

## [TBD] - TBD

### Added

- The overview TUI's New Session modal now autocompletes existing session names, including completed sessions hidden from the overview, and can resume those sessions instead of creating duplicates.

## [0.1.2] - 2026-04-18

### Fixed

- The published npm launcher now treats the `node_modules/.bin/todui` symlink as a direct invocation, so `npx -y @roberttlange/todui --version` and related commands actually execute the bundled binary.

## [0.1.1] - 2026-04-18

### Fixed

- npm launcher now falls back to local Cargo build outputs when run from a source checkout, so `npx -y @roberttlange/todui --help` works inside the repository.
- Linux `aarch64-unknown-linux-gnu` release builds now pin the correct cross-linker in CI, with a regression test covering the workflow wiring.

## [0.1.0] - 2026-04-18

### Added

- First public release of `todui`, a local-first Rust todo manager with both a scriptable CLI and a full-screen terminal UI.
- Session-based todo tracking with tags, notes, repo metadata, recent-session resume flows, and per-action human or agent provenance.
- Immutable session revisions with history browsing, read-only historical resume behavior, and markdown export for external sharing or audit.
- Built-in Pomodoro support, configurable themes, configurable key bindings, and TOML-based local configuration.
- npm packaging for `@roberttlange/todui` with prebuilt macOS/Linux binaries for `x64` and `arm64`, plus source installation via Cargo.

### Release Summary

- Codebase shape: Rust application code under `src/`, SQLite persistence and migrations under `src/db/`, terminal UI screens and widgets under `src/tui/`, markdown export under `src/export/`, and CLI coverage in `tests/`.
- User-facing surface: `todui` opens an overview TUI by default and ships CLI flows for `session`, `add`, `delete`, `edit`, `done`, `undone`, `resume`, `repo`, and `export`.
- Storage model: local SQLite with bundled `rusqlite`, immutable revision snapshots, recent-session tracking, repo-aware session metadata, and todo notes stored with the session state.
- Terminal UX: multi-screen Ratatui/Crossterm interface with overview, live session, history, markdown, editor, details, and Pomodoro widgets.
- Distribution: version-synced Rust and npm packages, Node.js installer wrapper, bundled example config, release assets, and npm smoke tests around installation behavior.
