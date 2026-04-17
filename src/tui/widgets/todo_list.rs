use std::cmp::Ordering;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};

use crate::domain::pomodoro::PomodoroRun;
use crate::domain::revision::RevisionTodo;
use crate::domain::todo::{TodoActorKind, TodoStatus};
use crate::timestamp::format_month_day_local;
use crate::tui::theme::{SelectionTone, SurfaceTone, TextTone, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoSection {
    Open,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoClickTarget {
    Checkbox { section: TodoSection, row: usize },
    Row { section: TodoSection, row: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TodoListAreas {
    pub open: Rect,
    pub completed: Rect,
}

#[derive(Debug, Clone)]
pub struct GroupedTodos<'a> {
    open: Vec<&'a RevisionTodo>,
    completed: Vec<&'a RevisionTodo>,
}

impl<'a> GroupedTodos<'a> {
    pub fn new(todos: &'a [RevisionTodo]) -> Self {
        Self::new_with_filter(todos, |_| true)
    }

    pub fn new_with_filter<F>(todos: &'a [RevisionTodo], mut include: F) -> Self
    where
        F: FnMut(&RevisionTodo) -> bool,
    {
        let mut open = todos
            .iter()
            .filter(|todo| include(todo))
            .filter(|todo| matches!(todo.status, TodoStatus::Open))
            .collect::<Vec<_>>();
        open.sort_by_key(|todo| (todo.position, todo.todo_id));

        let mut completed = todos
            .iter()
            .filter(|todo| include(todo))
            .filter(|todo| matches!(todo.status, TodoStatus::Done))
            .collect::<Vec<_>>();
        completed.sort_by(compare_completed_todos);

        Self { open, completed }
    }

    pub fn open(&self) -> &[&'a RevisionTodo] {
        &self.open
    }

    pub fn completed(&self) -> &[&'a RevisionTodo] {
        &self.completed
    }

    pub fn len(&self) -> usize {
        self.open.len() + self.completed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn todo_at_flat_index(&self, index: usize) -> Option<&'a RevisionTodo> {
        if index < self.open.len() {
            self.open.get(index).copied()
        } else {
            self.completed.get(index - self.open.len()).copied()
        }
    }

    pub fn flat_index_of(&self, todo_id: i64) -> Option<usize> {
        self.open
            .iter()
            .position(|todo| todo.todo_id == todo_id)
            .or_else(|| {
                self.completed
                    .iter()
                    .position(|todo| todo.todo_id == todo_id)
                    .map(|index| self.open.len() + index)
            })
    }

    pub fn section_row_for_flat_index(&self, index: usize) -> Option<(TodoSection, usize)> {
        if index < self.open.len() {
            Some((TodoSection::Open, index))
        } else {
            let completed_index = index.checked_sub(self.open.len())?;
            self.completed
                .get(completed_index)
                .map(|_| (TodoSection::Completed, completed_index))
        }
    }

    pub fn flat_index_for_section_row(&self, section: TodoSection, row: usize) -> Option<usize> {
        match section {
            TodoSection::Open => self.open.get(row).map(|_| row),
            TodoSection::Completed => self.completed.get(row).map(|_| self.open.len() + row),
        }
    }

    #[cfg(test)]
    fn open_ids(&self) -> Vec<i64> {
        self.open.iter().map(|todo| todo.todo_id).collect()
    }

    #[cfg(test)]
    fn completed_ids(&self) -> Vec<i64> {
        self.completed.iter().map(|todo| todo.todo_id).collect()
    }
}

pub fn split_todo_list_area(area: Rect) -> TodoListAreas {
    let open_height = area.height.saturating_add(1) / 2;
    let completed_height = area.height.saturating_sub(open_height);
    let panes = Layout::vertical([
        Constraint::Length(open_height),
        Constraint::Length(completed_height),
    ])
    .split(area);

    TodoListAreas {
        open: panes[0],
        completed: panes[1],
    }
}

pub fn section_visible_rows(area: Rect) -> usize {
    area.height.saturating_sub(3).max(1) as usize
}

pub fn todo_section_table(
    title: &'static str,
    section: TodoSection,
    todos: &[&RevisionTodo],
    scroll_offset: usize,
    visible_rows: usize,
    _run: Option<&PomodoroRun>,
    theme: &Theme,
) -> Table<'static> {
    let rows = todos
        .iter()
        .skip(scroll_offset)
        .take(visible_rows)
        .map(|todo| todo_table_row(todo, theme))
        .collect::<Vec<_>>();

    let tone = section_surface_tone(section);

    Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(11),
        ],
    )
    .header(
        Row::new([
            Cell::from(""),
            Cell::from("Title"),
            Cell::from("Last Update"),
        ])
        .style(theme.surface_title_style(tone)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(theme.surface_style(SurfaceTone::Neutral))
            .border_style(theme.surface_border_style(tone))
            .title_style(theme.surface_title_style(tone)),
    )
    .column_spacing(1)
    .row_highlight_style(section_highlight_style(theme, section).add_modifier(Modifier::BOLD))
}

pub fn section_state(selected_row: Option<usize>) -> TableState {
    let mut state = TableState::default();
    state.select(selected_row);
    state
}

pub fn todo_click_target(
    areas: TodoListAreas,
    open_scroll_offset: usize,
    completed_scroll_offset: usize,
    x: u16,
    y: u16,
) -> Option<TodoClickTarget> {
    section_click_target(areas.open, TodoSection::Open, open_scroll_offset, x, y).or_else(|| {
        section_click_target(
            areas.completed,
            TodoSection::Completed,
            completed_scroll_offset,
            x,
            y,
        )
    })
}

pub fn todo_time_label(todo: &RevisionTodo) -> String {
    let timestamp = match todo.status {
        TodoStatus::Open => todo.created_at,
        TodoStatus::Done => todo.completed_at.unwrap_or(todo.updated_at),
    };
    format_month_day_local(timestamp)
}

fn compare_completed_todos(left: &&RevisionTodo, right: &&RevisionTodo) -> Ordering {
    completion_sort_timestamp(right)
        .cmp(&completion_sort_timestamp(left))
        .then_with(|| right.updated_at.cmp(&left.updated_at))
        .then_with(|| left.position.cmp(&right.position))
        .then_with(|| left.todo_id.cmp(&right.todo_id))
}

fn completion_sort_timestamp(todo: &RevisionTodo) -> i64 {
    todo.completed_at.unwrap_or(todo.updated_at)
}

fn section_click_target(
    area: Rect,
    section: TodoSection,
    scroll_offset: usize,
    x: u16,
    y: u16,
) -> Option<TodoClickTarget> {
    let inner_x = area.x.saturating_add(1);
    let inner_y = area.y.saturating_add(1);
    if x < inner_x || y <= inner_y || y >= area.bottom().saturating_sub(1) {
        return None;
    }

    let row = scroll_offset + usize::from(y.saturating_sub(inner_y + 1));
    if x <= inner_x.saturating_add(2) {
        Some(TodoClickTarget::Checkbox { section, row })
    } else {
        Some(TodoClickTarget::Row { section, row })
    }
}

fn todo_table_row(todo: &RevisionTodo, theme: &Theme) -> Row<'static> {
    Row::new([
        Cell::from(todo_checkbox(todo)).style(todo_checkbox_style(theme, todo)),
        Cell::from(format!("{} {}", todo_provenance_badge(todo), todo.title))
            .style(todo_title_style(theme, todo)),
        Cell::from(todo_time_label(todo)).style(todo_timestamp_style(theme, todo)),
    ])
}

pub fn todo_provenance_badge(todo: &RevisionTodo) -> String {
    match todo.completed_by_kind {
        Some(completed_by_kind) if matches!(todo.status, TodoStatus::Done) => {
            match todo.created_by_kind == completed_by_kind {
                true => format!("[{}]", actor_short_label(todo.created_by_kind)),
                false => format!(
                    "[{}>{}]",
                    actor_short_label(todo.created_by_kind),
                    actor_short_label(completed_by_kind)
                ),
            }
        }
        _ => format!("[{}]", actor_short_label(todo.created_by_kind)),
    }
}

fn section_surface_tone(section: TodoSection) -> SurfaceTone {
    match section {
        TodoSection::Open => SurfaceTone::Open,
        TodoSection::Completed => SurfaceTone::Completed,
    }
}

fn section_highlight_style(theme: &Theme, section: TodoSection) -> Style {
    match section {
        TodoSection::Open => theme.selection_style(SelectionTone::Open),
        TodoSection::Completed => theme.selection_style(SelectionTone::Completed),
    }
}

fn todo_checkbox_style(theme: &Theme, todo: &RevisionTodo) -> Style {
    match todo.status {
        TodoStatus::Open => theme.text_style(TextTone::Open),
        TodoStatus::Done => theme.text_style(TextTone::Completed),
    }
}

fn todo_title_style(theme: &Theme, todo: &RevisionTodo) -> Style {
    match todo.status {
        TodoStatus::Open => theme.text_style(TextTone::Default),
        TodoStatus::Done => theme.text_style(TextTone::Muted),
    }
}

fn todo_timestamp_style(theme: &Theme, todo: &RevisionTodo) -> Style {
    match todo.status {
        TodoStatus::Open => theme.text_style(TextTone::Muted),
        TodoStatus::Done => theme.text_style(TextTone::Meta),
    }
}

fn todo_checkbox(todo: &RevisionTodo) -> &'static str {
    match todo.status {
        TodoStatus::Open => "[ ]",
        TodoStatus::Done => "[x]",
    }
}

