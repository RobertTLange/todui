use std::ffi::OsString;
use std::fmt::{Display, Formatter};
use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::config;
use crate::db::Database;
use crate::domain::todo::TodoStatus;
use crate::error::{AppError, Result};
use crate::export::markdown::{self, MarkdownOptions};
use crate::timestamp::{format_export_local, now_utc_timestamp};
use crate::tui;

const CLI_LONG_ABOUT: &str = "Manage session-based todo lists from the CLI or full-screen TUI.\n\nRun `todui` without a subcommand to open the session overview. Use `resume` to jump straight into a session, `session` to manage sessions and history, and `export md` to write a markdown snapshot.\n\nAutomation recipes:\n  List sessions: `todui session list`\n  Add a todo with kwargs: `todui add \"Draft spec\" --session writing-sprint --note \"cover CLI\"`\n  Inspect todo titles and notes from the CLI: `todui export md writing-sprint --include-notes`\n  Open a session interactively: `todui resume writing-sprint`\n\nNotes:\n  `session list` prints tab-separated rows.\n  `add` prints the new todo id on stdout.\n  There is no dedicated `todo show <id>` command; use `export md` for CLI inspection or `resume` for the TUI.";

const SESSION_LONG_ABOUT: &str = "Create, list, tag, delete, and inspect revision history for sessions.\n\nUse `todui session list` to discover available session slugs before calling `add`, `resume`, or `export md`.";

const SESSION_LONG_HELP: &str = "Examples:\n  todui session list\n  todui session new \"Writing Sprint\" --tag work\n  todui session history writing-sprint\n  todui session tag writing-sprint --set private";

const SESSION_LIST_LONG_ABOUT: &str = "Print one session per line in a tab-separated format for scripts and agents.\n\nColumns:\n  <slug>\\t<display-name>\\t<tag-or->\\t<last-opened-local-time>\\tr<current-revision>";

const SESSION_HISTORY_LONG_ABOUT: &str = "Print one revision per line in a tab-separated format for scripts and agents.\n\nColumns:\n  r<revision-number>\\t<created-at-local-time>\\t<reason>\\t<todo-count>\\t<done-count>";

const ADD_LONG_ABOUT: &str = "Create a new todo in a session.\n\nIf `--session` is omitted, todui resolves the most recently opened session. On success, stdout contains the new todo id as a single integer.";

const ADD_LONG_HELP: &str = "Examples:\n  todui add \"Draft spec\" --session writing-sprint\n  todui add \"Review keybindings\" --session writing-sprint --note \"Ghostty + mouse\"";

const EDIT_LONG_ABOUT: &str = "Update the title and/or note for an existing todo.\n\nPass at least one of `--title`, `--note`, or `--clear-note`. On success, stdout prints `<todo-id>\\tedited`.";

const EDIT_LONG_HELP: &str = "Examples:\n  todui edit 7 --session writing-sprint --title \"Draft final spec\"\n  todui edit 7 --session writing-sprint --note \"cover CLI and TUI\"\n  todui edit 7 --session writing-sprint --clear-note";

const RESUME_LONG_ABOUT: &str = "Open a session in the full-screen TUI.\n\nWithout arguments, todui resumes the most recently opened session head. `--revision` opens a historical snapshot in read-only mode.";

const EXPORT_MD_LONG_ABOUT: &str = "Render the live head or a historical revision as markdown.\n\nUse this command when an agent needs todo titles or note bodies from the CLI. `--include-notes` includes note text, `--open-only` filters out completed todos, and `--revision` exports a read-only historical snapshot.";

const EXPORT_MD_LONG_HELP: &str = "Examples:\n  todui export md writing-sprint --include-notes\n  todui export md writing-sprint --revision 3 --timestamps full\n  todui export md writing-sprint --output sprint.md --open-only";

