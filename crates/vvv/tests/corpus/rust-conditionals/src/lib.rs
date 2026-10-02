mod api {
    pub fn work() {}
    pub struct Data;
    pub const UNIT: () = ();
}
fn branches(value: Option<usize>) {
    if let Some(value) = value {
        let _ = value;
    } else {
        let _ = value;
    }
    let _ = value;
}
fn chain(value: usize, input: Option<usize>) {
    if let Some(value) = input
        && value > 0
        && let Some(value) = Some(value + 1)
        && value > 1
    {
        let _ = value;
    } else {
        let _ = value;
    }
    let _ = value;
}
fn imports(input: Option<usize>, ready: bool) {
    use crate::api::work;
    if ready {
        work();
        let _: crate::api::Data;
    } else {
        work();
    }
    if let Some(value) = input {
        work();
        let _ = value;
        let _ = || value;
        fn nested() { let _ = value; }
    }
}
fn inner_import(input: Option<fn()>) {
    if let Some(work) = input {
        work();
        {
            use crate::api::work;
            work();
        }
    }
}
fn alternatives(first: Option<usize>, second: Option<usize>, value: usize) {
    if let Some(value) = first {
        let _ = value;
    } else if let Some(value) = second {
        let _ = value;
    } else {
        let _ = value;
    }
}
fn unsupported(input: Option<Point>, value: usize) {
    if let Some(Point { value }) = input {
        let _ = value;
    } else {
        let _ = value;
    }
    let _ = value;
}
struct Point { value: usize }
fn macro_branch(input: Option<usize>) {
    if let Some(mut value) = input {
        unknown!();
        let _ = value;
        let mut inner = 1;
        let _ = inner;
        crate::api::work();
    }
}
fn constant_pattern(input: Option<()>) {
    use crate::api::UNIT;
    if let Some(UNIT) = input {
        let _ = UNIT;
    }
}
fn loop_barrier(input: Option<usize>) {
    if let Some(value) = input {
        while true { let _ = value; }
    }
}

const VALUE: () = ();
fn module_constant(input: Option<()>) {
    if let Some(VALUE) = input {
        let _ = VALUE;
    }
}
use crate::api::UNIT as IMPORTED;
fn module_imported_constant(input: Option<()>) {
    if let Some(IMPORTED) = input {
        let _ = IMPORTED;
    }
}
fn ambiguity(ready: bool) {
    if ready {
        use crate::api::work;
        fn work() {}
        work();
    }
}
fn pattern_competition(input: (usize, usize)) {
    if let (value, value) = input {
        let _ = value;
    }
}
