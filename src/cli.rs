use std::ffi::OsString;
use std::fmt::{Display, Formatter};
use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::config;
use crate::db::Database;
use crate::domain::todo::TodoStatus;
use crate::error::Result;
use crate::export::markdown::{self, MarkdownOptions};
use crate::timestamp::{format_export_local, now_utc_timestamp};
use crate::tui;

#[derive(Debug, Parser)]
#[command(
    name = "todui",
    version,
    about = "Terminal todo sessions with revisions"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    Add {
        title: String,
        #[arg(long)]
        session: Option<String>,
        #[arg(long = "note")]
        note: Option<String>,
    },
    Done {
        todo_id: i64,
        #[arg(long)]
        session: Option<String>,
    },
    Undone {
        todo_id: i64,
        #[arg(long)]
        session: Option<String>,
    },
    Resume {
        session: Option<String>,
        #[arg(long)]
        revision: Option<u32>,
    },
    Export {
        #[command(subcommand)]
        command: ExportCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    New {
        name: String,
        #[arg(long)]
        slug: Option<String>,
    },
    List,
    History {
        session: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ExportCommand {
    Md {
        session: Option<String>,
        #[arg(long)]
        revision: Option<u32>,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, default_value_t = ExportFormat::Gfm)]
        format: ExportFormat,
        #[arg(long, default_value_t = TimestampMode::Compact)]
        timestamps: TimestampMode,
        #[arg(long)]
        include_notes: bool,
        #[arg(long)]
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
        SessionCommand::New { name, slug } => {
            let session = database.create_session(&name, slug.as_deref(), now_utc_timestamp())?;
            writeln!(writer, "{}", session.slug)?;
            Ok(())
        }
        SessionCommand::List => {
            for session in database.list_sessions()? {
                writeln!(
                    writer,
                    "{}\t{}\t{}\tr{}",
                    session.slug,
                    session.name,
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
    }
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
    use crate::cli::{Cli, execute, parse_from};
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
        assert!(rendered.contains("- [ ] Draft spec"));
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
                command: SessionCommand::New { name, slug },
            } => {
                assert_eq!(name, "Writing Sprint");
                assert_eq!(slug.as_deref(), Some("writing"));
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
}
