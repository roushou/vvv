use crate as root;
use root::{a::sub::Nested, a::Foo};

fn nested(_: Nested) {}
fn foo(_: Foo) {}
