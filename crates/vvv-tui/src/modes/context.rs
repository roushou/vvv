//! Shared application state lent to a mode transition, without access to other modes.
use crate::model::Status;
pub(crate) struct ModeContext<'a> {
    pub status: &'a mut Status,
}
impl ModeContext<'_> {
    pub fn fail(&mut self, message: &str) -> Vec<crate::action::Effect> {
        self.status.error(message);
        Vec::new()
    }
}
