use std::io::{Stdout, Write, stdout};

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::error::Result;

pub type AppTerminal = Terminal<CrosstermBackend<Stdout>>;

pub fn init_terminal() -> Result<AppTerminal> {
    enable_raw_mode()?;
    let mut handle = stdout();
    execute!(
        handle,
        EnterAlternateScreen,
        EnableMouseCapture,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    )?;
    Ok(Terminal::new(CrosstermBackend::new(handle))?)
}

pub fn restore_terminal(terminal: &mut AppTerminal) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        PopKeyboardEnhancementFlags
    )?;
    terminal.show_cursor()?;
    Ok(())
}

pub fn ring_terminal(terminal: &mut AppTerminal) -> Result<()> {
    terminal.backend_mut().write_all(b"\x07")?;
    terminal.backend_mut().flush()?;
    Ok(())
}
