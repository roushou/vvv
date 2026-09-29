use crate::bridge::Runtime;
use crate::origin::Engine;

pub struct App {
    engine: Engine,
    runtime: Runtime,
}

pub fn roundtrip(engine: Engine) -> Engine { engine }
pub fn generic<Engine>(engine: Engine) {}
pub fn local() { type Engine = u8; let value: Engine = 0; }
pub fn unicode(é: u8, engine: Engine) {}

impl App { pub fn method(engine: Engine) -> Engine { engine } }
