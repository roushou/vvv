//! The composition root: which language plugins this build ships.
//!
//! The one place outside `vvv-lang` that names a grammar. It chooses the
//! registry the binary and its tests hand to `Engine::new`, and does nothing
//! else with the languages.

use vvv_engine::Languages;

/// The languages compiled into this build, chosen by Cargo feature.
pub struct Builtins;

impl Builtins {
    /// Every language whose feature is on, in a fixed order.
    pub fn registry() -> Languages {
        let languages = Languages::new();
        #[cfg(feature = "rust")]
        let languages = languages.with(vvv_lang::rust::Rust::new());
        #[cfg(feature = "typescript")]
        let languages = languages
            .with(vvv_lang::typescript::TypeScript::new())
            .with(vvv_lang::typescript::Tsx::new());
        languages
    }
}
