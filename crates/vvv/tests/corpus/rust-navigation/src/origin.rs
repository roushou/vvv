/// Runtime state.
pub struct Engine {
    pub running: bool,
}
pub enum Error {
    Engine,
    Io,
}
