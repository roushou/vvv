mod values {
    pub const MIN: usize = 1;
    pub const MAX: usize = 9;
    pub struct Pair(pub usize);
    pub struct Point { pub x: usize }
    pub enum Message { Quit, Data(usize), Record { x: usize } }
}
use values::{MIN, MAX, Pair as Wrap, Point, Message};
use values::Message::{Quit, Data as Payload};
fn work(_: usize) {}
fn check(input: usize, MIN: usize, outer: usize) {
    match input {
        MIN..=MAX => work(outer),
        MIN | MAX => work(outer),
        Quit | Message::Quit => work(outer),
        Wrap(value) => work(value),
        Point { x: value } => work(value),
        Payload(value) | Message::Data(value) => work(value),
        Message::Record { x: value } => work(value),
        MIN | value => work(outer),
        _ => work(MIN),
    }
}

fn local(input: usize) {
    use crate::values::MAX as END;
    match input { 0..=END => work(input), _ => {} }
}

fn uncertain(input: usize) {
    unknown!();
    match input { MIN | MAX => work(input), crate::values::MIN..=crate::values::MAX => work(input), _ => {} }
}
mod limits;
mod bridge;
use bridge::{LOW, HIGH, Packet};
fn relocated(input: usize) {
    match input { LOW..=HIGH => work(input), Packet(value) => work(value), _ => {} }
}

fn spaced(input: usize) {
    match input { crate /* anchor */ :: values :: MIN..=values :: MAX => work(input), _ => {} }
}
