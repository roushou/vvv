use crate::Engine;

pub struct App {
    pub engine: Engine,
}

pub enum Error {
    Engine,
}

pub fn error() -> Error {
    Error::Engine
}

pub fn roundtrip(engine: Engine) -> Engine {
    engine
}