#[derive(Debug, Parser)]
#[command(
    name = "todui",
    version,
    about = "Terminal todo sessions with revisions",
    long_about = CLI_LONG_ABOUT
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(
        about = "Manage sessions, tags, and revision history",
        long_about = SESSION_LONG_ABOUT,
        after_long_help = SESSION_LONG_HELP
    )]
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    #[command(
        about = "Add a todo to a session",
        long_about = ADD_LONG_ABOUT,
        after_long_help = ADD_LONG_HELP
    )]
    Add {
        #[arg(help = "Todo title to create")]
        title: String,
        #[arg(
            long,
            help = "Session slug. Defaults to the most recently opened session"
        )]
        session: Option<String>,
        #[arg(long = "note", help = "Optional note text stored on the todo")]
        note: Option<String>,
    },
    #[command(about = "Delete a todo from a session")]
    Delete {
        #[arg(help = "Todo id to delete")]
        todo_id: i64,
        #[arg(
            long,
            help = "Session slug. Defaults to the most recently opened session"
        )]
        session: Option<String>,
    },
    #[command(
        about = "Edit a todo title and/or note",
        long_about = EDIT_LONG_ABOUT,
        after_long_help = EDIT_LONG_HELP
    )]
    Edit {
        #[arg(help = "Todo id to edit")]
        todo_id: i64,
        #[arg(
            long,
            help = "Session slug. Defaults to the most recently opened session"
        )]
        session: Option<String>,
        #[arg(long, help = "Replace the todo title")]
        title: Option<String>,
        #[arg(
            long = "note",
            conflicts_with = "clear_note",
            help = "Replace the todo note"
        )]
        note: Option<String>,
        #[arg(long = "clear-note", help = "Remove the todo note")]
        clear_note: bool,
    },
    #[command(about = "Mark a todo as done")]
    Done {
        #[arg(help = "Todo id to mark done")]
        todo_id: i64,
        #[arg(
            long,
            help = "Session slug. Defaults to the most recently opened session"
        )]
        session: Option<String>,
    },
    #[command(about = "Mark a todo as open again")]
    Undone {
        #[arg(help = "Todo id to mark open")]
        todo_id: i64,
        #[arg(
            long,
            help = "Session slug. Defaults to the most recently opened session"
        )]
        session: Option<String>,
    },
    #[command(about = "Open a session in the TUI", long_about = RESUME_LONG_ABOUT)]
    Resume {
        #[arg(help = "Session slug. Defaults to the most recently opened session")]
        session: Option<String>,
        #[arg(long, help = "Open a specific revision read-only")]
        revision: Option<u32>,
    },
    #[command(about = "Export a session snapshot as markdown")]
    Export {
        #[command(subcommand)]
        command: ExportCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    #[command(about = "Create a new session")]
    New {
        #[arg(help = "Human-readable session name")]
        name: String,
        #[arg(long, help = "Override the generated slug")]
        slug: Option<String>,
        #[arg(long, help = "Optional grouping tag stored on the session")]
        tag: Option<String>,
    },
    #[command(about = "Delete a session and all of its data")]
    Delete {
        #[arg(help = "Session slug. Defaults to the most recently opened session")]
        session: Option<String>,
    },
    #[command(
        about = "List sessions with tags, timestamps, and revision numbers",
        long_about = SESSION_LIST_LONG_ABOUT
    )]
    List,
    #[command(
        about = "Print the session revision history",
        long_about = SESSION_HISTORY_LONG_ABOUT
    )]
    History {
        #[arg(help = "Session slug. Defaults to the most recently opened session")]
        session: Option<String>,
    },
    #[command(about = "Set or clear the grouping tag for a session")]
    Tag {
        #[arg(help = "Session slug. Defaults to the most recently opened session")]
        session: Option<String>,
        #[arg(long, conflicts_with = "clear", help = "Assign a new tag value")]
        set: Option<String>,
        #[arg(long, conflicts_with = "set", help = "Remove the current tag")]
        clear: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ExportCommand {
    #[command(
        about = "Render a session snapshot as markdown",
        long_about = EXPORT_MD_LONG_ABOUT,
        after_long_help = EXPORT_MD_LONG_HELP
    )]
    Md {
        #[arg(help = "Session slug. Defaults to the most recently opened session")]
        session: Option<String>,
        #[arg(long, help = "Export a specific revision instead of the live head")]
        revision: Option<u32>,
        #[arg(long, help = "Write output to a file instead of stdout")]
        output: Option<PathBuf>,
        #[arg(long, default_value_t = ExportFormat::Gfm, help = "Markdown flavor to emit")]
        format: ExportFormat,
        #[arg(
            long,
            default_value_t = TimestampMode::Compact,
            help = "Timestamp detail level"
        )]
        timestamps: TimestampMode,
        #[arg(long, help = "Include todo notes in the export")]
        include_notes: bool,
        #[arg(long, help = "Include only open todos in the export")]
        open_only: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum ExportFormat {
    #[default]
    Gfm,
    Plain,
}

