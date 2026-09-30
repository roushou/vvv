pub(crate) use crate::a as package_alias;
pub(super) use crate::a as parent_alias;
pub(in crate::restricted) use crate::a as limited;
pub mod nested;
fn hidden() { use crate::b as local_only; }
mod inline { use crate::b as inline_only; }
