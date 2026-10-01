fn work(_: usize) {}
fn patterns(input: Option<usize>, items: &[usize], outer: usize) {
    match input {
        Some(value) | Other(value) => work(value),
        whole @ Some(inner) => { work(inner); let _ = whole; }
        Some(0 | 1..=9) => work(outer),
        Some(left) | Other(right) => work(left),
        _ => work(0),
    }
    let [first, tail @ .., last] = items else { return; };
    work(first); let _ = tail; work(last);
    if let Some(value @ 0..=9) = input { work(value); }
    work(value);
}

fn ranges(LIMIT: usize, input: usize) {
    match input { value @ 0..=LIMIT => work(value), _ => work(LIMIT) }
}
