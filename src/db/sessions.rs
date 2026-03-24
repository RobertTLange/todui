use rusqlite::{OptionalExtension, Transaction, params};

use crate::db::Database;
use crate::domain::revision::RevisionSummary;
use crate::domain::session::{Session, SessionSummary, slugify, validate_slug};
use crate::error::{AppError, Result};

impl Database {
    pub fn create_session(&mut self, name: &str, slug: Option<&str>, now: i64) -> Result<Session> {
        let resolved_slug = slug.map(str::to_owned).unwrap_or_else(|| slugify(name));
        validate_slug(&resolved_slug)?;

        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO sessions (slug, name, created_at, updated_at, last_opened_at, current_revision)
             VALUES (?1, ?2, ?3, ?3, ?3, 0)",
            params![resolved_slug, name, now],
        )?;
        let session_id = transaction.last_insert_rowid();
        let revision = create_revision_snapshot(&transaction, session_id, "session created", now)?;
        transaction.execute(
            "UPDATE sessions SET current_revision = ?1 WHERE id = ?2",
            params![revision.revision_number, session_id],
        )?;
        set_last_session_slug(&transaction, &resolved_slug)?;
        transaction.commit()?;

        self.get_session_by_slug(&resolved_slug)
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionSummary>> {
        let mut statement = self.connection.prepare(
            "SELECT slug, name, last_opened_at, current_revision
             FROM sessions
             ORDER BY last_opened_at DESC, id DESC",
        )?;
        let rows = statement.query_map([], map_session_summary)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_session_by_slug(&self, slug: &str) -> Result<Session> {
        self.connection
            .query_row(
                "SELECT id, slug, name, created_at, updated_at, last_opened_at, current_revision
                 FROM sessions
                 WHERE slug = ?1",
                [slug],
                map_session,
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => AppError::SessionNotFound(slug.to_string()),
                other => AppError::Database(other),
            })
    }

    pub fn get_most_recent_session(&self) -> Result<Session> {
        self.connection
            .query_row(
                "SELECT id, slug, name, created_at, updated_at, last_opened_at, current_revision
                 FROM sessions
                 ORDER BY last_opened_at DESC, id DESC
                 LIMIT 1",
                [],
                map_session,
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => AppError::NoRecentSession,
                other => AppError::Database(other),
            })
    }

    pub fn resolve_session_slug(&self, slug: Option<&str>) -> Result<String> {
        match slug {
            Some(value) => Ok(value.to_string()),
            None => Ok(self.get_most_recent_session()?.slug),
        }
    }

    pub fn mark_session_opened(&mut self, slug: &str, now: i64) -> Result<Session> {
        let transaction = self.connection.transaction()?;
        let session_id: i64 = transaction
            .query_row("SELECT id FROM sessions WHERE slug = ?1", [slug], |row| {
                row.get::<_, i64>(0)
            })
            .optional()?
            .ok_or_else(|| AppError::SessionNotFound(slug.to_string()))?;

        transaction.execute(
            "UPDATE sessions SET last_opened_at = ?1 WHERE id = ?2",
            params![now, session_id],
        )?;
        set_last_session_slug(&transaction, slug)?;
        transaction.commit()?;

        self.get_session_by_slug(slug)
    }

    pub fn current_revision_summary(&self, session_id: i64) -> Result<RevisionSummary> {
        self.connection
            .query_row(
                "SELECT revision_number, created_at, reason, todo_count, done_count
             FROM session_revisions
             WHERE session_id = ?1
             ORDER BY revision_number DESC
             LIMIT 1",
                [session_id],
                map_revision_summary,
            )
            .map_err(AppError::from)
    }
}

pub(crate) fn create_revision_snapshot(
    transaction: &Transaction<'_>,
    session_id: i64,
    reason: &str,
    now: i64,
) -> Result<RevisionSummary> {
    let revision_number: u32 = transaction.query_row(
        "SELECT COALESCE(MAX(revision_number), 0) + 1
         FROM session_revisions
         WHERE session_id = ?1",
        [session_id],
        |row| row.get(0),
    )?;

    let (todo_count, done_count): (i64, i64) = transaction.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END), 0)
         FROM todos
         WHERE session_id = ?1",
        [session_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    transaction.execute(
        "INSERT INTO session_revisions (
            session_id, revision_number, created_at, reason, todo_count, done_count
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            session_id,
            revision_number,
            now,
            reason,
            todo_count,
            done_count
        ],
    )?;
    let revision_id = transaction.last_insert_rowid();

    transaction.execute(
        "INSERT INTO session_revision_todos (
            revision_id, todo_id, title, notes, status, position, created_at, updated_at, completed_at
         )
         SELECT ?1, id, title, notes, status, position, created_at, updated_at, completed_at
         FROM todos
         WHERE session_id = ?2
         ORDER BY position",
        params![revision_id, session_id],
    )?;

    Ok(RevisionSummary {
        revision_number,
        created_at: now,
        reason: reason.to_string(),
        todo_count,
        done_count,
    })
}

fn set_last_session_slug(transaction: &Transaction<'_>, slug: &str) -> Result<()> {
    transaction.execute(
        "INSERT INTO app_state (key, value)
         VALUES ('last_session_slug', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [slug],
    )?;

    Ok(())
}

fn map_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        slug: row.get(1)?,
        name: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        last_opened_at: row.get(5)?,
        current_revision: row.get(6)?,
    })
}

fn map_session_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        slug: row.get(0)?,
        name: row.get(1)?,
        last_opened_at: row.get(2)?,
        current_revision: row.get(3)?,
    })
}

pub(crate) fn map_revision_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<RevisionSummary> {
    Ok(RevisionSummary {
        revision_number: row.get(0)?,
        created_at: row.get(1)?,
        reason: row.get(2)?,
        todo_count: row.get(3)?,
        done_count: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use crate::db::Database;

    #[test]
    fn creates_and_lists_sessions() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let created = database
            .create_session("Writing Sprint", None, 1_711_275_600)
            .expect("session");

        assert_eq!(created.slug, "writing-sprint");
        assert_eq!(created.current_revision, 1);

        let listed = database.list_sessions().expect("sessions");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].slug, "writing-sprint");
    }
}
