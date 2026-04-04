use crate::config::Config;
use crate::db::Database;
use crate::error::Result;

pub mod browser;
pub mod input;
pub mod layout;
pub mod overview;
pub mod screen;
pub mod terminal;
pub mod theme;
pub mod widgets;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiRoute {
    Overview,
    Session {
        session_name: Option<String>,
        revision: Option<u32>,
    },
}

pub fn run(database: &mut Database, config: &Config, initial_route: TuiRoute) -> Result<()> {
    let mut terminal = terminal::init_terminal()?;
    let result = run_loop(&mut terminal, database, config, initial_route);
    terminal::restore_terminal(&mut terminal)?;
    result
}

fn run_loop(
    terminal: &mut terminal::AppTerminal,
    database: &mut Database,
    config: &Config,
    initial_route: TuiRoute,
) -> Result<()> {
    let mut route = initial_route;
    loop {
        route = match route {
            TuiRoute::Overview => match overview::run_in_terminal(terminal, database, config)? {
                overview::OverviewExit::Quit => break Ok(()),
                overview::OverviewExit::OpenSession(session_name) => TuiRoute::Session {
                    session_name: Some(session_name),
                    revision: None,
                },
            },
            TuiRoute::Session {
                session_name,
                revision,
            } => match screen::run_in_terminal(terminal, database, config, session_name, revision)?
            {
                screen::SessionExit::Quit => break Ok(()),
                screen::SessionExit::Overview => TuiRoute::Overview,
            },
        };
    }
}
