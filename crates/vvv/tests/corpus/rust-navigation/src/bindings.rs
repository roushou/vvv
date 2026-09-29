fn pairs((left, right): (u8, u8)) { consume(left); consume(right); }
fn closure(value: u8) { let f = |value| value; consume(value); }
fn destructure(pair: (u8, u8)) { let (left, right) = pair; consume(right); }
use crate::origin::Engine;
fn annotated() { let engine: Engine; }
