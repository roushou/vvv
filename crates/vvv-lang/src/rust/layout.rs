//! Cargo layout: how Rust files and paths map onto crates and modules.
//!
//! A file's address is its crate — named as `use` paths name it, the lib
//! name or the package name with `-` as `_` — and its place under that
//! crate's `src/` (`src/a/b.rs` and `src/a/b/mod.rs` are both `a::b`). Import
//! paths starting with `crate`, `self` or `super` resolve against the file's
//! own crate; a path starting with another crate's name resolves into that
//! crate — a workspace member, possibly renamed, or an external dependency;
//! anything else (in-scope names) is left alone. Pure functions of the
//! [`Project`]: nothing here touches a file system.

use std::path::{Path, PathBuf};

use vvv_core::{
    Address, Dependency, Layout, ModulePath, Name, Package, PackageId, PathHead, PathSyntax,
    Project, ResolveError,
};

/// How Rust spells paths.
pub(crate) const SYNTAX: PathSyntax = PathSyntax::Scoped;
const MANIFEST: &str = "Cargo.toml";
const NOT_MODULES: &[&str] = &["bin", "tests", "examples", "benches"];
/// Crates every Rust file can name without a manifest saying so.
const SYSROOT: &[&str] = &["std", "core", "alloc"];

#[derive(Debug, Clone, Copy, Default)]
pub struct RustLayout;

/// A `.rs` file placed in its crate.
pub(crate) struct Placed {
    /// `<crate dir>/src`, workspace-relative.
    pub(crate) src: PathBuf,
    pub(crate) module: Address,
}

/// The two files a module move edits, and the names involved.
pub(crate) struct MoveSites {
    pub(crate) old_parent: PathBuf,
    pub(crate) new_parent: PathBuf,
    pub(crate) old_name: String,
    pub(crate) new_name: String,
}

impl RustLayout {
    /// The crate containing `path`, with its `src/` directory.
    fn crate_of(project: &Project, path: &Path) -> Option<(PackageId, PathBuf)> {
        let package = project.packages.containing(path)?;
        Some((package.id.clone(), package.root.join("src")))
    }

    /// A `.rs` file, or a directory (no extension) standing for the module
    /// it holds: `src/a/b.rs`, `src/a/b/mod.rs` and `src/a/b/` are all
    /// `crate::a::b`.
    pub(crate) fn place(project: &Project, path: &Path) -> Result<Placed, ResolveError> {
        let not_addressable = || ResolveError::NotAddressable(path.into());
        if path.extension().is_some_and(|e| e != "rs") {
            return Err(not_addressable());
        }
        let (package, src) = Self::crate_of(project, path).ok_or_else(not_addressable)?;
        let rel = path.strip_prefix(&src).map_err(|_| not_addressable())?;
        let mut parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if parts
            .first()
            .is_some_and(|p| NOT_MODULES.contains(&p.as_str()))
        {
            return Err(not_addressable());
        }
        let last = parts.pop().ok_or_else(not_addressable)?;
        match last.as_str() {
            "lib.rs" | "main.rs" if parts.is_empty() => {}
            "mod.rs" => {}
            other => parts.push(other.trim_end_matches(".rs").to_owned()),
        }
        if parts.is_empty() && path.extension().is_none() {
            // `src` itself: the crate root directory, not a module.
            return Err(not_addressable());
        }
        Ok(Placed {
            src,
            module: Address::new(package, parts),
        })
    }

    /// Where a module may be declared: `src/a/b.rs` before `src/a/b/mod.rs`;
    /// `src/lib.rs` before `src/main.rs` for the crate root.
    pub(crate) fn candidates_for(src: &Path, module: &Address) -> Vec<PathBuf> {
        if module.is_root() {
            return vec![src.join("lib.rs"), src.join("main.rs")];
        }
        let mut dir = src.to_path_buf();
        for seg in module.path() {
            dir.push(seg.as_str());
        }
        vec![dir.with_extension("rs"), dir.join("mod.rs")]
    }

    /// The file declaring `module`, if the project has it.
    pub(crate) fn file_for(project: &Project, src: &Path, module: &Address) -> Option<PathBuf> {
        Self::candidates_for(src, module)
            .into_iter()
            .find(|p| project.files.contains(p))
    }

    /// The other half of a module: `a/` for `a.rs`, `a.rs` for `a/`.
    fn other_half(project: &Project, path: &Path) -> Option<PathBuf> {
        if path.extension().is_some() {
            let dir = path.with_extension("");
            project.files.has_under(&dir).then_some(dir)
        } else {
            let file = path.with_extension("rs");
            project.files.contains(&file).then_some(file)
        }
    }

    pub(crate) fn stem(path: &Path) -> Result<String, ResolveError> {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .ok_or_else(|| ResolveError::NotAddressable(path.into()))
    }

    /// `crate::a::b`: an absolute path to `target` within its own crate.
    pub(crate) fn from_crate_root(target: &Address) -> ModulePath {
        ModulePath::new(SYNTAX, PathHead::Package, target.path().iter().cloned())
    }

    /// `name::a::b`: an absolute path to `target` in the crate called `name`.
    pub(crate) fn from_crate_named(name: &PackageId, target: &Address) -> ModulePath {
        ModulePath::new(
            SYNTAX,
            PathHead::Named,
            std::iter::once(Name::new(name.as_str())).chain(target.path().iter().cloned()),
        )
    }

