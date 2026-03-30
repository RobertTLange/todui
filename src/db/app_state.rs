use crate::db::Database;
use crate::error::Result;
use rusqlite::OptionalExtension;

const OVERVIEW_NOTES_KEY: &str = "overview_general_notes";

impl Database {
    pub fn get_overview_notes(&self) -> Result<String> {
        Ok(self
            .connection
            .query_row(
                "SELECT value FROM app_state WHERE key = ?1",
                [OVERVIEW_NOTES_KEY],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or_default())
    }

    pub fn save_overview_notes(&mut self, notes: &str) -> Result<()> {
        if notes.is_empty() {
            self.connection
                .execute("DELETE FROM app_state WHERE key = ?1", [OVERVIEW_NOTES_KEY])?;
        } else {
            self.connection.execute(
                "INSERT INTO app_state (key, value)
                 VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (OVERVIEW_NOTES_KEY, notes),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Database;

    #[test]
    fn overview_notes_default_to_empty_string() {
        let (_directory, database) = Database::open_temp().expect("database");

        assert_eq!(database.get_overview_notes().expect("notes"), "");
    }

    #[test]
    fn overview_notes_round_trip_through_app_state() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let notes = "# Inbox\n\n- **Ship** overview notes";

        database.save_overview_notes(notes).expect("save");

        assert_eq!(database.get_overview_notes().expect("notes"), notes);
    }
}
