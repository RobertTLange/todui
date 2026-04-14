use std::collections::HashMap;

use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::domain::revision::{RevisionSummary, RevisionTodo};
use crate::domain::todo::TodoStatus;
use crate::timestamp::format_full_local;
use crate::tui::theme::{SurfaceTone, TextTone, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionHistoryEventKind {
    Added,
    Edited,
    Completed,
    Reopened,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionHistoryEvent {
    pub timestamp: i64,
    pub revision_number: u32,
    pub todo_id: i64,
    pub title: String,
    pub kind: SessionHistoryEventKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RevisionTodoSnapshot {
    pub revision: RevisionSummary,
    pub todos: Vec<RevisionTodo>,
}

pub(crate) fn derive_session_history_events(
    snapshots: &[RevisionTodoSnapshot],
) -> Vec<SessionHistoryEvent> {
    let mut ordered = snapshots.to_vec();
    ordered.sort_by_key(|snapshot| snapshot.revision.revision_number);

    let mut revision_events = Vec::new();
    for window in ordered.windows(2) {
        let previous = &window[0];
        let current = &window[1];
        let previous_by_id = previous
            .todos
            .iter()
            .map(|todo| (todo.todo_id, todo))
            .collect::<HashMap<_, _>>();
        let current_by_id = current
            .todos
            .iter()
            .map(|todo| (todo.todo_id, todo))
            .collect::<HashMap<_, _>>();
        let mut events = Vec::new();

        for todo in &current.todos {
            let Some(previous_todo) = previous_by_id.get(&todo.todo_id) else {
                events.push(SessionHistoryEvent {
                    timestamp: current.revision.created_at,
                    revision_number: current.revision.revision_number,
                    todo_id: todo.todo_id,
                    title: todo.title.clone(),
                    kind: SessionHistoryEventKind::Added,
                });
                continue;
            };

            let kind = match (previous_todo.status, todo.status) {
                (TodoStatus::Open, TodoStatus::Done) => Some(SessionHistoryEventKind::Completed),
                (TodoStatus::Done, TodoStatus::Open) => Some(SessionHistoryEventKind::Reopened),
                _ if previous_todo.title != todo.title || previous_todo.notes != todo.notes => {
                    Some(SessionHistoryEventKind::Edited)
                }
                _ => None,
            };

            if let Some(kind) = kind {
                events.push(SessionHistoryEvent {
                    timestamp: current.revision.created_at,
                    revision_number: current.revision.revision_number,
                    todo_id: todo.todo_id,
                    title: todo.title.clone(),
                    kind,
                });
            }
        }

        for todo in &previous.todos {
            if current_by_id.contains_key(&todo.todo_id) {
                continue;
            }
            events.push(SessionHistoryEvent {
                timestamp: current.revision.created_at,
                revision_number: current.revision.revision_number,
                todo_id: todo.todo_id,
                title: todo.title.clone(),
                kind: SessionHistoryEventKind::Deleted,
            });
        }

        revision_events.push(events);
    }

    revision_events.into_iter().rev().flatten().collect()
}

pub(crate) fn session_history_panel(
    theme: &Theme,
    events: &[SessionHistoryEvent],
    width: u16,
) -> Paragraph<'static> {
    let inner_width = usize::from(width.saturating_sub(2));
    let text = if events.is_empty() {
        Text::from(vec![
            Line::from("No note history yet."),
            Line::from(String::new()),
            Line::from("Add, edit, complete, or delete a todo to populate this feed."),
        ])
    } else {
        Text::from(
            events
                .iter()
                .flat_map(|event| {
                    let timestamp = format_full_local(event.timestamp);
                    let first_rest = format!(" - {}", event.kind.label());
                    let title = normalized_single_line(&event.title);
                    let title_text = if title.is_empty() {
                        "-"
                    } else {
                        title.as_str()
                    };
                    let second = truncate_with_ellipsis(&format!("  {title_text}"), inner_width);

                    vec![
                        Line::from(vec![
                            Span::styled(timestamp.clone(), theme.text_style(TextTone::Muted)),
                            Span::styled(
                                truncate_with_ellipsis(
                                    &first_rest,
                                    inner_width.saturating_sub(timestamp.chars().count()),
                                ),
                                theme.text_style(event.kind.text_tone()),
                            ),
                        ]),
                        Line::from(Span::styled(second, theme.text_style(TextTone::Default))),
                    ]
                })
                .collect::<Vec<_>>(),
        )
    };

    Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("History")
                .style(theme.surface_style(SurfaceTone::Neutral))
                .border_style(theme.surface_border_style(SurfaceTone::History))
                .title_style(theme.surface_title_style(SurfaceTone::History)),
        )
        .style(theme.surface_style(SurfaceTone::Neutral))
}

impl SessionHistoryEventKind {
    fn label(self) -> &'static str {
        match self {
            Self::Added => "Added",
            Self::Edited => "Edited",
            Self::Completed => "Completed",
            Self::Reopened => "Reopened",
            Self::Deleted => "Deleted",
        }
    }

    fn text_tone(self) -> TextTone {
        match self {
            Self::Added | Self::Reopened => TextTone::Open,
            Self::Edited => TextTone::Meta,
            Self::Completed => TextTone::Completed,
            Self::Deleted => TextTone::Danger,
        }
    }
}

