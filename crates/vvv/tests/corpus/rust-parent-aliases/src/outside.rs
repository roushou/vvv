use crate::parent::parent as denied;
use crate::restricted::package_alias as package;
use crate::restricted::parent_alias as parent;
use crate::restricted::limited as forbidden;
use crate::restricted::local_only as false_scope;
use crate::restricted::inline_only as false_inline;
pub fn private(_: denied::Foo) {}
pub fn package_visible(_: package::Foo) {}
pub fn parent_visible(_: parent::Foo) {}
pub fn restricted(_: forbidden::Foo) {}
pub fn function_local(_: false_scope::Foo) {}
pub fn inline_local(_: false_inline::Foo) {}
