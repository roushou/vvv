//! Relative-specifier layout (`./a/b`, `../c`).
//!
//! A file's address is its workspace-relative path. Bare specifiers
//! (`react`, `@scope/pkg`) and `tsconfig` path aliases are left alone. Pure
//! functions of the [`Project`]: extension and `index` probing ask the
//! project's file list, never the disk.

use std::path::{Component, Path, PathBuf};

use vvv_core::{
    Address, Layout, ModulePath, Name, Package, PackageId, PathHead, PathSyntax, Project,
    ResolveError,
};

/// How TypeScript spells specifiers.
pub(crate) const SYNTAX: PathSyntax = PathSyntax::Posix;

const EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "d.ts"];
const TRY_EXTENSIONS: &[&str] = &["ts", "tsx", "d.ts", "js", "jsx", "mts", "cts"];
const INDEX: &[&str] = &["index.ts", "index.tsx", "index.js"];

#[derive(Debug, Clone, Copy, Default)]
pub struct TsLayout;

impl TsLayout {
    /// Addresses are workspace-relative paths under one package: relative
    /// specifiers cross package directories freely, so a monorepo's
    /// `package.json` names add nothing until `paths` aliases are resolved.
    const PACKAGE: &str = "";

    /// Collapse `.` and `..` without touching the file system.
    fn normalize(path: &Path) -> PathBuf {
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    out.pop();
                }
                other => out.push(other),
            }
        }
        out
    }

    /// `target` spelled relative to `dir`, always starting with `./` or `../`.
    pub(crate) fn relative(dir: &Path, target: &Path) -> ModulePath {
        let dir: Vec<_> = dir.components().collect();
        let target: Vec<_> = target.components().collect();
        let common = dir.iter().zip(&target).take_while(|(a, b)| a == b).count();
        let ups = dir.len() - common;
        let head = if ups == 0 {
            PathHead::Here
        } else {
            PathHead::Up(ups as u8)
        };
        ModulePath::new(
            SYNTAX,
            head,
            target[common..]
                .iter()
                .map(|c| Name::new(c.as_os_str().to_string_lossy())),
        )
    }

    pub(crate) fn known_extension(name: &str) -> Option<&'static str> {
        EXTENSIONS
            .iter()
            .copied()
            .filter(|ext| name.ends_with(&format!(".{ext}")))
            .max_by_key(|ext| ext.len())
    }

    fn address_of(path: &Path) -> Address {
        Address::new(
            PackageId::new(Self::PACKAGE),
            path.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned()),
        )
    }

    pub(crate) fn path_of(address: &Address) -> PathBuf {
        address.path().iter().map(Name::as_str).collect()
    }
}

impl Layout for TsLayout {
    fn manifests(&self) -> &'static [&'static str] {
        &[]
    }

    fn package(&self, _: &Path, _: &str) -> Option<Package> {
        None
    }

    fn address(&self, _: &Project, path: &Path) -> Result<Address, ResolveError> {
        let name = path.file_name().map(|n| n.to_string_lossy());
        let is_dir = path.extension().is_none();
        match name.as_deref().and_then(Self::known_extension) {
            Some(_) => Ok(Self::address_of(path)),
            None if is_dir => Ok(Self::address_of(path)),
            None => Err(ResolveError::NotAddressable(path.into())),
        }
    }

    fn resolve(&self, project: &Project, file: &Path, import: &ModulePath) -> Option<Address> {
        // Bare specifiers (`react`, `@scope/pkg`) and absolute paths are
        // not files of this tree.
        let ups = match import.head {
            PathHead::Here => 0,
            PathHead::Up(n) => n,
            PathHead::Named | PathHead::Package | PathHead::SelfType | PathHead::Root => {
                return None;
            }
        };
        let mut base = file.parent()?.to_path_buf();
        for _ in 0..ups {
            base.push("..");
        }
        base.extend(import.segments.iter().map(Name::as_str));
        let base = Self::normalize(&base);
        let name = base.file_name()?.to_string_lossy().into_owned();
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(ext) = Self::known_extension(&name) {
            candidates.push(base.clone());
            // ESM style: `./x.js` written for a `./x.ts` source.
            if let Some(stem) = name.strip_suffix(&format!(".{ext}")) {
                let swapped = match ext {
                    "js" => Some("ts"),
                    "jsx" => Some("tsx"),
                    "mjs" => Some("mts"),
                    "cjs" => Some("cts"),
                    _ => None,
                };
                if let Some(swapped) = swapped {
                    candidates.push(base.with_file_name(format!("{stem}.{swapped}")));
                }
            }
        } else {
            candidates.extend(
                TRY_EXTENSIONS
                    .iter()
                    .map(|ext| base.with_file_name(format!("{name}.{ext}"))),
            );
            candidates.extend(INDEX.iter().map(|index| base.join(index)));
        }
        candidates
            .into_iter()
            .find(|c| project.files.contains(c))
            .map(|c| Self::address_of(&c))
    }

    fn candidates(&self, _: &Project, address: &Address) -> Vec<PathBuf> {
        vec![Self::path_of(address)]
    }
}