impl Display for ExportFormat {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gfm => formatter.write_str("gfm"),
            Self::Plain => formatter.write_str("plain"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum TimestampMode {
    Full,
    #[default]
    Compact,
    None,
}

impl Display for TimestampMode {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full => formatter.write_str("full"),
            Self::Compact => formatter.write_str("compact"),
            Self::None => formatter.write_str("none"),
        }
    }
}

pub fn parse_from<I, T>(args: I) -> Cli
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    Cli::parse_from(args)
}

pub fn run<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = parse_from(args);
    let paths = config::resolve_paths()?;
    let config = config::load(&paths)?;
    let mut database = Database::open(&paths.db_path)?;
    let mut stdout = std::io::stdout().lock();
    execute(&mut database, &config, &mut stdout, cli)
}

pub fn execute<W: Write>(
    database: &mut Database,
    config: &config::Config,
    writer: &mut W,
    cli: Cli,
) -> Result<()> {
    execute_with_runner(database, config, writer, cli, &mut DefaultTuiRunner)
}

fn execute_with_runner<W: Write>(
    database: &mut Database,
    config: &config::Config,
    writer: &mut W,
    cli: Cli,
    runner: &mut impl TuiRunner,
) -> Result<()> {
    match cli.command {
        Some(Command::Session { command }) => handle_session_command(database, writer, command),
        Some(Command::Add {
            title,
            session,
            note,
        }) => {
            let session_slug = database.resolve_session_slug(session.as_deref())?;
            let todo = database.add_todo(
                &session_slug,
                &title,
                note.as_deref().unwrap_or(""),
                now_utc_timestamp(),
            )?;
            writeln!(writer, "{}", todo.id)?;
            Ok(())
        }
        Some(Command::Delete { todo_id, session }) => {
            let todo = database.delete_todo(todo_id, session.as_deref(), now_utc_timestamp())?;
            writeln!(writer, "{}\tdeleted", todo.id)?;
            Ok(())
        }
        Some(Command::Edit {
            todo_id,
            session,
            title,
            note,
            clear_note,
        }) => handle_edit_command(database, writer, todo_id, session, title, note, clear_note),
        Some(Command::Done { todo_id, session }) => {
            let todo = database.set_todo_status(
                todo_id,
                session.as_deref(),
                TodoStatus::Done,
                now_utc_timestamp(),
            )?;
            writeln!(writer, "{}\tdone", todo.id)?;
            Ok(())
        }
        Some(Command::Undone { todo_id, session }) => {
            let todo = database.set_todo_status(
                todo_id,
                session.as_deref(),
                TodoStatus::Open,
                now_utc_timestamp(),
            )?;
            writeln!(writer, "{}\topen", todo.id)?;
            Ok(())
        }
        Some(Command::Resume { session, revision }) => {
            runner.run_session(database, config, session, revision)
        }
        Some(Command::Export { command }) => handle_export_command(database, writer, command),
        None => runner.run_overview(database, config),
    }
}

trait TuiRunner {
    fn run_overview(&mut self, database: &mut Database, config: &config::Config) -> Result<()>;

    fn run_session(
        &mut self,
        database: &mut Database,
        config: &config::Config,
        session: Option<String>,
        revision: Option<u32>,
    ) -> Result<()>;
}

struct DefaultTuiRunner;

impl TuiRunner for DefaultTuiRunner {
    fn run_overview(&mut self, database: &mut Database, config: &config::Config) -> Result<()> {
        tui::overview::run(database, config)
    }

    fn run_session(
        &mut self,
        database: &mut Database,
        config: &config::Config,
        session: Option<String>,
        revision: Option<u32>,
    ) -> Result<()> {
        tui::screen::run(database, config, session, revision)
    }
}