fn actor_short_label(actor_kind: TodoActorKind) -> &'static str {
    actor_kind.short_label()
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use ratatui::layout::Rect;

    use super::{
        GroupedTodos, TodoClickTarget, TodoSection, section_highlight_style, todo_checkbox_style,
        todo_click_target, todo_provenance_badge, todo_time_label, todo_timestamp_style,
    };
    use crate::domain::revision::RevisionTodo;
    use crate::domain::todo::TodoStatus;
    use crate::tui::theme::Theme;

    #[test]
    fn groups_open_and_completed_todos_with_recent_completions_first() {
        let todos = vec![
            todo(10, TodoStatus::Done, 3, 100, 220, Some(260)),
            todo(11, TodoStatus::Open, 1, 90, 200, None),
            todo(12, TodoStatus::Done, 2, 95, 210, Some(300)),
            todo(13, TodoStatus::Open, 4, 110, 230, None),
        ];

        let grouped = GroupedTodos::new(&todos);

        assert_eq!(grouped.open_ids(), vec![11, 13]);
        assert_eq!(grouped.completed_ids(), vec![12, 10]);
        assert_eq!(grouped.flat_index_of(11), Some(0));
        assert_eq!(grouped.flat_index_of(12), Some(2));
        assert_eq!(
            grouped.section_row_for_flat_index(3),
            Some((TodoSection::Completed, 1))
        );
    }

    #[test]
    fn click_targets_include_the_correct_section_and_row() {
        let areas = super::split_todo_list_area(Rect::new(0, 0, 40, 12));

        assert_eq!(todo_click_target(areas, 0, 0, 0, 2), None);
        assert_eq!(
            todo_click_target(areas, 0, 0, 1, 2),
            Some(TodoClickTarget::Checkbox {
                section: TodoSection::Open,
                row: 0,
            })
        );
        assert_eq!(
            todo_click_target(areas, 0, 0, 6, 3),
            Some(TodoClickTarget::Row {
                section: TodoSection::Open,
                row: 1,
            })
        );
        assert_eq!(
            todo_click_target(areas, 0, 0, 6, 8),
            Some(TodoClickTarget::Row {
                section: TodoSection::Completed,
                row: 0,
            })
        );
    }

    #[test]
    fn timestamp_helper_preserves_completion_semantics() {
        let open = todo(21, TodoStatus::Open, 1, 120, 180, None);
        let done = todo(22, TodoStatus::Done, 2, 121, 181, Some(240));
        assert_eq!(
            todo_time_label(&open),
            crate::timestamp::format_month_day_local(120)
        );
        assert_eq!(
            todo_time_label(&done),
            crate::timestamp::format_month_day_local(240)
        );
    }

    #[test]
    fn styling_helpers_separate_open_completed_and_timestamps() {
        let theme = Theme::default();
        let open = todo(31, TodoStatus::Open, 1, 120, 180, None);
        let done = todo(32, TodoStatus::Done, 2, 121, 181, Some(240));

        assert_eq!(
            section_highlight_style(&theme, TodoSection::Open).bg,
            Some(Color::Rgb(24, 60, 110))
        );
        assert_eq!(
            section_highlight_style(&theme, TodoSection::Completed).bg,
            Some(Color::Rgb(74, 23, 29))
        );
        assert_ne!(
            todo_checkbox_style(&theme, &open).fg,
            todo_checkbox_style(&theme, &done).fg
        );
        assert_ne!(
            todo_timestamp_style(&theme, &open).fg,
            todo_timestamp_style(&theme, &done).fg
        );
    }

    #[test]
    fn provenance_badge_distinguishes_mixed_completion() {
        let mut todo = todo(41, TodoStatus::Done, 1, 120, 180, Some(240));
        todo.created_by_kind = crate::domain::todo::TodoActorKind::Agent;
        todo.completed_by_kind = Some(crate::domain::todo::TodoActorKind::Human);

        assert_eq!(todo_provenance_badge(&todo), "[A>H]");
    }

    fn todo(
        todo_id: i64,
        status: TodoStatus,
        position: i64,
        created_at: i64,
        updated_at: i64,
        completed_at: Option<i64>,
    ) -> RevisionTodo {
        RevisionTodo {
            todo_id,
            title: format!("todo-{todo_id}"),
            notes: String::new(),
            repo: None,
            created_by_kind: crate::domain::todo::TodoActorKind::Human,
            completed_by_kind: matches!(status, TodoStatus::Done)
                .then_some(crate::domain::todo::TodoActorKind::Human),
            status,
            position,
            created_at,
            updated_at,
            completed_at,
        }
    }
}
