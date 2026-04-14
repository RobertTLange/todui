use rusqlite::{OptionalExtension, Transaction, params};

use crate::db::Database;
use crate::domain::github::normalize_optional_repo;
use crate::domain::revision::RevisionSummary;
use crate::domain::session::{
    Session, SessionHeadToken, SessionOverview, SessionSummary, normalize_session_name,
    normalize_tag, validate_session_name,
};
use crate::error::{AppError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct OverviewAggregateSnapshot {
    pub total_sessions: i64,
    pub tagged_sessions: i64,
    pub total_todos: i64,
    pub done_todos: i64,
    pub human_open_todos: i64,
    pub agent_open_todos: i64,
    pub human_completed_todos: i64,
    pub agent_completed_todos: i64,
    pub average_revision: u32,
}

impl Database {
    pub fn has_any_sessions(&self) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions LIMIT 1)",
            [],
            |row| row.get::<_, i64>(0),
        )? != 0)
    }

    pub fn create_session(
        &mut self,
        name: &str,
        tag: Option<&str>,
        repo: Option<&str>,
        now: i64,
    ) -> Result<Session> {
        let resolved_name = normalize_session_name(name);
        let resolved_tag = normalize_tag(tag)?;
        let resolved_repo = normalize_optional_repo(repo)?;
        validate_session_name(&resolved_name)?;

        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO sessions (slug, created_at, updated_at, last_opened_at, current_revision, tag, repo)
             VALUES (?1, ?2, ?2, ?2, 0, ?3, ?4)",
            params![resolved_name, now, resolved_tag, resolved_repo],
        )?;
        let session_id = transaction.last_insert_rowid();
        let revision = create_revision_snapshot(&transaction, session_id, "session created", now)?;
        transaction.execute(
            "UPDATE sessions SET current_revision = ?1 WHERE id = ?2",
            params![revision.revision_number, session_id],
        )?;
        set_last_session_name(&transaction, &resolved_name)?;
        transaction.commit()?;

        self.get_session_by_name(&resolved_name)
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionSummary>> {
        let mut statement = self.connection.prepare(
            "SELECT slug, tag, repo, last_opened_at, current_revision
             FROM sessions
             ORDER BY last_opened_at DESC, id DESC",
        )?;
        let rows = statement.query_map([], map_session_summary)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn list_session_overview(&self) -> Result<Vec<SessionOverview>> {
        let mut statement = self.connection.prepare(
            "SELECT
                sessions.id,
                sessions.slug,
                sessions.tag,
                sessions.repo,
                sessions.updated_at,
                sessions.last_opened_at,
                sessions.current_revision,
                COALESCE(session_revisions.todo_count, 0),
                COALESCE(session_revisions.done_count, 0)
             FROM sessions
             LEFT JOIN session_revisions
               ON session_revisions.session_id = sessions.id
              AND session_revisions.revision_number = sessions.current_revision
             WHERE
               COALESCE(session_revisions.todo_count, 0) = 0
               OR COALESCE(session_revisions.todo_count, 0) > COALESCE(session_revisions.done_count, 0)
             ORDER BY
               sessions.tag IS NULL ASC,
               sessions.tag ASC,
               sessions.updated_at DESC,
               sessions.id DESC",
        )?;
        let rows = statement.query_map([], map_session_overview)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub(crate) fn overview_aggregate_as_of(&self, as_of: i64) -> Result<OverviewAggregateSnapshot> {
        let (
            total_sessions,
            tagged_sessions,
            total_todos,
            done_todos,
            human_open_todos,
            agent_open_todos,
            human_completed_todos,
            agent_completed_todos,
            total_revisions,
        ): (i64, i64, i64, i64, i64, i64, i64, i64, i64) = self.connection.query_row(
            "WITH latest_revisions AS (
                SELECT session_id, MAX(revision_number) AS revision_number
                FROM session_revisions
                WHERE created_at <= ?1
                GROUP BY session_id
             )
             SELECT
                COUNT(*) AS total_sessions,
                COALESCE(SUM(CASE WHEN revisions.session_tag IS NOT NULL THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(revisions.todo_count), 0),
                COALESCE(SUM(revisions.done_count), 0),
                COALESCE(SUM((
                    SELECT COUNT(*)
                    FROM session_revision_todos revision_todos
                    WHERE revision_todos.revision_id = revisions.id
                      AND revision_todos.status = 'open'
                      AND revision_todos.created_by_kind = 'human'
                )), 0),
                COALESCE(SUM((
                    SELECT COUNT(*)
                    FROM session_revision_todos revision_todos
                    WHERE revision_todos.revision_id = revisions.id
                      AND revision_todos.status = 'open'
                      AND revision_todos.created_by_kind = 'agent'
                )), 0),
                COALESCE(SUM((
                    SELECT COUNT(*)
                    FROM session_revision_todos revision_todos
                    WHERE revision_todos.revision_id = revisions.id
                      AND revision_todos.status = 'done'
                      AND revision_todos.completed_by_kind = 'human'
                )), 0),
                COALESCE(SUM((
                    SELECT COUNT(*)
                    FROM session_revision_todos revision_todos
                    WHERE revision_todos.revision_id = revisions.id
                      AND revision_todos.status = 'done'
                      AND revision_todos.completed_by_kind = 'agent'
                )), 0),
                COALESCE(SUM(revisions.revision_number), 0)
             FROM sessions
             JOIN latest_revisions
               ON latest_revisions.session_id = sessions.id
             JOIN session_revisions AS revisions
               ON revisions.session_id = latest_revisions.session_id
              AND revisions.revision_number = latest_revisions.revision_number
             WHERE sessions.created_at <= ?1
               AND (
                    revisions.todo_count = 0
                    OR revisions.todo_count > revisions.done_count
               )",
            [as_of],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )?;

        let average_revision = if total_sessions == 0 {
            0
        } else {
            ((total_revisions + (total_sessions / 2)) / total_sessions) as u32
        };

        Ok(OverviewAggregateSnapshot {
            total_sessions,
            tagged_sessions,
            total_todos,
            done_todos,
            human_open_todos,
            agent_open_todos,
            human_completed_todos,
            agent_completed_todos,
            average_revision,
        })
    }

    pub fn get_session_by_name(&self, name: &str) -> Result<Session> {
        self.connection
            .query_row(
                "SELECT id, slug, tag, repo, created_at, updated_at, last_opened_at, current_revision
                 FROM sessions
                 WHERE slug = ?1",
                [name],
                map_session,
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => AppError::SessionNotFound(name.to_string()),
                other => AppError::Database(other),
            })
    }

    pub fn get_most_recent_session(&self) -> Result<Session> {
        self.connection
            .query_row(
                "SELECT id, slug, tag, repo, created_at, updated_at, last_opened_at, current_revision
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

    pub fn resolve_session_name(&self, name: Option<&str>) -> Result<String> {
        match name {
            Some(value) => Ok(value.to_string()),
            None => Ok(self.get_most_recent_session()?.name),
        }
    }

    pub fn mark_session_opened(&mut self, name: &str, now: i64) -> Result<Session> {
        let transaction = self.connection.transaction()?;
        let session_id: i64 = transaction
            .query_row("SELECT id FROM sessions WHERE slug = ?1", [name], |row| {
                row.get::<_, i64>(0)
            })
            .optional()?
            .ok_or_else(|| AppError::SessionNotFound(name.to_string()))?;

        transaction.execute(
            "UPDATE sessions SET last_opened_at = ?1 WHERE id = ?2",
            params![now, session_id],
        )?;
        set_last_session_name(&transaction, name)?;
        transaction.commit()?;

        self.get_session_by_name(name)
    }

    pub fn delete_session(&mut self, name: &str) -> Result<Session> {
        let transaction = self.connection.transaction()?;
        let session = transaction
            .query_row(
                "SELECT id, slug, tag, repo, created_at, updated_at, last_opened_at, current_revision
                 FROM sessions
                 WHERE slug = ?1",
                [name],
                map_session,
            )
            .optional()?
            .ok_or_else(|| AppError::SessionNotFound(name.to_string()))?;

        transaction.execute("DELETE FROM sessions WHERE id = ?1", [session.id])?;
        sync_last_session_name(&transaction)?;
        transaction.commit()?;

        Ok(session)
    }

    pub fn edit_session(
        &mut self,
        current_name: &str,
        name: &str,
        tag: Option<&str>,
        repo: Option<&str>,
        now: i64,
    ) -> Result<Session> {
        let next_name = normalize_session_name(name);
        let resolved_tag = normalize_tag(tag)?;
        let resolved_repo = normalize_optional_repo(repo)?;
        validate_session_name(&next_name)?;

        let transaction = self.connection.transaction()?;
        let (session_id, stored_name, current_tag, current_repo): (
            i64,
            String,
            Option<String>,
            Option<String>,
        ) = transaction
            .query_row(
                "SELECT id, slug, tag, repo FROM sessions WHERE slug = ?1",
                [current_name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?
            .ok_or_else(|| AppError::SessionNotFound(current_name.to_string()))?;
        let repo_changed = current_repo != resolved_repo;

        if stored_name == next_name && current_tag == resolved_tag && current_repo == resolved_repo
        {
            transaction.commit()?;
            return self.get_session_by_name(current_name);
        }

        transaction.execute(
            "UPDATE sessions
             SET slug = ?1, tag = ?2, repo = ?3, updated_at = ?4
             WHERE id = ?5",
            params![next_name, resolved_tag, resolved_repo, now, session_id],
        )?;
        if repo_changed {
            transaction.execute(
                "UPDATE todos SET repo = ?1, updated_at = ?2 WHERE session_id = ?3",
                params![resolved_repo, now, session_id],
            )?;
        }
        update_last_session_name_if_needed(&transaction, current_name, &next_name)?;
        let revision = create_revision_snapshot(&transaction, session_id, "session edited", now)?;
        transaction.execute(
            "UPDATE sessions SET current_revision = ?1 WHERE id = ?2",
            params![revision.revision_number, session_id],
        )?;
        transaction.commit()?;

        self.get_session_by_name(&next_name)
    }

    pub fn update_session_metadata(
        &mut self,
        name: &str,
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
                    [name],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?
                .ok_or_else(|| AppError::SessionNotFound(name.to_string()))?;

        let Some(reason) = session_metadata_revision_reason(
            current_tag.as_deref(),
            resolved_tag.as_deref(),
            current_repo.as_deref(),
            resolved_repo.as_deref(),
        ) else {
            transaction.commit()?;
            return self.get_session_by_name(name);
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

        self.get_session_by_name(name)
    }

    pub fn update_session_tag(
        &mut self,
        name: &str,
        tag: Option<&str>,
        now: i64,
    ) -> Result<Session> {
        let current = self.get_session_by_name(name)?;
        self.update_session_metadata(name, tag, current.repo.as_deref(), now)
    }

    pub fn update_session_repo(
        &mut self,
        name: &str,
        repo: Option<&str>,
        now: i64,
    ) -> Result<Session> {
        let current = self.get_session_by_name(name)?;
        self.update_session_metadata(name, current.tag.as_deref(), repo, now)
    }

    pub fn session_head_token(&self, name: &str) -> Result<SessionHeadToken> {
        self.connection
            .query_row(
                "SELECT current_revision, updated_at
                 FROM sessions
                 WHERE slug = ?1",
                [name],
                |row| {
                    Ok(SessionHeadToken {
                        current_revision: row.get(0)?,
                        updated_at: row.get(1)?,
                    })
                },
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => AppError::SessionNotFound(name.to_string()),
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
            revision_id, todo_id, title, notes, repo, created_by_kind, completed_by_kind, status, position, created_at, updated_at, completed_at
         )
         SELECT ?1, id, title, notes, repo, created_by_kind, completed_by_kind, status, position, created_at, updated_at, completed_at
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

fn set_last_session_name(transaction: &Transaction<'_>, name: &str) -> Result<()> {
    transaction.execute(
        "INSERT INTO app_state (key, value)
         VALUES ('last_session_name', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [name],
    )?;

    Ok(())
}

fn sync_last_session_name(transaction: &Transaction<'_>) -> Result<()> {
    let most_recent_name: Option<String> = transaction
        .query_row(
            "SELECT slug
             FROM sessions
             ORDER BY last_opened_at DESC, id DESC
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(name) = most_recent_name {
        set_last_session_name(transaction, &name)?;
    } else {
        transaction.execute("DELETE FROM app_state WHERE key = 'last_session_name'", [])?;
    }

    Ok(())
}

fn update_last_session_name_if_needed(
    transaction: &Transaction<'_>,
    current_name: &str,
    next_name: &str,
) -> Result<()> {
    let last_session_name: Option<String> = transaction
        .query_row(
            "SELECT value FROM app_state WHERE key = 'last_session_name'",
            [],
            |row| row.get(0),
        )
        .optional()?;

    if last_session_name.as_deref() == Some(current_name) {
        set_last_session_name(transaction, next_name)?;
    }

    Ok(())
}

fn map_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        name: row.get(1)?,
        tag: row.get(2)?,
        repo: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        last_opened_at: row.get(6)?,
        current_revision: row.get(7)?,
    })
}

fn map_session_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        name: row.get(0)?,
        tag: row.get(1)?,
        repo: row.get(2)?,
        last_opened_at: row.get(3)?,
        current_revision: row.get(4)?,
    })
}

fn map_session_overview(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionOverview> {
    Ok(SessionOverview {
        id: row.get(0)?,
        name: row.get(1)?,
        tag: row.get(2)?,
        repo: row.get(3)?,
        updated_at: row.get(4)?,
        last_opened_at: row.get(5)?,
        current_revision: row.get(6)?,
        todo_count: row.get(7)?,
        done_count: row.get(8)?,
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
            .create_session("Writing Sprint", Some("Work Projects"), None, 1_711_275_600)
            .expect("session");

        assert_eq!(created.name, "writing-sprint");
        assert_eq!(created.tag.as_deref(), Some("work-projects"));
        assert_eq!(created.current_revision, 1);

        let listed = database.list_sessions().expect("sessions");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "writing-sprint");
        assert_eq!(listed[0].tag.as_deref(), Some("work-projects"));
    }

    #[test]
    fn overview_aggregate_as_of_uses_historical_visible_state() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let baseline_time = 1_711_275_600;
        let recent_time = baseline_time + (6 * 24 * 60 * 60);

        let baseline = database
            .create_session("Baseline Sprint", Some("work"), None, baseline_time)
            .expect("baseline session");
        database
            .add_todo_with_actor(
                &baseline.name,
                "Draft spec",
                "",
                None,
                crate::domain::todo::TodoActorKind::Human,
                baseline_time + 60,
            )
            .expect("baseline todo");

        let recent = database
            .create_session("Recent Sprint", Some("private"), None, recent_time)
            .expect("recent session");
        let recent_done = database
            .add_todo_with_actor(
                &recent.name,
                "Done task",
                "",
                None,
                crate::domain::todo::TodoActorKind::Agent,
                recent_time + 60,
            )
            .expect("recent done todo");
        database
            .add_todo_with_actor(
                &recent.name,
                "Open task",
                "",
                None,
                crate::domain::todo::TodoActorKind::Human,
                recent_time + 120,
            )
            .expect("recent open todo");
        database
            .set_todo_status_with_actor(
                recent_done.id,
                Some(&recent.name),
                crate::domain::todo::TodoStatus::Done,
                crate::domain::todo::TodoActorKind::Agent,
                recent_time + 180,
            )
            .expect("mark done");

        let before_recent = database
            .overview_aggregate_as_of(recent_time - 1)
            .expect("before recent snapshot");
        assert_eq!(before_recent.total_sessions, 1);
        assert_eq!(before_recent.tagged_sessions, 1);
        assert_eq!(before_recent.total_todos, 1);
        assert_eq!(before_recent.done_todos, 0);
        assert_eq!(before_recent.human_open_todos, 1);
        assert_eq!(before_recent.agent_open_todos, 0);
        assert_eq!(before_recent.human_completed_todos, 0);
        assert_eq!(before_recent.agent_completed_todos, 0);
        assert_eq!(before_recent.average_revision, 2);

        let after_recent = database
            .overview_aggregate_as_of(recent_time + 180)
            .expect("after recent snapshot");
        assert_eq!(after_recent.total_sessions, 2);
        assert_eq!(after_recent.tagged_sessions, 2);
        assert_eq!(after_recent.total_todos, 3);
        assert_eq!(after_recent.done_todos, 1);
        assert_eq!(after_recent.human_open_todos, 2);
        assert_eq!(after_recent.agent_open_todos, 0);
        assert_eq!(after_recent.human_completed_todos, 0);
        assert_eq!(after_recent.agent_completed_todos, 1);
        assert_eq!(after_recent.average_revision, 3);
    }

    #[test]
    fn resolves_recent_session_and_marks_opened() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        database
            .create_session("Writing Sprint", None, None, 1_711_275_600)
            .expect("session");
        database
            .create_session("Reading Sprint", None, None, 1_711_275_700)
            .expect("session");

        assert_eq!(
            database.resolve_session_name(None).expect("recent"),
            "reading-sprint"
        );

        let marked = database
            .mark_session_opened("writing-sprint", 1_711_275_800)
            .expect("marked");
        assert_eq!(marked.name, "writing-sprint");
        assert_eq!(
            database.resolve_session_name(None).expect("recent"),
            "writing-sprint"
        );
    }

    #[test]
    fn lists_overview_rows_in_tag_then_recent_order_with_counts() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let writing = database
            .create_session("Writing Sprint", Some("work"), None, 1_711_275_600)
            .expect("session");
        database
            .add_todo(&writing.name, "Draft spec", "", None, 1_711_275_650)
            .expect("todo");
        database
            .add_todo(&writing.name, "Review keybindings", "", None, 1_711_275_660)
            .expect("todo");
        database
            .set_todo_status(
                1,
                Some(&writing.name),
                crate::domain::todo::TodoStatus::Done,
                1_711_275_670,
            )
            .expect("done");

        let private = database
            .create_session("Reading Sprint", Some("private"), None, 1_711_275_700)
            .expect("session");
        let planning = database
            .create_session("Planning Sprint", Some("work"), None, 1_711_275_705)
            .expect("session");
        database
            .add_todo(&planning.name, "Outline roadmap", "", None, 1_711_275_810)
            .expect("todo");
        let finished = database
            .create_session("Archive Sprint", Some("archive"), None, 1_711_275_750)
            .expect("session");
        let finished_todo = database
            .add_todo(&finished.name, "Ship it", "", None, 1_711_275_760)
            .expect("todo");
        database
            .set_todo_status(
                finished_todo.id,
                Some(&finished.name),
                crate::domain::todo::TodoStatus::Done,
                1_711_275_770,
            )
            .expect("done");
        let inbox = database
            .create_session("Inbox", None, None, 1_711_275_900)
            .expect("session");
        database
            .mark_session_opened(&writing.name, 1_711_275_800)
            .expect("opened");

        let overview = database.list_session_overview().expect("overview");
        assert_eq!(overview.len(), 4);
        assert_eq!(overview[0].name, private.name);
        assert_eq!(overview[0].tag.as_deref(), Some("private"));
        assert_eq!(overview[1].name, planning.name);
        assert_eq!(overview[1].tag.as_deref(), Some("work"));
        assert_eq!(overview[1].updated_at, 1_711_275_810);
        assert_eq!(overview[2].name, writing.name);
        assert_eq!(overview[2].tag.as_deref(), Some("work"));
        assert_eq!(overview[2].updated_at, 1_711_275_670);
        assert_eq!(overview[3].name, inbox.name);
        assert_eq!(overview[3].tag, None);
        assert_eq!(overview[3].updated_at, 1_711_275_900);
        assert_eq!(overview[2].todo_count, 2);
        assert_eq!(overview[2].done_count, 1);
        assert!(overview.iter().all(|session| session.name != finished.name));
    }

    #[test]
    fn deleting_session_cascades_and_updates_recent_pointer() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let writing = database
            .create_session("Writing Sprint", None, None, 1_711_275_600)
            .expect("session");
        database
            .add_todo(&writing.name, "Draft spec", "", None, 1_711_275_650)
            .expect("todo");

        let reading = database
            .create_session("Reading Sprint", Some("private"), None, 1_711_275_700)
            .expect("session");
        let todo = database
            .add_todo(&reading.name, "Review paper", "", None, 1_711_275_750)
            .expect("todo");
        let run = database
            .start_pomodoro(
                crate::domain::pomodoro::PomodoroKind::Focus,
                1_500,
                1_711_275_760,
            )
            .expect("run");

        let deleted = database.delete_session(&reading.name).expect("delete");
        assert_eq!(deleted.name, reading.name);
        assert!(database.get_session_by_name(&reading.name).is_err());
        assert!(database.get_todo(todo.id).is_err());
        let run = database.get_pomodoro_run(run.id).expect("run");
        assert_eq!(run.todo_id, None);
        assert_eq!(run.session_id, None);
        assert_eq!(
            database.resolve_session_name(None).expect("recent"),
            writing.name
        );
    }

    #[test]
    fn deleting_last_session_clears_recent_pointer() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session("Writing Sprint", None, None, 1_711_275_600)
            .expect("session");

        database.delete_session(&session.name).expect("delete");

        assert!(matches!(
            database.resolve_session_name(None),
            Err(crate::error::AppError::NoRecentSession)
        ));
    }

    #[test]
    fn session_head_token_changes_for_same_session_mutations_only() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let writing = database
            .create_session("Writing Sprint", None, None, 1_711_275_600)
            .expect("writing session");
        let initial = database
            .session_head_token(&writing.name)
            .expect("initial head token");

        database
            .add_todo(&writing.name, "Draft spec", "", None, 1_711_275_700)
            .expect("todo");
        let after_todo = database
            .session_head_token(&writing.name)
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
            .create_session("Reading Sprint", None, None, 1_711_275_800)
            .expect("reading session");
        let after_other_session = database
            .session_head_token(&writing.name)
            .expect("unchanged token");
        assert_eq!(after_other_session, after_todo);
    }

    #[test]
    fn updates_and_clears_session_tag_with_new_revision() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session("Writing Sprint", None, None, 1_711_275_600)
            .expect("session");

        let updated = database
            .update_session_tag(&session.name, Some("Private Projects"), 1_711_275_700)
            .expect("set tag");
        assert_eq!(updated.tag.as_deref(), Some("private-projects"));
        assert_eq!(updated.current_revision, 2);
        assert_eq!(updated.updated_at, 1_711_275_700);

        let cleared = database
            .update_session_tag(&session.name, None, 1_711_275_800)
            .expect("clear tag");
        assert_eq!(cleared.tag, None);
        assert_eq!(cleared.current_revision, 3);
        assert_eq!(cleared.updated_at, 1_711_275_800);
    }

    #[test]
    fn updates_and_clears_session_repo_with_new_revision() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session("Writing Sprint", None, None, 1_711_275_600)
            .expect("session");

        let updated = database
            .update_session_repo(
                &session.name,
                Some("https://github.com/SakanaAI/todui-keymove"),
                1_711_275_700,
            )
            .expect("set repo");
        assert_eq!(updated.repo.as_deref(), Some("sakanaai/todui-keymove"));
        assert_eq!(updated.current_revision, 2);

        let cleared = database
            .update_session_repo(&session.name, None, 1_711_275_800)
            .expect("clear repo");
        assert_eq!(cleared.repo, None);
        assert_eq!(cleared.current_revision, 3);
    }

    #[test]
    fn edits_session_name_slug_and_rewrites_all_todo_repos() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session(
                "Writing Sprint",
                Some("work"),
                Some("@SakanaAI/todui"),
                1_711_275_600,
            )
            .expect("session");
        let inherited = database
            .add_todo(&session.name, "Draft spec", "", None, 1_711_275_650)
            .expect("todo");
        let explicit = database
            .add_todo(
                &session.name,
                "Review bindings",
                "",
                Some("@OpenAI/codex"),
                1_711_275_660,
            )
            .expect("todo");

        database
            .mark_session_opened(&session.name, 1_711_275_700)
            .expect("opened");
        let updated = database
            .edit_session(
                &session.name,
                "Deep Work",
                Some("Private"),
                Some("@SakanaAI/todui-keymove"),
                1_711_275_800,
            )
            .expect("edited session");

        assert_eq!(updated.name, "deep-work");
        assert_eq!(updated.tag.as_deref(), Some("private"));
        assert_eq!(updated.repo.as_deref(), Some("sakanaai/todui-keymove"));
        assert_eq!(updated.current_revision, 4);
        assert_eq!(updated.updated_at, 1_711_275_800);
        assert_eq!(
            database.resolve_session_name(None).expect("recent"),
            "deep-work"
        );
        assert!(database.get_session_by_name("writing-sprint").is_err());

        let inherited = database.get_todo(inherited.id).expect("todo");
        let explicit = database.get_todo(explicit.id).expect("todo");
        assert_eq!(inherited.repo.as_deref(), Some("sakanaai/todui-keymove"));
        assert_eq!(explicit.repo.as_deref(), Some("sakanaai/todui-keymove"));
    }

    #[test]
    fn editing_session_to_existing_slug_is_rejected() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let writing = database
            .create_session("Writing Sprint", None, None, 1_711_275_600)
            .expect("session");
        database
            .create_session("Reading Sprint", None, None, 1_711_275_700)
            .expect("session");

        let error = database
            .edit_session(&writing.name, "Reading Sprint", None, None, 1_711_275_800)
            .expect_err("duplicate name");

        assert!(matches!(error, crate::error::AppError::Database(_)));
        assert_eq!(
            database
                .get_session_by_name(&writing.name)
                .expect("session")
                .name,
            "writing-sprint"
        );
    }

    #[test]
    fn editing_session_name_without_repo_change_keeps_todo_repos() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session(
                "Writing Sprint",
                Some("work"),
                Some("@SakanaAI/todui"),
                1_711_275_600,
            )
            .expect("session");
        let inherited = database
            .add_todo(&session.name, "Draft spec", "", None, 1_711_275_650)
            .expect("todo");
        let explicit = database
            .add_todo(
                &session.name,
                "Review bindings",
                "",
                Some("@OpenAI/codex"),
                1_711_275_660,
            )
            .expect("todo");

        let updated = database
            .edit_session(
                &session.name,
                "Research Sprint",
                Some("Deep Work"),
                Some("@SakanaAI/todui"),
                1_711_275_700,
            )
            .expect("edited session");

        assert_eq!(updated.name, "research-sprint");
        assert_eq!(database.get_todo(inherited.id).expect("todo").repo, None);
        assert_eq!(
            database
                .get_todo(explicit.id)
                .expect("todo")
                .repo
                .as_deref(),
            Some("openai/codex")
        );
    }
}