fn handle_session_command<W: Write>(
    database: &mut Database,
    writer: &mut W,
    command: SessionCommand,
) -> Result<()> {
    match command {
        SessionCommand::New { name, slug, tag } => {
            let session = database.create_session(
                &name,
                slug.as_deref(),
                tag.as_deref(),
                now_utc_timestamp(),
            )?;
            writeln!(writer, "{}", session.slug)?;
            Ok(())
        }
        SessionCommand::Delete { session } => {
            let session_slug = database.resolve_session_slug(session.as_deref())?;
            let deleted = database.delete_session(&session_slug)?;
            writeln!(writer, "{}\tdeleted", deleted.slug)?;
            Ok(())
        }
        SessionCommand::List => {
            for session in database.list_sessions()? {
                writeln!(
                    writer,
                    "{}\t{}\t{}\t{}\tr{}",
                    session.slug,
                    session.name,
                    session.tag.as_deref().unwrap_or("-"),
                    format_export_local(session.last_opened_at),
                    session.current_revision
                )?;
            }
            Ok(())
        }
        SessionCommand::History { session } => {
            let session_slug = database.resolve_session_slug(session.as_deref())?;
            for revision in database.list_revisions(&session_slug)? {
                writeln!(
                    writer,
                    "r{}\t{}\t{}\t{}\t{}",
                    revision.revision_number,
                    format_export_local(revision.created_at),
                    revision.reason,
                    revision.todo_count,
                    revision.done_count
                )?;
            }
            Ok(())
        }
        SessionCommand::Tag {
            session,
            set,
            clear,
        } => {
            if set.is_none() && !clear {
                return Err(AppError::InvalidCommandUsage(
                    "session tag requires --set or --clear",
                ));
            }

            let session_slug = database.resolve_session_slug(session.as_deref())?;
            let updated = database.update_session_tag(
                &session_slug,
                if clear { None } else { set.as_deref() },
                now_utc_timestamp(),
            )?;
            writeln!(
                writer,
                "{}\t{}",
                updated.slug,
                updated.tag.as_deref().unwrap_or("-")
            )?;
            Ok(())
        }
    }
}

fn handle_edit_command<W: Write>(
    database: &mut Database,
    writer: &mut W,
    todo_id: i64,
    session: Option<String>,
    title: Option<String>,
    note: Option<String>,
    clear_note: bool,
) -> Result<()> {
    if title.is_none() && note.is_none() && !clear_note {
        return Err(AppError::InvalidCommandUsage(
            "edit requires --title, --note, or --clear-note",
        ));
    }

    let current = database.get_todo(todo_id)?;
    let next_title = title.unwrap_or(current.title);
    let next_notes = if clear_note {
        String::new()
    } else {
        note.unwrap_or(current.notes)
    };
    if next_title.trim().is_empty() {
        return Err(AppError::InvalidCommandUsage("title cannot be empty"));
    }

    let todo = database.update_todo(
        todo_id,
        session.as_deref(),
        &next_title,
        &next_notes,
        now_utc_timestamp(),
    )?;
    writeln!(writer, "{}\tedited", todo.id)?;
    Ok(())
}

#[cfg(test)]
mod dispatch_tests {
    use super::{Cli, Command, DefaultTuiRunner, SessionCommand, execute_with_runner};
    use crate::config::Config;
    use crate::db::Database;
    use crate::error::Result;

    #[derive(Default)]
    struct SpyTuiRunner {
        overview_calls: usize,
        session_calls: Vec<(Option<String>, Option<u32>)>,
    }

    impl super::TuiRunner for SpyTuiRunner {
        fn run_overview(&mut self, _database: &mut Database, _config: &Config) -> Result<()> {
            self.overview_calls += 1;
            Ok(())
        }

        fn run_session(
            &mut self,
            _database: &mut Database,
            _config: &Config,
            session: Option<String>,
            revision: Option<u32>,
        ) -> Result<()> {
            self.session_calls.push((session, revision));
            Ok(())
        }
    }

    #[test]
    fn default_command_dispatches_to_overview() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let mut writer = Vec::new();
        let mut runner = SpyTuiRunner::default();

        execute_with_runner(
            &mut database,
            &Config::default(),
            &mut writer,
            Cli { command: None },
            &mut runner,
        )
        .expect("execute");

        assert_eq!(runner.overview_calls, 1);
        assert!(runner.session_calls.is_empty());
        assert!(writer.is_empty());
    }

    #[test]
    fn resume_command_dispatches_to_session_screen() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let mut writer = Vec::new();
        let mut runner = SpyTuiRunner::default();

        execute_with_runner(
            &mut database,
            &Config::default(),
            &mut writer,
            Cli {
                command: Some(Command::Resume {
                    session: Some(String::from("writing-sprint")),
                    revision: Some(3),
                }),
            },
            &mut runner,
        )
        .expect("execute");

        assert_eq!(runner.overview_calls, 0);
        assert_eq!(
            runner.session_calls,
            vec![(Some(String::from("writing-sprint")), Some(3))]
        );
    }

    #[test]
    fn session_commands_do_not_hit_tui_runner() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let mut writer = Vec::new();
        let mut runner = SpyTuiRunner::default();

        execute_with_runner(
            &mut database,
            &Config::default(),
            &mut writer,
            Cli {
                command: Some(Command::Session {
                    command: SessionCommand::New {
                        name: String::from("Writing Sprint"),
                        slug: None,
                        tag: None,
                    },
                }),
            },
            &mut runner,
        )
        .expect("execute");

        assert_eq!(runner.overview_calls, 0);
        assert!(runner.session_calls.is_empty());
        assert_eq!(String::from_utf8(writer).expect("utf8"), "writing-sprint\n");
    }

    #[test]
    fn default_tui_runner_stays_constructible() {
        let _runner = DefaultTuiRunner;
    }
}

