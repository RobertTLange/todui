# AGENTS.md

## Workflow

- Treat the docs in `docs/` as the ground truth when implementing the app.
- Follow the plan and milestones in `docs/plan.md`.
- Commit changes in logical chunks at suitable checkpoints.
- Prefer small, reviewable commits over one large final commit.
- Do not ask the user questions during implementation.
- Make reasonable assumptions and continue execution.
- Return to the user only once all scoped work and milestones are completed.

## Clean Code Guidelines

### Primary Directive

- Optimize for readability, maintainability, and verification, not cleverness.
- Follow the Boy Scout Rule: leave the code cleaner than you found it.
- Keep code aligned with the architecture and fixed decisions in `docs/spec.md`, `docs/plan.md`, and `docs/implement.md`.

### Naming

- Use names that reveal intent: a reader should understand why something exists, what it does, and how it is used.
- Prefer precise Rust names: nouns for types, verbs for functions, explicit enum variant names, no vague `Manager`, `Helper`, `Utils`, or `Data` buckets.
- Avoid disinformation: do not name something `session` if it is a revision snapshot, or `timer` if it is persisted run state.
- Use searchable, pronounceable names; avoid single-letter variables except for very short local indices.
- If a name needs a comment to explain it, rename it.

### Functions And Modules

- Keep functions small and single-purpose.
- Prefer extracting pure helpers when a function starts mixing parsing, mutation, formatting, persistence, and rendering.
- Keep branching shallow where possible; replace repeated condition chains with enums, helper functions, or small dispatch points.
- Keep modules focused:
  - `domain/` for business rules and state transitions
  - `db/` for SQLite access, migrations, and transactions
  - `tui/` for rendering, layout, hit testing, and event mapping
  - `export/` for markdown formatting
  - app/reducer layer for orchestration and action handling
- Keep business logic out of TUI widgets and CLI argument parsing.
- Split files before they become large or mixed-purpose.

### Data And State

- Prefer typed structs and enums over stringly typed state.
- Model invalid states out of existence where practical.
- Keep historical revisions strictly read-only.
- Keep Pomodoro tick math in memory; do not write timer state every second.
- Store timestamps as UTC Unix epoch integers only.
- Centralize config, path, keybinding, and style-token definitions instead of scattering literals.
- Avoid magic numbers and ad hoc strings; name them once and reuse them.

### Errors And Boundaries

- Surface errors clearly with `Result` and project-level error types.
- Avoid `unwrap` and `expect` outside tests unless failure is truly unrecoverable at process startup.
- Keep CLI behavior deterministic and machine-friendly: data to stdout, diagnostics to stderr, non-zero exit on failure.
- Fail clearly on DB, migration, and path errors; do not silently degrade core persistence guarantees.
- Keep terminal setup and cleanup explicit so the terminal is always restored on exit.

### Comments And Style

- Prefer self-explanatory code over explanatory comments.
- Add comments only when intent, invariants, or a non-obvious tradeoff would otherwise be hard to recover from the code alone.
- Remove stale comments during edits.
- Keep formatting and structure consistent with the surrounding Rust codebase.

### Design Checks

- Make the smallest design that cleanly satisfies the spec and milestone.
- Remove duplication once the behavior is understood and covered by tests.
- Prefer explicit data flow over hidden coupling.
- Place logic where a reader would expect to find it.
- Before finishing a change, check for common smells:
  - duplicated logic
  - mixed responsibilities
  - magic numbers or strings
  - misplaced persistence logic in UI code
  - read/write paths accidentally enabled in historical mode
  - comments compensating for poor naming

## Testing

- Maximize test coverage for all changes.
- Treat missing tests for new behavior and bug fixes as incomplete work unless clearly not applicable.
- Write unit tests for focused logic such as:
  - slug generation
  - timestamp formatting
  - reducer transitions
  - markdown export formatting
  - Pomodoro remaining-time math
  - key and mouse event mapping
- Write integration tests for:
  - SQLite repositories and migrations
  - revision snapshot creation
  - read-only revision behavior
  - CLI flows and stdout/stderr behavior
  - active Pomodoro uniqueness rules
- Use temporary databases in tests; keep tests deterministic, isolated, and repeatable.
- Keep tests readable too: one behavior per test when practical, clear setup, clear assertions, no hidden shared state.
- Run the milestone validation commands from `docs/plan.md` before considering the work complete.
