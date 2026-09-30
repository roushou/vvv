pub fn helper() -> usize { 1 }
macro_rules! opaque { ($name:ident) => { let $name = 7; } }
macro_rules! expression { () => { 1 } }
pub fn example(input: usize) {
    let mut before = input;
    helper();
    opaque!(before);
    helper();
    consume(before);
    let mut after = 2;
    consume(after);
    crate::helper();
    { let mut inner = 3; consume(inner); }
    fn nested() { helper(); }
}
fn consume(_: usize) {}
pub fn expression_only(input: usize) {
    let value = expression!();
    helper();
    consume(input);
    consume(value);
}
#[cfg(feature = "one")]
pub fn competing() {}
#[cfg(feature = "two")]
pub fn competing() {}
pub fn ambiguous_expression() { let _value = expression!(); competing(); }
pub fn multiple(input: usize) {
    opaque!(input);
    let mut input = 2;
    consume(input);
    opaque!(input);
    consume(input);
}
macro_rules! assert_eq { ($name:ident) => { let $name = 9; } }
pub fn custom_assert(input: usize) {
    assert_eq!(input);
    consume(input);
}
pub fn inner_competing(input: usize) {
    opaque!(input);
    {
        #[cfg(feature = "one")]
        fn choice() {}
        #[cfg(feature = "two")]
        fn choice() {}
        choice();
    }
}
pub fn identifier_pattern(input: ()) {
    opaque!(input);
    let value = ();
    consume_unit(value);
}
fn consume_unit(_: ()) {}
pub fn same_block() {
    let mut local = 1;
    consume(local);
    opaque!(local);
}
pub fn outer_local() {
    let mut local = 1;
    { consume(local); opaque!(local); }
}
pub fn inner_parameter(mut input: usize) {
    opaque!(input);
    fn inner(mut input: usize) { consume(input); }
}