fn handle_export_command<W: Write>(
    database: &mut Database,
    writer: &mut W,
    command: ExportCommand,
) -> Result<()> {
    match command {
        ExportCommand::Md {
            session,
            revision,
            output,
            format,
            timestamps,
            include_notes,
            open_only,
        } => {
            let session_slug = database.resolve_session_slug(session.as_deref())?;
            let snapshot = database.load_snapshot(&session_slug, revision)?;
            let body = markdown::render(
                &snapshot,
                &MarkdownOptions {
                    format,
                    timestamps,
                    include_notes,
                    open_only,
                    exported_at: now_utc_timestamp(),
                },
            );

            if let Some(output_path) = output {
                if let Some(parent) = output_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(output_path, body)?;
            } else {
                write!(writer, "{body}")?;
            }

            Ok(())
        }
    }
}

#[cfg(test)]
mod execute_tests {
    use crate::cli::{Cli, Command, SessionCommand, execute, parse_from};
    use crate::config::Config;
    use crate::db::Database;

    #[test]
    fn creates_session_and_exports_markdown() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let mut output = Vec::new();

        execute(
            &mut database,
            &Config::default(),
            &mut output,
            parse_from(["todui", "session", "new", "Writing Sprint", "--tag", "work"]),
        )
        .expect("create session");
        execute(
            &mut database,
            &Config::default(),
            &mut output,
            parse_from(["todui", "add", "Draft spec", "--session", "writing-sprint"]),
        )
        .expect("add todo");

        let mut export = Vec::new();
        execute(
            &mut database,
            &Config::default(),
            &mut export,
            Cli {
                command: Some(super::Command::Export {
                    command: super::ExportCommand::Md {
                        session: Some(String::from("writing-sprint")),
                        revision: None,
                        output: None,
                        format: super::ExportFormat::Gfm,
                        timestamps: super::TimestampMode::Compact,
                        include_notes: false,
                        open_only: false,
                    },
                }),
            },
        )
        .expect("export");

