#[cfg(one)]
fn work() {}
#[cfg(two)]
fn work() {}
fn caller() { work(); }
