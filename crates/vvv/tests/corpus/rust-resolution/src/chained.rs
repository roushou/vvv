use crate as root;
use root::a as parent;
use parent::child as leaf;

pub fn via_parent(_: parent::Foo) {}
pub fn via_child(_: leaf::Child) {}
