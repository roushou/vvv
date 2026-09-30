fn work() {}
fn caller() {
    use crate::a::run as work;
    work();
}
