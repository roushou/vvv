//! How to spell an import in TypeScript: a relative specifier from one file
//! to another, keeping the original's extension style. Moving a file changes
//! nothing else, so there is no relocation.

use std::path::Path;

use vvv_core::{Address, ModulePath, Name, PathHead, Project, Surgery};

use super::layout::{SYNTAX, TsLayout};

#[derive(Debug, Clone, Copy, Default)]
pub struct TsSurgery;

impl Surgery for TsSurgery {
    fn import_statement(
        &self,
        project: &Project,
        from: &Path,
        target: &Address,
        name: &str,
    ) -> Option<String> {
        let bare = ModulePath::new(SYNTAX, PathHead::Here, std::iter::empty::<&str>());
        Some(format!(
            "import {{ {name} }} from '{}';",
            self.render(project, from, target, &bare)
        ))
    }

    fn render(
        &self,
        _: &Project,
        file: &Path,
        target: &Address,
        original: &ModulePath,
    ) -> ModulePath {
        let target = TsLayout::path_of(target);
        let dir = file.parent().unwrap_or(Path::new(""));
        let original_name = original.last().map_or("", Name::as_str);
        let target_name = target.file_name().map(|n| n.to_string_lossy().into_owned());
        let target_ext = target_name.as_deref().and_then(TsLayout::known_extension);

        let spelled = match (TsLayout::known_extension(original_name), target_ext) {
            // Original had an extension: keep that spelling (`.js` for `.ts`).
            (Some(orig_ext), Some(ext)) => {
                let stem = target.with_file_name(
                    target_name
                        .as_deref()
                        .and_then(|n| n.strip_suffix(&format!(".{ext}")))
                        .unwrap_or_default(),
                );
                stem.with_file_name(format!(
                    "{}.{orig_ext}",
                    stem.file_name().unwrap_or_default().to_string_lossy()
                ))
            }
            (None, Some(ext)) => {
                let stem = target_name
                    .as_deref()
                    .and_then(|n| n.strip_suffix(&format!(".{ext}")))
                    .unwrap_or_default();
                let without_ext = target.with_file_name(stem);
                let original_is_index = original_name == "index";
                if stem == "index" && !original_is_index {
                    without_ext
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or(without_ext)
                } else {
                    without_ext
                }
            }
            _ => target,
        };
        TsLayout::relative(dir, &spelled)
    }
}
