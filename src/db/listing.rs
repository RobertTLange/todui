//! Read-only listings for `todui list`: every session with live todo counts,
//! and todos with the name of the session they belong to.

use rusqlite::params;

use crate::db::Database;
use crate::db::todos::map_todo;
use crate::domain::todo::Todo;
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedSession {
    pub name: String,
    pub tag: Option<String>,
    pub repo: Option<String>,
    pub last_opened_at: i64,
    pub current_revision: u32,
    pub open_count: i64,
    pub done_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedTodo {
    pub session_name: String,
    pub todo: Todo,
}

impl Database {
    /// Every session, most recently opened first, with its live open/done counts.
    pub fn list_sessions_with_counts(&self) -> Result<Vec<ListedSession>> {
        let mut statement = self.connection.prepare(
            "SELECT
                sessions.slug,
                sessions.tag,
                sessions.repo,
                sessions.last_opened_at,
                sessions.current_revision,
                COALESCE(SUM(todos.status = 'open'), 0),
                COALESCE(SUM(todos.status = 'done'), 0)
             FROM sessions
             LEFT JOIN todos ON todos.session_id = sessions.id
             GROUP BY sessions.id
             ORDER BY sessions.last_opened_at DESC, sessions.id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ListedSession {
                name: row.get(0)?,
                tag: row.get(1)?,
                repo: row.get(2)?,
                last_opened_at: row.get(3)?,
                current_revision: row.get(4)?,
                open_count: row.get(5)?,
                done_count: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Todos of one session (or all), in session order then position; with
    /// `open_only`, done todos are left out. An unknown session is an error.
    pub fn list_todos_with_sessions(
        &self,
        session_name: Option<&str>,
        open_only: bool,
    ) -> Result<Vec<ListedTodo>> {
        if let Some(name) = session_name {
            self.get_session_by_name(name)?;
        }
        let mut statement = self.connection.prepare(
            "SELECT todos.id, todos.session_id, todos.title, todos.notes, todos.repo,
                    todos.created_by_kind, todos.completed_by_kind, todos.status, todos.position,
                    todos.created_at, todos.updated_at, todos.completed_at, sessions.slug
             FROM todos
             JOIN sessions ON sessions.id = todos.session_id
             WHERE (?1 IS NULL OR sessions.slug = ?1)
               AND (?2 = 0 OR todos.status = 'open')
             ORDER BY sessions.last_opened_at DESC, sessions.id DESC, todos.position ASC",
        )?;
        let rows = statement.query_map(params![session_name, open_only], |row| {
            Ok(ListedTodo {
                todo: map_todo(row)?,
                session_name: row.get(12)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
}
