use crate::cli::{ExportFormat, TimestampMode};
use crate::domain::revision::SessionSnapshot;
use crate::domain::todo::TodoStatus;
use crate::timestamp::format_export_local;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownOptions {
    pub format: ExportFormat,
    pub timestamps: TimestampMode,
    pub include_notes: bool,
    pub open_only: bool,
    pub exported_at: i64,
}

pub fn render(snapshot: &SessionSnapshot, options: &MarkdownOptions) -> String {
    let mut lines = vec![
        format!("# Session: {}", snapshot.session.name),
        String::new(),
        format!("- session: {}", snapshot.session.name),
    ];
    if let Some(tag) = &snapshot.session.tag {
        lines.push(format!("- tag: {tag}"));
    }
    lines.extend([
        format!("- revision: {}", snapshot.revision.revision_number),
        format!(
            "- exported-at: {}",
            format_export_local(options.exported_at)
        ),
        format!(
            "- session-updated-at: {}",
            format_export_local(snapshot.session.updated_at)
        ),
        String::new(),
        String::from("## Todos"),
        String::new(),
    ]);

    for todo in snapshot
        .todos
        .iter()
        .filter(|todo| !options.open_only || todo.status == TodoStatus::Open)
    {
        let marker = match (options.format, todo.status) {
            (ExportFormat::Gfm, TodoStatus::Open) => "- [ ]",
            (ExportFormat::Gfm, TodoStatus::Done) => "- [x]",
            (ExportFormat::Plain, TodoStatus::Open) => "- TODO:",
            (ExportFormat::Plain, TodoStatus::Done) => "- DONE:",
        };
        lines.push(format!("{marker} {}", todo.title));

        match options.timestamps {
            TimestampMode::Full => {
                lines.push(format!(
                    "  - created: {}",
                    format_export_local(todo.created_at)
                ));
                lines.push(format!(
                    "  - updated: {}",
                    format_export_local(todo.updated_at)
                ));
                if let Some(completed_at) = todo.completed_at {
                    lines.push(format!(
                        "  - completed: {}",
                        format_export_local(completed_at)
                    ));
                }
            }
            TimestampMode::Compact => match todo.status {
                TodoStatus::Open => lines.push(format!(
                    "  - created: {}",
                    format_export_local(todo.created_at)
                )),
                TodoStatus::Done => {
                    if let Some(completed_at) = todo.completed_at {
                        lines.push(format!(
                            "  - completed: {}",
                            format_export_local(completed_at)
                        ));
                    }
                }
            },
            TimestampMode::None => {}
        }

        if options.include_notes && !todo.notes.trim().is_empty() {
            lines.push(format!("  - notes: {}", todo.notes.trim()));
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use crate::cli::{ExportFormat, TimestampMode};
    use crate::domain::revision::{RevisionMode, RevisionSummary, RevisionTodo, SessionSnapshot};
    use crate::domain::session::Session;
    use crate::domain::todo::TodoStatus;
    use crate::export::markdown::{MarkdownOptions, render};

    #[test]
    fn renders_gfm_export() {
        let snapshot = SessionSnapshot {
            session: Session {
                id: 1,
                name: String::from("writing-sprint"),
                tag: Some(String::from("work")),
                repo: None,
                created_at: 1,
                updated_at: 1_711_275_900,
                last_opened_at: 1_711_275_900,
                current_revision: 2,
            },
            revision: RevisionSummary {
                revision_number: 2,
                created_at: 1_711_275_900,
                reason: String::from("todo added"),
                todo_count: 1,
                done_count: 0,
            },
            todos: vec![RevisionTodo {
                todo_id: 1,
                title: String::from("Draft spec"),
                notes: String::from("cover db"),
                repo: None,
                status: TodoStatus::Open,
                position: 1,
                created_at: 1_711_275_700,
                updated_at: 1_711_275_700,
                completed_at: None,
            }],
            mode: RevisionMode::Head,
        };

        let output = render(
            &snapshot,
            &MarkdownOptions {
                format: ExportFormat::Gfm,
                timestamps: TimestampMode::Compact,
                include_notes: true,
                open_only: false,
                exported_at: 1_711_276_000,
            },
        );

        assert!(output.contains("- [ ] Draft spec"));
        assert!(output.contains("- tag: work"));
        assert!(output.contains("notes: cover db"));
    }
}
