## Screenshot Fixtures

Three checkout-local fixture pairs for release screenshots.

Run from repo root:

```bash
target/debug/todui --config fixtures/screenshots/overview-dark-cyan.toml
target/debug/todui --config fixtures/screenshots/session-light-red.toml resume release-polish
target/debug/todui --config fixtures/screenshots/history-beige-blue.toml resume launch-week --revision 5
```

Notes:

- Configs point at SQLite files in this folder.
- View 1 opens the overview.
- View 2 opens a live session.
- View 3 opens a read-only historical revision.
- Around `76x24` to `90x28` works well for side-by-side captures.
