use crate::origin::Engine;
fn generic<Engine>(value: Engine) -> Engine { value }
fn shadow(value: Engine) {
    let value = value;
    { let value = 1; consume(value); }
    consume(value);
}
fn nested<T>(value: T) { fn inner() { let x: T; } }
fn destructured(value: Engine, (left, right): (u8, u8)) { consume(value); }
