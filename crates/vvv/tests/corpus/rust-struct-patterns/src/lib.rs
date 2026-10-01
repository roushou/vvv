struct Point { x: usize, y: usize }
fn work(_: usize) {}
fn scopes(x: usize, point: Point, input: Option<Point>, items: Vec<Point>) {
    let Point { x, y: renamed } = point;
    work(x);
    work(renamed);
    if let Some(Point { x, ref y }) = input {
        work(x);
        let _ = y;
    } else { work(x); }
    work(x);
    match input {
        Some(Point { x: arm, y: ref other }) => { work(arm); let _ = other; }
        _ => work(x),
    }
    for Point { x: item, .. } in items { work(item); }
    while let Some(Point { x: next, .. }) = input { work(next); }
    let closure = |Point { x: captured, .. }: Point| work(captured);
    let Some(Point { x: after, .. }) = input else { work(x); return; };
    work(after);
}

fn parameter(Point { x: value, .. }: Point) { work(value); }
fn unsupported(x: usize, input: Option<Point>) {
    if let Some(Point { x: prefix, y: other @ _ }) = input { work(x); }
    work(x);
}

fn duplicate(point: Point) {
    let Point { x: value, y: value } = point;
    work(value);
}
