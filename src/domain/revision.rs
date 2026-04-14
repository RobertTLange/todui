#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RevisionMode {
    #[default]
    Head,
    Historical(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionSummary {
    pub revision_number: u32,
    pub created_at: i64,
    pub reason: String,
    pub todo_count: i64,
    pub done_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionTodo {
    pub todo_id: i64,
    pub title: String,
    pub notes: String,
    pub repo: Option<String>,
    pub created_by_kind: crate::domain::todo::TodoActorKind,
    pub completed_by_kind: Option<crate::domain::todo::TodoActorKind>,
    pub status: crate::domain::todo::TodoStatus,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub session: crate::domain::session::Session,
    pub revision: RevisionSummary,
    pub todos: Vec<RevisionTodo>,
    pub mode: RevisionMode,
}