fn normalized_single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_with_ellipsis(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let truncated = text.chars().take(max_chars - 3).collect::<String>();
    format!("{truncated}...")
}

#[cfg(test)]
mod tests {
    use crate::domain::revision::{RevisionSummary, RevisionTodo};
    use crate::domain::todo::TodoStatus;

    use super::{
        RevisionTodoSnapshot, SessionHistoryEvent, SessionHistoryEventKind,
        derive_session_history_events,
    };

    #[test]
    fn derives_added_edited_status_and_deleted_events_newest_first() {
        let snapshots = vec![
            snapshot(1, 1_711_275_600, vec![]),
            snapshot(
                2,
                1_711_275_700,
                vec![todo(7, "Draft spec", "cover db", None, TodoStatus::Open)],
            ),
            snapshot(
                3,
                1_711_275_800,
                vec![todo(
                    7,
                    "Draft spec",
                    "cover db and tui",
                    None,
                    TodoStatus::Open,
                )],
            ),
            snapshot(
                4,
                1_711_275_900,
                vec![todo(
                    7,
                    "Draft spec",
                    "cover db and tui",
                    None,
                    TodoStatus::Done,
                )],
            ),
            snapshot(
                5,
                1_711_276_000,
                vec![todo(
                    7,
                    "Draft spec",
                    "cover db and tui",
                    None,
                    TodoStatus::Open,
                )],
            ),
            snapshot(6, 1_711_276_100, vec![]),
        ];

        assert_eq!(
            derive_session_history_events(&snapshots),
            vec![
                event(
                    1_711_276_100,
                    6,
                    7,
                    "Draft spec",
                    SessionHistoryEventKind::Deleted
                ),
                event(
                    1_711_276_000,
                    5,
                    7,
                    "Draft spec",
                    SessionHistoryEventKind::Reopened
                ),
                event(
                    1_711_275_900,
                    4,
                    7,
                    "Draft spec",
                    SessionHistoryEventKind::Completed
                ),
                event(
                    1_711_275_800,
                    3,
                    7,
                    "Draft spec",
                    SessionHistoryEventKind::Edited
                ),
                event(
                    1_711_275_700,
                    2,
                    7,
                    "Draft spec",
                    SessionHistoryEventKind::Added
                ),
            ]
        );
    }

    #[test]
    fn ignores_repo_only_revisions() {
        let snapshots = vec![
            snapshot(
                1,
                1_711_275_600,
                vec![todo(7, "Draft spec", "cover db", None, TodoStatus::Open)],
            ),
            snapshot(
                2,
                1_711_275_700,
                vec![todo(
                    7,
                    "Draft spec",
                    "cover db",
                    Some("sakanaai/todui"),
                    TodoStatus::Open,
                )],
            ),
        ];

        assert!(derive_session_history_events(&snapshots).is_empty());
    }

    fn snapshot(
        revision_number: u32,
        created_at: i64,
        todos: Vec<RevisionTodo>,
    ) -> RevisionTodoSnapshot {
        RevisionTodoSnapshot {
            revision: RevisionSummary {
                revision_number,
                created_at,
                reason: String::from("todo changed"),
                todo_count: todos.len() as i64,
                done_count: todos
                    .iter()
                    .filter(|todo| matches!(todo.status, TodoStatus::Done))
                    .count() as i64,
            },
            todos,
        }
    }

    fn todo(
        todo_id: i64,
        title: &str,
        notes: &str,
        repo: Option<&str>,
        status: TodoStatus,
    ) -> RevisionTodo {
        RevisionTodo {
            todo_id,
            title: title.to_string(),
            notes: notes.to_string(),
            repo: repo.map(str::to_string),
            created_by_kind: crate::domain::todo::TodoActorKind::Human,
            completed_by_kind: matches!(status, TodoStatus::Done)
                .then_some(crate::domain::todo::TodoActorKind::Human),
            status,
            position: 1,
            created_at: 1_711_275_600,
            updated_at: 1_711_275_600,
            completed_at: matches!(status, TodoStatus::Done).then_some(1_711_275_900),
        }
    }

    fn event(
        timestamp: i64,
        revision_number: u32,
        todo_id: i64,
        title: &str,
        kind: SessionHistoryEventKind,
    ) -> SessionHistoryEvent {
        SessionHistoryEvent {
            timestamp,
            revision_number,
            todo_id,
            title: title.to_string(),
            kind,
        }
    }
}
