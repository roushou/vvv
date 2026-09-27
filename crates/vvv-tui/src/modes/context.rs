//! Shared application state lent to a mode transition, without access to other modes.
use crate::model::Status;
pub(crate) struct ModeContext<'a> {
    pub status: &'a mut Status,
    pub generation: &'a mut u64,
}
impl ModeContext<'_> {
    pub fn next_generation(&mut self) -> u64 {
        *self.generation += 1;
        *self.generation
    }
    pub fn fail(&mut self, message: &str) -> Vec<crate::action::Effect> {
        self.status.error(message);
        Vec::new()
    }
}
