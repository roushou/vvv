pub struct Root;
pub fn root() -> usize { 1 }
pub mod inline {
    use super::{Root as ImportedRoot, root as imported};
    pub struct Local;
    pub fn make(_: ImportedRoot) -> usize { imported() }
    pub mod nested {
        use super::{Local as Alias, make};
        pub fn call(_: Alias) -> usize { make(crate::Root) }
    }
    mod hidden { pub struct Secret; }
    pub fn inside(_: hidden::Secret) {}
    pub mod competing {
        #[cfg(unix)] pub struct Same;
        #[cfg(windows)] pub struct Same;
        pub fn choose(_: Same) {}
    }
    pub mod cycle_a { pub use super::cycle_b::alias as alias; }
    pub mod cycle_b { pub use super::cycle_a::alias as alias; }
    pub fn cyclic(_: cycle_a::alias) {}
}
pub use inline::Local as Exported;
pub mod disk;
pub fn outside(_: inline::Local, _: Exported) {}
fn denied(_: inline::hidden::Secret) {}
#[cfg(test)]
mod tests {
    use super::*;
    struct Root;
    fn helper() -> usize { 2 }
    fn exercise(_: Root) { helper(); }
    mod deeper {
        use super::Root as Parent;
        fn exercise(_: Parent) {}
    }
    fn noncapturing() {
        let value = 1;
        fn inner() -> usize { value }
    }
    fn local_import() {
        use crate::Root as Imported;
        fn inner(_: Imported) {}
    }
}
pub mod visibility {
    pub(crate) struct Package;
    pub(super) struct Parent;
    pub(in crate::visibility) struct Limited;
    pub mod nested { pub fn allowed(_: super::Limited) {} }
}
pub fn allowed(_: visibility::Package, _: visibility::Parent) {}
pub fn denied_restriction(_: visibility::Limited) {}
