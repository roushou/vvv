use crate::inline::{Local as Via, make};
pub fn client(_: Via) -> usize { make(crate::Root) }
pub fn qualified(_: crate::inline::nested::Alias) {}
