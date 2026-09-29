/// Build an array without including implementation context.
#[inline]
pub fn build<const N: usize>(input: [u8; { 2 }]) -> [u8; N]
where
    [u8; N]: Sized,
{
    let _ = input;
    [0; N]
}
