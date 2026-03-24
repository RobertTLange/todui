use crate::action::Action;
use crate::app::AppState;
use crate::error::Result;

#[derive(Debug, Default)]
pub struct Reducer;

impl Reducer {
    pub fn dispatch(&mut self, _state: &mut AppState, _action: Action) -> Result<()> {
        Ok(())
    }
}
