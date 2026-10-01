//! Rust construct views; parser nodes never leave the syntax adapter.
mod block;
mod condition;
mod conditional;
mod let_declaration;
mod loop_expression;
mod match_arm;
mod match_expression;
mod navigation;
mod pattern;

pub(super) use navigation::RustNavigation;

mod call;
mod import;
mod macro_invocation;
mod module;
pub(super) use call::Call;
pub(super) use import::{Import, UseAlias, UseGlob, UseGroup, UseList};
pub(super) use module::{Module, ModuleBody, ModuleOwner, Visibility};

mod header;
pub(super) use header::{Closure, Declaration, Function};
