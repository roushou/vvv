mod a {
    pub fn run() {}
    pub struct Data;
}
mod b { pub fn run() {} }
fn hoisted() {
    work();
    use crate::a::run as work;
    work();
}
fn chained() {
    use crate::a as tools;
    use tools::run as work;
    work();
    tools::run();
}
fn nested() {
    use crate::a::run as work;
    work();
    {
        use crate::b::run as work;
        work();
    }
    work();
}
fn parameter(work: fn()) {
    use crate::a::run as work;
    work();
}
fn locals() {
    use crate::a::run as work;
    let work = work;
    work();
}
fn competition() {
    use crate::a::run as work;
    fn work() {}
    work();
}
fn duplicate() {
    use crate::a::run as work;
    use crate::b::run as work;
    work();
}
fn capture() {
    use crate::a::run as work;
    fn inner() { work(); }
    inner();
}
fn types() {
    use crate::a::Data as Item;
    let _: Item;
}
fn macro_block() {
    use crate::a::run as work;
    unknown!();
    work();
    crate::a::run();
}
fn glob() {
    use crate::a::*;
    run();
}
fn cycle() {
    use second as first;
    use first as second;
    first();
}
fn external() {
    use std::mem::drop as discard;
    discard(1);
}
mod inline {
    fn local() {}
    fn call() {
        use self::local as work;
        use super::a::run as outer;
        work();
        outer();
    }
}
fn isolated() { work(); }
const UNIT: () = ();
fn constant_pattern() {
    use crate::UNIT;
    let UNIT = ();
    let _ = UNIT;
}
type OnlyType = ();
fn namespaces(value: fn()) {
    use crate::OnlyType as value;
    value();
}
fn grouped() {
    use crate::a::{run as work, Data as Item};
    work();
    let _: Item;
}
fn closure() {
    use crate::a::run as work;
    let _ = || work();
}
fn grouped_alias() {
    use crate::a as tools;
    use tools::{run as work, Data as Item};
    work();
    let _: Item;
}
fn import_owner() {
    use crate::a as tools;
    use tools::run as work;
    {
        use crate::b as tools;
        work();
    }
}

fn let_else(value: Option<usize>) {
    use crate::a::run as work;
    let Some(value) = value else {
        let _ = value;
        work();
        return;
    };
    let _ = value;
    work();
}
