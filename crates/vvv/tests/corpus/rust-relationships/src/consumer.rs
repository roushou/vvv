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
