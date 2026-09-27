use crate::a as module_alias;
use crate as parent_alias;
use module_alias::{Foo, child::Child};

pub fn module_path(_: module_alias::Foo) {}
pub fn parent_path(_: parent_alias::a::child::Child) {}
pub fn grouped_path(_: Foo, _: Child) {}
pub fn self_path() { self::module_path(module_alias::Foo); }
