//! Output of `todui list`: JSON for programs (sessions and todos with ids), or one
//! tab-separated row per todo for people and shell pipelines.

use std::io::Write;

use serde::Serialize;

use crate::db::listing::{ListedSession, ListedTodo};
use crate::domain::todo::{TodoActorKind, TodoStatus};
use crate::error::Result;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionJson<'a> {
    slug: &'a str,
    tag: Option<&'a str>,
    repo: Option<&'a str>,
    last_opened_at: i64,
    revision: u32,
    open_count: i64,
    done_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TodoJson<'a> {
    id: i64,
    session: &'a str,
    title: &'a str,
    notes: &'a str,
    repo: Option<&'a str>,
    status: &'static str,
    position: i64,
    created_at: i64,
    completed_at: Option<i64>,
    created_by: &'static str,
    completed_by: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct ListingJson<'a> {
    sessions: Vec<SessionJson<'a>>,
    todos: Vec<TodoJson<'a>>,
}

fn status_word(status: TodoStatus) -> &'static str {
    match status {
        TodoStatus::Open => "open",
        TodoStatus::Done => "done",
    }
}

fn session_json(session: &ListedSession) -> SessionJson<'_> {
    SessionJson {
        slug: &session.name,
        tag: session.tag.as_deref(),
        repo: session.repo.as_deref(),
        last_opened_at: session.last_opened_at,
        revision: session.current_revision,
        open_count: session.open_count,
        done_count: session.done_count,
    }
}

fn todo_json(listed: &ListedTodo) -> TodoJson<'_> {
    let todo = &listed.todo;
    TodoJson {
        id: todo.id,
        session: &listed.session_name,
        title: &todo.title,
        notes: &todo.notes,
        repo: todo.repo.as_deref(),
        status: status_word(todo.status),
        position: todo.position,
        created_at: todo.created_at,
        completed_at: todo.completed_at,
        created_by: todo.created_by_kind.as_str(),
        completed_by: todo.completed_by_kind.map(TodoActorKind::as_str),
    }
}

/// `{"sessions": [...], "todos": [...]}` on one line.
pub fn write_json<W: Write>(
    writer: &mut W,
    sessions: &[ListedSession],
    todos: &[ListedTodo],
) -> Result<()> {
    let listing = ListingJson {
        sessions: sessions.iter().map(session_json).collect(),
        todos: todos.iter().map(todo_json).collect(),
    };
    let text = serde_json::to_string(&listing).map_err(std::io::Error::other)?;
    writeln!(writer, "{text}")?;
    Ok(())
}

/// `<id>\t<session>\t<status>\t<position>\t<title>` per todo.
pub fn write_rows<W: Write>(writer: &mut W, todos: &[ListedTodo]) -> Result<()> {
    for listed in todos {
        let todo = &listed.todo;
        writeln!(
            writer,
            "{}\t{}\t{}\t{}\t{}",
            todo.id,
            listed.session_name,
            status_word(todo.status),
            todo.position,
            todo.title
        )?;
    }
    Ok(())
}
