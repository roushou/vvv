use crate::a as alias;

fn consume(_: alias::Foo) {}

fn nested(_: alias::sub::Nested) {}
