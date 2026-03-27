use rusqlite::{OptionalExtension, Transaction, params};

use crate::db::Database;
use crate::domain::github::normalize_optional_repo;
use crate::domain::revision::RevisionSummary;
use crate::domain::session::{
    Session, SessionHeadToken, SessionOverview, SessionSummary, normalize_tag, slugify,
    validate_slug,
};
use crate::error::{AppError, Result};

impl Database {
    pub fn create_session(
        &mut self,
        name: &str,
        slug: Option<&str>,
        tag: Option<&str>,
        repo: Option<&str>,
        now: i64,
    ) -> Result<Session> {
        let resolved_slug = slug.map(str::to_owned).unwrap_or_else(|| slugify(name));
        let resolved_tag = normalize_tag(tag)?;
        let resolved_repo = normalize_optional_repo(repo)?;
        validate_slug(&resolved_slug)?;

        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO sessions (slug, name, created_at, updated_at, last_opened_at, current_revision, tag, repo)
             VALUES (?1, ?2, ?3, ?3, ?3, 0, ?4, ?5)",
            params![resolved_slug, name, now, resolved_tag, resolved_repo],
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
            "SELECT slug, name, tag, repo, last_opened_at, current_revision
             FROM sessions
             ORDER BY last_opened_at DESC, id DESC",
        )?;
        let rows = statement.query_map([], map_session_summary)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn list_session_overview(&self) -> Result<Vec<SessionOverview>> {
        let mut statement = self.connection.prepare(
            "SELECT
                sessions.slug,
                sessions.name,
                sessions.tag,
                sessions.repo,
                sessions.last_opened_at,
                sessions.current_revision,
                COALESCE(session_revisions.todo_count, 0),
                COALESCE(session_revisions.done_count, 0)
             FROM sessions
             LEFT JOIN session_revisions
               ON session_revisions.session_id = sessions.id
              AND session_revisions.revision_number = sessions.current_revision
             ORDER BY sessions.last_opened_at DESC, sessions.id DESC",
        )?;
        let rows = statement.query_map([], map_session_overview)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_session_by_slug(&self, slug: &str) -> Result<Session> {
        self.connection
            .query_row(
                "SELECT id, slug, name, tag, repo, created_at, updated_at, last_opened_at, current_revision
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
                "SELECT id, slug, name, tag, repo, created_at, updated_at, last_opened_at, current_revision
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

    pub fn delete_session(&mut self, slug: &str) -> Result<Session> {
        let transaction = self.connection.transaction()?;
        let session = transaction
            .query_row(
                "SELECT id, slug, name, tag, repo, created_at, updated_at, last_opened_at, current_revision
                 FROM sessions
                 WHERE slug = ?1",
                [slug],
                map_session,
            )
            .optional()?
            .ok_or_else(|| AppError::SessionNotFound(slug.to_string()))?;

        transaction.execute("DELETE FROM sessions WHERE id = ?1", [session.id])?;
        sync_last_session_slug(&transaction)?;
        transaction.commit()?;

        Ok(session)
    }

    pub fn update_session_metadata(
        &mut self,
        slug: &str,
        tag: Option<&str>,
        repo: Option<&str>,
        now: i64,
    ) -> Result<Session> {
        let resolved_tag = normalize_tag(tag)?;
        let resolved_repo = normalize_optional_repo(repo)?;
        let transaction = self.connection.transaction()?;
        let (session_id, current_tag, current_repo): (i64, Option<String>, Option<String>) =
            transaction
                .query_row(
                    "SELECT id, tag, repo FROM sessions WHERE slug = ?1",
                    [slug],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?
                .ok_or_else(|| AppError::SessionNotFound(slug.to_string()))?;

        let Some(reason) = session_metadata_revision_reason(
            current_tag.as_deref(),
            resolved_tag.as_deref(),
            current_repo.as_deref(),
            resolved_repo.as_deref(),
        ) else {
            transaction.commit()?;
            return self.get_session_by_slug(slug);
        };

        transaction.execute(
            "UPDATE sessions SET tag = ?1, repo = ?2, updated_at = ?3 WHERE id = ?4",
            params![resolved_tag, resolved_repo, now, session_id],
        )?;
        let revision = create_revision_snapshot(&transaction, session_id, reason, now)?;
        transaction.execute(
            "UPDATE sessions SET current_revision = ?1 WHERE id = ?2",
            params![revision.revision_number, session_id],
        )?;
        transaction.commit()?;

        self.get_session_by_slug(slug)
    }

    pub fn update_session_tag(
        &mut self,
        slug: &str,
        tag: Option<&str>,
        now: i64,
    ) -> Result<Session> {
        let current = self.get_session_by_slug(slug)?;
        self.update_session_metadata(slug, tag, current.repo.as_deref(), now)
    }

    pub fn update_session_repo(
        &mut self,
        slug: &str,
        repo: Option<&str>,
        now: i64,
    ) -> Result<Session> {
        let current = self.get_session_by_slug(slug)?;
        self.update_session_metadata(slug, current.tag.as_deref(), repo, now)
    }

    pub fn session_head_token(&self, slug: &str) -> Result<SessionHeadToken> {
        self.connection
            .query_row(
                "SELECT current_revision, updated_at
                 FROM sessions
                 WHERE slug = ?1",
                [slug],
                |row| {
                    Ok(SessionHeadToken {
                        current_revision: row.get(0)?,
                        updated_at: row.get(1)?,
                    })
                },
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => AppError::SessionNotFound(slug.to_string()),
                other => AppError::Database(other),
            })
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
    let (session_tag, session_repo): (Option<String>, Option<String>) = transaction.query_row(
        "SELECT tag, repo FROM sessions WHERE id = ?1",
        [session_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    transaction.execute(
        "INSERT INTO session_revisions (
            session_id, revision_number, created_at, reason, session_tag, session_repo, todo_count, done_count
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            session_id,
            revision_number,
            now,
            reason,
            session_tag,
            session_repo,
            todo_count,
            done_count
        ],
    )?;
    let revision_id = transaction.last_insert_rowid();

    transaction.execute(
        "INSERT INTO session_revision_todos (
            revision_id, todo_id, title, notes, repo, status, position, created_at, updated_at, completed_at
         )
         SELECT ?1, id, title, notes, repo, status, position, created_at, updated_at, completed_at
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

fn sync_last_session_slug(transaction: &Transaction<'_>) -> Result<()> {
    let most_recent_slug: Option<String> = transaction
        .query_row(
            "SELECT slug
             FROM sessions
             ORDER BY last_opened_at DESC, id DESC
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(slug) = most_recent_slug {
        set_last_session_slug(transaction, &slug)?;
    } else {
        transaction.execute("DELETE FROM app_state WHERE key = 'last_session_slug'", [])?;
    }

    Ok(())
}

fn map_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        slug: row.get(1)?,
        name: row.get(2)?,
        tag: row.get(3)?,
        repo: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        last_opened_at: row.get(7)?,
        current_revision: row.get(8)?,
    })
}

fn map_session_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        slug: row.get(0)?,
        name: row.get(1)?,
        tag: row.get(2)?,
        repo: row.get(3)?,
        last_opened_at: row.get(4)?,
        current_revision: row.get(5)?,
    })
}

fn map_session_overview(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionOverview> {
    Ok(SessionOverview {
        slug: row.get(0)?,
        name: row.get(1)?,
        tag: row.get(2)?,
        repo: row.get(3)?,
        last_opened_at: row.get(4)?,
        current_revision: row.get(5)?,
        todo_count: row.get(6)?,
        done_count: row.get(7)?,
    })
}

fn session_metadata_revision_reason(
    current_tag: Option<&str>,
    next_tag: Option<&str>,
    current_repo: Option<&str>,
    next_repo: Option<&str>,
) -> Option<&'static str> {
    let tag_changed = current_tag != next_tag;
    let repo_changed = current_repo != next_repo;

    match (tag_changed, repo_changed) {
        (false, false) => None,
        (true, false) => Some("session tag updated"),
        (false, true) => Some("session repo updated"),
        (true, true) => Some("session metadata updated"),
    }
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
    use crate::domain::session::SessionHeadToken;

    #[test]
    fn creates_and_lists_sessions() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let created = database
            .create_session(
                "Writing Sprint",
                None,
                Some("Work Projects"),
                None,
                1_711_275_600,
            )
            .expect("session");

        assert_eq!(created.slug, "writing-sprint");
        assert_eq!(created.tag.as_deref(), Some("work-projects"));
        assert_eq!(created.current_revision, 1);

        let listed = database.list_sessions().expect("sessions");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].slug, "writing-sprint");
        assert_eq!(listed[0].tag.as_deref(), Some("work-projects"));
    }

    #[test]
    fn resolves_recent_session_and_marks_opened() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        database
            .create_session("Writing Sprint", None, None, None, 1_711_275_600)
            .expect("session");
        database
            .create_session("Reading Sprint", None, None, None, 1_711_275_700)
            .expect("session");

        assert_eq!(
            database.resolve_session_slug(None).expect("recent"),
            "reading-sprint"
        );

        let marked = database
            .mark_session_opened("writing-sprint", 1_711_275_800)
            .expect("marked");
        assert_eq!(marked.slug, "writing-sprint");
        assert_eq!(
            database.resolve_session_slug(None).expect("recent"),
            "writing-sprint"
        );
    }

    #[test]
    fn lists_overview_rows_in_recent_order_with_counts() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let writing = database
            .create_session("Writing Sprint", None, Some("work"), None, 1_711_275_600)
            .expect("session");
        database
            .add_todo(&writing.slug, "Draft spec", "", None, 1_711_275_650)
            .expect("todo");
        database
            .add_todo(&writing.slug, "Review keybindings", "", None, 1_711_275_660)
            .expect("todo");
        database
            .set_todo_status(
                1,
                Some(&writing.slug),
                crate::domain::todo::TodoStatus::Done,
                1_711_275_670,
            )
            .expect("done");

        let reading = database
            .create_session("Reading Sprint", None, None, None, 1_711_275_700)
            .expect("session");
        database
            .mark_session_opened(&writing.slug, 1_711_275_800)
            .expect("opened");

        let overview = database.list_session_overview().expect("overview");
        assert_eq!(overview.len(), 2);
        assert_eq!(overview[0].slug, writing.slug);
        assert_eq!(overview[0].tag.as_deref(), Some("work"));
        assert_eq!(overview[0].todo_count, 2);
        assert_eq!(overview[0].done_count, 1);
        assert_eq!(overview[1].slug, reading.slug);
        assert_eq!(overview[1].tag, None);
    }

    #[test]
    fn deleting_session_cascades_and_updates_recent_pointer() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let writing = database
            .create_session("Writing Sprint", None, None, None, 1_711_275_600)
            .expect("session");
        database
            .add_todo(&writing.slug, "Draft spec", "", None, 1_711_275_650)
            .expect("todo");

        let reading = database
            .create_session("Reading Sprint", None, Some("private"), None, 1_711_275_700)
            .expect("session");
        let todo = database
            .add_todo(&reading.slug, "Review paper", "", None, 1_711_275_750)
            .expect("todo");
        let run = database
            .start_pomodoro(
                crate::domain::pomodoro::PomodoroKind::Focus,
                1_500,
                1_711_275_760,
            )
            .expect("run");

        let deleted = database.delete_session(&reading.slug).expect("delete");
        assert_eq!(deleted.slug, reading.slug);
        assert!(database.get_session_by_slug(&reading.slug).is_err());
        assert!(database.get_todo(todo.id).is_err());
        let run = database.get_pomodoro_run(run.id).expect("run");
        assert_eq!(run.todo_id, None);
        assert_eq!(run.session_id, None);
        assert_eq!(
            database.resolve_session_slug(None).expect("recent"),
            writing.slug
        );
    }

    #[test]
    fn deleting_last_session_clears_recent_pointer() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session("Writing Sprint", None, None, None, 1_711_275_600)
            .expect("session");

        database.delete_session(&session.slug).expect("delete");

        assert!(matches!(
            database.resolve_session_slug(None),
            Err(crate::error::AppError::NoRecentSession)
        ));
    }

    #[test]
    fn session_head_token_changes_for_same_session_mutations_only() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let writing = database
            .create_session("Writing Sprint", None, None, None, 1_711_275_600)
            .expect("writing session");
        let initial = database
            .session_head_token(&writing.slug)
            .expect("initial head token");

        database
            .add_todo(&writing.slug, "Draft spec", "", None, 1_711_275_700)
            .expect("todo");
        let after_todo = database
            .session_head_token(&writing.slug)
            .expect("updated head token");
        assert_ne!(after_todo, initial);
        assert_eq!(
            after_todo,
            SessionHeadToken {
                current_revision: 2,
                updated_at: 1_711_275_700,
            }
        );

        database
            .create_session("Reading Sprint", None, None, None, 1_711_275_800)
            .expect("reading session");
        let after_other_session = database
            .session_head_token(&writing.slug)
            .expect("unchanged token");
        assert_eq!(after_other_session, after_todo);
    }

    #[test]
    fn updates_and_clears_session_tag_with_new_revision() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session("Writing Sprint", None, None, None, 1_711_275_600)
            .expect("session");

        let updated = database
            .update_session_tag(&session.slug, Some("Private Projects"), 1_711_275_700)
            .expect("set tag");
        assert_eq!(updated.tag.as_deref(), Some("private-projects"));
        assert_eq!(updated.current_revision, 2);
        assert_eq!(updated.updated_at, 1_711_275_700);

        let cleared = database
            .update_session_tag(&session.slug, None, 1_711_275_800)
            .expect("clear tag");
        assert_eq!(cleared.tag, None);
        assert_eq!(cleared.current_revision, 3);
        assert_eq!(cleared.updated_at, 1_711_275_800);
    }

    #[test]
    fn updates_and_clears_session_repo_with_new_revision() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session("Writing Sprint", None, None, None, 1_711_275_600)
            .expect("session");

        let updated = database
            .update_session_repo(
                &session.slug,
                Some("https://github.com/SakanaAI/todui-keymove"),
                1_711_275_700,
            )
            .expect("set repo");
        assert_eq!(updated.repo.as_deref(), Some("sakanaai/todui-keymove"));
        assert_eq!(updated.current_revision, 2);

        let cleared = database
            .update_session_repo(&session.slug, None, 1_711_275_800)
            .expect("clear repo");
        assert_eq!(cleared.repo, None);
        assert_eq!(cleared.current_revision, 3);
    }
}
