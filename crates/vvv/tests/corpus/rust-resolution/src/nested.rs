use crate::a as module_alias;
use module_alias::{child::{Child, child_fn}, Foo};

pub fn nested(_: Child, _: Foo) { child_fn(); }
