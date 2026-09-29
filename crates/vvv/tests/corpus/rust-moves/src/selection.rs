/// Selected declaration carries its documentation.
#[derive(Clone)]
pub struct Selected<T>(pub T);
impl<T> Selected<T> { pub fn value(&self) -> &T { &self.0 } }
impl<T: Default> Default for Selected<T> { fn default() -> Self { Self(T::default()) } }
mod nested { pub struct Selected; impl Selected {} }

#[cfg(unix)]
pub fn conditional() {}
#[cfg(windows)]
pub fn conditional() {}

pub struct Qualified;
impl crate::selection::Qualified {}