        let rendered = String::from_utf8(export).expect("utf8");
        assert!(rendered.contains("# Session: writing-sprint"));
        assert!(rendered.contains("- tag: work"));
        assert!(rendered.contains("- [ ] Draft spec"));
    }

    #[test]
    fn edits_todo_from_cli() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let mut output = Vec::new();

        execute(
            &mut database,
            &Config::default(),
            &mut output,
            parse_from(["todui", "session", "new", "Writing Sprint", "--tag", "work"]),
        )
        .expect("create session");
        execute(
            &mut database,
            &Config::default(),
            &mut output,
            parse_from([
                "todui",
                "add",
                "Draft spec",
                "--session",
                "writing-sprint",
                "--note",
                "cover db",
            ]),
        )
        .expect("add todo");

        let mut edit_output = Vec::new();
        execute(
            &mut database,
            &Config::default(),
            &mut edit_output,
            Cli {
                command: Some(Command::Edit {
                    todo_id: 1,
                    session: Some(String::from("writing-sprint")),
                    title: Some(String::from("Draft final spec")),
                    note: None,
                    clear_note: true,
                }),
            },
        )
        .expect("edit todo");

        assert_eq!(String::from_utf8(edit_output).expect("utf8"), "1\tedited\n");
        let todo = database.get_todo(1).expect("todo");
        assert_eq!(todo.title, "Draft final spec");
        assert!(todo.notes.is_empty());
    }

    #[test]
    fn deletes_todo_from_cli() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let mut output = Vec::new();

        execute(
            &mut database,
            &Config::default(),
            &mut output,
            parse_from(["todui", "session", "new", "Writing Sprint"]),
        )
        .expect("create session");
        execute(
            &mut database,
            &Config::default(),
            &mut output,
            parse_from(["todui", "add", "Draft spec", "--session", "writing-sprint"]),
        )
        .expect("add todo");

        let mut delete_output = Vec::new();
        execute(
            &mut database,
            &Config::default(),
            &mut delete_output,
            Cli {
                command: Some(Command::Delete {
                    todo_id: 1,
                    session: Some(String::from("writing-sprint")),
                }),
            },
        )
        .expect("delete todo");

        assert_eq!(
            String::from_utf8(delete_output).expect("utf8"),
            "1\tdeleted\n"
        );
        assert!(database.get_todo(1).is_err());
    }

    #[test]
    fn deletes_session_from_cli() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let mut output = Vec::new();

        execute(
            &mut database,
            &Config::default(),
            &mut output,
            parse_from(["todui", "session", "new", "Writing Sprint"]),
        )
        .expect("create session");

        let mut delete_output = Vec::new();
        execute(
            &mut database,
            &Config::default(),
            &mut delete_output,
            Cli {
                command: Some(Command::Session {
                    command: SessionCommand::Delete {
                        session: Some(String::from("writing-sprint")),
                    },
                }),
            },
        )
        .expect("delete session");

        assert_eq!(
            String::from_utf8(delete_output).expect("utf8"),
            "writing-sprint\tdeleted\n"
        );
        assert!(database.get_session_by_slug("writing-sprint").is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, ExportCommand, ExportFormat, SessionCommand, TimestampMode, parse_from};

    #[test]
    fn parses_session_new_command() {
        let cli = parse_from([
            "todui",
            "session",
            "new",
            "Writing Sprint",
            "--slug",
            "writing",
        ]);

        match cli.command.expect("command") {
            Command::Session {
                command: SessionCommand::New { name, slug, tag },
            } => {
                assert_eq!(name, "Writing Sprint");
                assert_eq!(slug.as_deref(), Some("writing"));
                assert_eq!(tag, None);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_session_new_tag_and_tag_command() {
        let cli = parse_from(["todui", "session", "new", "Writing Sprint", "--tag", "work"]);

        match cli.command.expect("command") {
            Command::Session {
                command: SessionCommand::New { tag, .. },
            } => {
                assert_eq!(tag.as_deref(), Some("work"));
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let cli = parse_from([
            "todui",
            "session",
            "tag",
            "writing-sprint",
            "--set",
            "private",
        ]);

        match cli.command.expect("command") {
            Command::Session {
                command:
                    SessionCommand::Tag {
                        session,
                        set,
                        clear,
                    },
            } => {
                assert_eq!(session.as_deref(), Some("writing-sprint"));
                assert_eq!(set.as_deref(), Some("private"));
                assert!(!clear);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_export_defaults() {
        let cli = parse_from(["todui", "export", "md", "writing-sprint"]);

        match cli.command.expect("command") {
            Command::Export {
                command:
                    ExportCommand::Md {
                        session,
                        format,
                        timestamps,
                        include_notes,
                        open_only,
                        ..
                    },
            } => {
                assert_eq!(session.as_deref(), Some("writing-sprint"));
                assert_eq!(format, ExportFormat::Gfm);
                assert_eq!(timestamps, TimestampMode::Compact);
                assert!(!include_notes);
                assert!(!open_only);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_edit_command() {
        let cli = parse_from([
            "todui",
            "edit",
            "7",
            "--session",
            "writing-sprint",
            "--title",
            "Draft final spec",
            "--note",
            "cover TUI",
        ]);

        match cli.command.expect("command") {
            Command::Edit {
                todo_id,
                session,
                title,
                note,
                clear_note,
            } => {
                assert_eq!(todo_id, 7);
                assert_eq!(session.as_deref(), Some("writing-sprint"));
                assert_eq!(title.as_deref(), Some("Draft final spec"));
                assert_eq!(note.as_deref(), Some("cover TUI"));
                assert!(!clear_note);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_delete_command() {
        let cli = parse_from(["todui", "delete", "7", "--session", "writing-sprint"]);

        match cli.command.expect("command") {
            Command::Delete { todo_id, session } => {
                assert_eq!(todo_id, 7);
                assert_eq!(session.as_deref(), Some("writing-sprint"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_session_delete_command() {
        let cli = parse_from(["todui", "session", "delete", "writing-sprint"]);

        match cli.command.expect("command") {
            Command::Session {
                command: SessionCommand::Delete { session },
            } => {
                assert_eq!(session.as_deref(), Some("writing-sprint"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
