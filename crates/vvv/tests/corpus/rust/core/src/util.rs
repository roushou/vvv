use std::collections::HashMap;

pub fn area(side: u32) -> u32 {
    side * side
}

pub(crate) fn helper() -> u32 {
    area(1)
}

fn unused_private() -> u32 {
    helper()
}

pub const VERSION: u32 = 1;