    /// Everything about a module move that paths alone decide: the file's
    /// role, the crate, and both parents. What remains — the `mod` line
    /// itself — needs the parents' facts and is the surgery's.
    pub(crate) fn move_sites(
        project: &Project,
        from: &Path,
        to: &Path,
    ) -> Result<MoveSites, ResolveError> {
        let src_from = Self::place(project, from)?;
        let src_to = Self::place(project, to)?;
        let from_name = from.file_name().map(|n| n.to_string_lossy());
        if matches!(from_name.as_deref(), Some("lib.rs" | "main.rs" | "mod.rs")) {
            return Err(ResolveError::Root(from.into()));
        }
        if src_from.src != src_to.src {
            return Err(ResolveError::CrossProject {
                from: from.into(),
                to: to.into(),
            });
        }
        let old_name = Self::stem(from)?;
        let new_name = Self::stem(to)?;
        let old_parent = src_from
            .module
            .parent()
            .and_then(|m| Self::file_for(project, &src_from.src, &m))
            .ok_or_else(|| ResolveError::NoDeclaringFile {
                name: format!("mod {old_name};"),
            })?;
        let new_parent_module = src_to
            .module
            .parent()
            .unwrap_or_else(|| Address::root(src_to.module.package().clone()));
        let new_parent =
            Self::file_for(project, &src_to.src, &new_parent_module).ok_or_else(|| {
                ResolveError::NoParentFile {
                    module: Self::from_crate_root(&new_parent_module).to_string(),
                    candidates: Self::candidates_for(&src_to.src, &new_parent_module)
                        .into_iter()
                        .map(Into::into)
                        .collect(),
                    declaration: format!("mod {new_name};"),
                }
            })?;
        Ok(MoveSites {
            old_parent,
            new_parent,
            old_name,
            new_name,
        })
    }
}

impl Layout for RustLayout {
    fn manifests(&self) -> &'static [&'static str] {
        &[MANIFEST]
    }

    /// A manifest's crate, named as `use` paths name it — `[lib] name`, else
    /// `[package] name` with `-` as `_` — and every crate it depends on under
    /// the name it is used as. `None` for a workspace-only manifest.
    fn package(&self, manifest: &Path, text: &str) -> Option<Package> {
        let table: toml::Table = text.parse().ok()?;
        let name = |section: &str| table.get(section)?.get("name")?.as_str().map(str::to_owned);
        let package = name("package")?;
        let id = PackageId::new(name("lib").unwrap_or_else(|| package.replace('-', "_")));
        let mut dependencies: Vec<Dependency> =
            ["dependencies", "dev-dependencies", "build-dependencies"]
                .iter()
                .filter_map(|section| table.get(*section)?.as_table())
                .chain(
                    table
                        .get("target")
                        .and_then(toml::Value::as_table)
                        .into_iter()
                        .flat_map(|targets| targets.values())
                        .filter_map(|t| t.get("dependencies")?.as_table()),
                )
                .flat_map(|deps| deps.iter())
                .map(|(key, spec)| Dependency {
                    used_as: PackageId::new(key.replace('-', "_")),
                    // `fff = { package = "fff-search" }` uses `fff-search` as `fff`.
                    package: spec
                        .get("package")
                        .and_then(toml::Value::as_str)
                        .unwrap_or(key)
                        .to_owned(),
                })
                .collect();
        dependencies.sort_by(|a, b| a.used_as.cmp(&b.used_as));
        dependencies.dedup_by(|a, b| a.used_as == b.used_as);
        Some(Package {
            id,
            name: package,
            root: manifest.parent().unwrap_or(Path::new("")).to_path_buf(),
            dependencies,
        })
    }

    fn address(&self, project: &Project, path: &Path) -> Result<Address, ResolveError> {
        Ok(Self::place(project, path)?.module)
    }

    fn candidates(&self, project: &Project, address: &Address) -> Vec<PathBuf> {
        project
            .packages
            .named(address.package())
            .map(|p| Self::candidates_for(&p.root.join("src"), address))
            .unwrap_or_default()
    }

    fn resolve(&self, project: &Project, from: &Path, import: &ModulePath) -> Option<Address> {
        let placed = Self::place(project, from).ok()?;
        let module = placed.module;
        let mut segments = import.segments.iter().cloned();
        let base = match &import.head {
            PathHead::Package => Address::root(module.package().clone()),
            PathHead::Here => module,
            PathHead::Up(n) => (0..*n).try_fold(module, |m, _| m.parent())?,
            PathHead::SelfType | PathHead::Root => return None,
            PathHead::Named => {
                let first = import.first()?;
                // 2018 paths may start with a child module: `select::Select`
                // in `rewrite/mod.rs` means `self::select::Select`. Only when
                // that module's file exists; otherwise it is some in-scope name.
                if Self::file_for(project, &placed.src, &module.join(first.clone())).is_some() {
                    module
                } else {
                    // Another crate, by the name this crate uses for it: a
                    // workspace member, possibly renamed, an external
                    // dependency, or the standard library, which no manifest
                    // lists.
                    let head = PackageId::new(first.as_str());
                    let id = project
                        .packages
                        .referent(module.package(), &head)
                        .or_else(|| SYSROOT.contains(&first.as_str()).then_some(head))?;
                    segments.next();
                    Address::root(id)
                }
            }
        };
        Some(base.extend(segments))
    }

    fn companions(&self, project: &Project, from: &Path, to: &Path) -> Vec<(PathBuf, PathBuf)> {
        match Self::other_half(project, from) {
            Some(half) if half.extension().is_some() => vec![(half, to.with_extension("rs"))],
            Some(half) => vec![(half, to.with_extension(""))],
            None => Vec::new(),
        }
    }

    fn touched_by_move(
        &self,
        project: &Project,
        from: &Path,
        to: &Path,
    ) -> Result<Vec<PathBuf>, ResolveError> {
        let sites = Self::move_sites(project, from, to)?;
        let mut touched = vec![sites.old_parent];
        if sites.new_parent != touched[0] {
            touched.push(sites.new_parent);
        }
        Ok(touched)
    }
}
