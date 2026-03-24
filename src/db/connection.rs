use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use crate::db::migrations::{LATEST_USER_VERSION, MIGRATION_SQL};
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
        connection.execute_batch(MIGRATION_SQL)?;
        connection.pragma_update(None, "user_version", LATEST_USER_VERSION)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Database;

    #[test]
    fn opens_database_and_applies_schema() {
        let (_directory, database) = Database::open_temp().expect("database");
        let user_version: i32 = database
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("version");

        assert_eq!(user_version, 1);
    }
}
