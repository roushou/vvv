use crate::bridge::task as execute;
use crate::origin::other;
pub fn caller() {
    execute();
    crate::origin::work();
    let callback = execute;
    callback();
    other();
    let deferred = || execute();
    fn nested() { crate::origin::work(); }
}
pub fn uncertain(receiver: Unknown) { receiver.work(); }
pub fn local_import() {
    use crate::origin::work as local_work;
    local_work();
}
mod helpers {
    use crate::origin::work as scoped_work;
    pub fn aliased() { scoped_work(); }
}
