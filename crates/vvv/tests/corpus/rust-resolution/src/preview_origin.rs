pub struct Engine {
    pub running: bool,
}

impl Engine {
    pub fn new() -> Self {
        Self { running: true }
    }
}
