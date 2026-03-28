use rusqlite::Connection;

use crate::error::Result;

pub const LATEST_USER_VERSION: i32 = 5;

const MIGRATION_V1_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
  id                INTEGER PRIMARY KEY,
  slug              TEXT NOT NULL UNIQUE,
  name              TEXT NOT NULL,
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL,
  last_opened_at    INTEGER NOT NULL,
  current_revision  INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS todos (
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

CREATE INDEX IF NOT EXISTS idx_todos_session_position
  ON todos(session_id, position);

CREATE TABLE IF NOT EXISTS session_revisions (
  id                INTEGER PRIMARY KEY,
  session_id        INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  revision_number   INTEGER NOT NULL,
  created_at        INTEGER NOT NULL,
  reason            TEXT NOT NULL,
  todo_count        INTEGER NOT NULL,
  done_count        INTEGER NOT NULL,
  UNIQUE(session_id, revision_number)
) STRICT;

CREATE TABLE IF NOT EXISTS session_revision_todos (
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

CREATE INDEX IF NOT EXISTS idx_revision_todos_position
  ON session_revision_todos(revision_id, position);

CREATE TABLE IF NOT EXISTS pomodoro_runs (
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

CREATE UNIQUE INDEX IF NOT EXISTS idx_one_active_pomodoro
  ON pomodoro_runs(1)
  WHERE state IN ('running', 'paused');

CREATE TABLE IF NOT EXISTS app_state (
  key               TEXT PRIMARY KEY,
  value             TEXT NOT NULL
) STRICT;
"#;

const MIGRATION_V2_SQL: &str = r#"
ALTER TABLE sessions ADD COLUMN tag TEXT;
ALTER TABLE session_revisions ADD COLUMN session_tag TEXT;
"#;

const MIGRATION_V3_SQL: &str = r#"
DROP INDEX IF EXISTS idx_pomodoro_session_started;

ALTER TABLE pomodoro_runs RENAME TO pomodoro_runs_old;

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

INSERT INTO pomodoro_runs (
  id,
  session_id,
  todo_id,
  kind,
  state,
  planned_seconds,
  started_at,
  paused_at,
  accumulated_pause,
  ended_at,
  updated_at
)
SELECT
  id,
  NULL,
  todo_id,
  kind,
  state,
  planned_seconds,
  started_at,
  paused_at,
  accumulated_pause,
  ended_at,
  updated_at
FROM pomodoro_runs_old;

DROP TABLE pomodoro_runs_old;

CREATE UNIQUE INDEX idx_one_active_pomodoro
  ON pomodoro_runs(1)
  WHERE state IN ('running', 'paused');
"#;

const MIGRATION_V4_SQL: &str = r#"
ALTER TABLE sessions ADD COLUMN repo TEXT;
ALTER TABLE todos ADD COLUMN repo TEXT;
ALTER TABLE session_revisions ADD COLUMN session_repo TEXT;
ALTER TABLE session_revision_todos ADD COLUMN repo TEXT;
"#;

const MIGRATION_V5_SQL: &str = r#"
ALTER TABLE sessions DROP COLUMN name;
"#;

pub fn apply(connection: &Connection, current_version: i32) -> Result<()> {
    if current_version < 1 {
        connection.execute_batch(MIGRATION_V1_SQL)?;
        connection.pragma_update(None, "user_version", 1)?;
    }
    if current_version < 2 {
        connection.execute_batch(MIGRATION_V2_SQL)?;
        connection.pragma_update(None, "user_version", 2)?;
    }
    if current_version < 3 {
        connection.execute_batch(MIGRATION_V3_SQL)?;
        connection.pragma_update(None, "user_version", 3)?;
    }
    if current_version < 4 {
        connection.execute_batch(MIGRATION_V4_SQL)?;
        connection.pragma_update(None, "user_version", 4)?;
    }
    if current_version < 5 {
        connection.execute_batch(MIGRATION_V5_SQL)?;
        connection.pragma_update(None, "user_version", 5)?;
    }

    Ok(())
}
