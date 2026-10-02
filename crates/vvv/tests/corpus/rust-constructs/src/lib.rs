fn work(_: usize) {}
fn scopes(value: usize, input: Option<usize>, items: Vec<usize>) {
    match input {
        Some(value) if value > 0 => {
            work(value);
            let capture = || work(value);
            fn nested() { work(value); }
        }
        _ => work(value),
    }
    work(value);
    for value in items {
        work(value);
    }
    work(value);
    while let Some(value) = input {
        work(value);
    }
    work(value);
    while value > 0 { work(value); }
    loop { work(value); break; }
    let callback = |value: usize| work(value);
    work(value);
    match input {
        Some(Point { field }) => work(value),
        _ => work(value),
    }
    work(value);
}
fn ordered(value: usize, input: Option<usize>) {
    for value in source(value) { work(value); }
    while let Some(value) = input && value > 0 && let Some(value) = next(value) {
        work(value);
    }
    match input {
        Some(value) if value > 0 => work(value),
        _ => work(value),
    }
    work(value);
    for (prefix, Point { field }) in input { work(value); }
    work(value);
    match input {
        Some(mut value) => { unknown!(); work(value); },
        _ => work(value),
    }
}
