use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use crate::db::migrations::{LATEST_USER_VERSION, apply};
use crate::error::Result;

#[derive(Debug)]
pub struct Database {
    pub(crate) connection: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(path)?;
        configure_connection(&connection)?;

        Ok(Self { connection })
    }

    #[cfg(test)]
    pub fn open_temp() -> Result<(tempfile::TempDir, Self)> {
        let directory = tempfile::tempdir()?;
        let database = Self::open(&directory.path().join("todui.db"))?;
        Ok((directory, database))
    }
}

fn configure_connection(connection: &Connection) -> Result<()> {
    connection.busy_timeout(Duration::from_millis(5_000))?;
    connection.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;

    let current_version: i32 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current_version < LATEST_USER_VERSION {
        apply(connection, current_version)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, OptionalExtension};

    use super::Database;
    use crate::db::migrations::LATEST_USER_VERSION;

    #[test]
    fn opens_database_and_applies_schema() {
        let (_directory, database) = Database::open_temp().expect("database");
        let user_version: i32 = database
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("version");

        assert_eq!(user_version, LATEST_USER_VERSION);
    }

    #[test]
    fn upgrades_v1_database_to_latest_schema() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("todui.db");
        let connection = Connection::open(&path).expect("open raw connection");
        connection
            .execute_batch(
                "
                PRAGMA user_version = 1;
                CREATE TABLE sessions (
                  id                INTEGER PRIMARY KEY,
                  slug              TEXT NOT NULL UNIQUE,
                  name              TEXT NOT NULL,
                  created_at        INTEGER NOT NULL,
                  updated_at        INTEGER NOT NULL,
                  last_opened_at    INTEGER NOT NULL,
                  current_revision  INTEGER NOT NULL
                ) STRICT;
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
                CREATE TABLE pomodoro_runs (
                  id                  INTEGER PRIMARY KEY,
                  session_id          INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
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
                CREATE TABLE app_state (
                  key               TEXT PRIMARY KEY,
                  value             TEXT NOT NULL
                ) STRICT;
                ",
            )
            .expect("seed v1 schema");
        drop(connection);

        let reopened = Database::open(&path).expect("reopen upgraded database");
        let user_version: i32 = reopened
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("version");
        let session_tag_exists: String = reopened
            .connection
            .query_row(
                "SELECT name FROM pragma_table_info('sessions') WHERE name = 'tag'",
                [],
                |row| row.get(0),
            )
            .expect("session tag column");
        let session_name_exists: Option<String> = reopened
            .connection
            .query_row(
                "SELECT name FROM pragma_table_info('sessions') WHERE name = 'name'",
                [],
                |row| row.get(0),
            )
            .optional()
            .expect("session name column lookup");
        let revision_tag_exists: String = reopened
            .connection
            .query_row(
                "SELECT name FROM pragma_table_info('session_revisions') WHERE name = 'session_tag'",
                [],
                |row| row.get(0),
            )
            .expect("revision tag column");
        let session_repo_exists: String = reopened
            .connection
            .query_row(
                "SELECT name FROM pragma_table_info('sessions') WHERE name = 'repo'",
                [],
                |row| row.get(0),
            )
            .expect("session repo column");
        let todo_repo_exists: String = reopened
            .connection
            .query_row(
                "SELECT name FROM pragma_table_info('todos') WHERE name = 'repo'",
                [],
                |row| row.get(0),
            )
            .expect("todo repo column");
        let revision_repo_exists: String = reopened
            .connection
            .query_row(
                "SELECT name FROM pragma_table_info('session_revisions') WHERE name = 'session_repo'",
                [],
                |row| row.get(0),
            )
            .expect("revision repo column");
        let revision_todo_repo_exists: String = reopened
            .connection
            .query_row(
                "SELECT name FROM pragma_table_info('session_revision_todos') WHERE name = 'repo'",
                [],
                |row| row.get(0),
            )
            .expect("revision todo repo column");
        let todo_created_by_exists: String = reopened
            .connection
            .query_row(
                "SELECT name FROM pragma_table_info('todos') WHERE name = 'created_by_kind'",
                [],
                |row| row.get(0),
            )
            .expect("todo created by column");
        let todo_completed_by_exists: String = reopened
            .connection
            .query_row(
                "SELECT name FROM pragma_table_info('todos') WHERE name = 'completed_by_kind'",
                [],
                |row| row.get(0),
            )
            .expect("todo completed by column");
        let revision_todo_created_by_exists: String = reopened
            .connection
            .query_row(
                "SELECT name FROM pragma_table_info('session_revision_todos') WHERE name = 'created_by_kind'",
                [],
                |row| row.get(0),
            )
            .expect("revision todo created by column");
        let revision_todo_completed_by_exists: String = reopened
            .connection
            .query_row(
                "SELECT name FROM pragma_table_info('session_revision_todos') WHERE name = 'completed_by_kind'",
                [],
                |row| row.get(0),
            )
            .expect("revision todo completed by column");
        reopened
            .connection
            .execute(
                "INSERT INTO sessions (slug, created_at, updated_at, last_opened_at, current_revision)
                 VALUES ('migration-check', 1, 1, 1, 1)",
                [],
            )
            .expect("insert migrated session");
        reopened
            .connection
            .execute(
                "INSERT INTO todos (session_id, title, notes, status, position, created_at, updated_at, completed_at)
                 VALUES (1, 'Legacy open', '', 'open', 1, 1, 1, NULL)",
                [],
            )
            .expect("insert migrated todo");
        let todo_created_by_value: String = reopened
            .connection
            .query_row(
                "SELECT created_by_kind FROM todos WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .expect("todo created by value");
        let todo_completed_by_value: Option<String> = reopened
            .connection
            .query_row(
                "SELECT completed_by_kind FROM todos WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .expect("todo completed by value");
        let pomodoro_session_not_null: i64 = reopened
            .connection
            .query_row(
                "SELECT \"notnull\" FROM pragma_table_info('pomodoro_runs') WHERE name = 'session_id'",
                [],
                |row| row.get(0),
            )
            .expect("pomodoro session not null flag");

        assert_eq!(user_version, LATEST_USER_VERSION);
        assert_eq!(session_name_exists, None);
        assert_eq!(session_tag_exists, "tag");
        assert_eq!(revision_tag_exists, "session_tag");
        assert_eq!(session_repo_exists, "repo");
        assert_eq!(todo_repo_exists, "repo");
        assert_eq!(revision_repo_exists, "session_repo");
        assert_eq!(revision_todo_repo_exists, "repo");
        assert_eq!(todo_created_by_exists, "created_by_kind");
        assert_eq!(todo_completed_by_exists, "completed_by_kind");
        assert_eq!(revision_todo_created_by_exists, "created_by_kind");
        assert_eq!(revision_todo_completed_by_exists, "completed_by_kind");
        assert_eq!(todo_created_by_value, "human");
        assert_eq!(todo_completed_by_value, None);
        assert_eq!(pomodoro_session_not_null, 0);
    }
}
