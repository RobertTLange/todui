#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    Open,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    pub id: i64,
    pub session_id: i64,
    pub title: String,
    pub notes: String,
    pub repo: Option<String>,
    pub status: TodoStatus,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoSource {
    Session,
    Todo,
}

impl RepoSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Todo => "todo",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoTodoMatch {
    pub todo_id: i64,
    pub session_slug: String,
    pub title: String,
    pub status: TodoStatus,
    pub effective_repo: String,
    pub source: RepoSource,
}
