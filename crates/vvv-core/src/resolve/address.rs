use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::Name;

/// The name a package goes by in paths: a Rust crate's lib name (`fff`, not
/// `fff-search`), a TypeScript package's name. The layout decides the spelling.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PackageId(String);

impl PackageId {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for PackageId {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl fmt::Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A position in a language's namespace: the package it belongs to and the
/// path within it — `fff :: [grep, types]` for a Rust module, path
/// components for a TypeScript file. Prefix relations, which are what a move
/// cares about, never hold across packages.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Address {
    package: PackageId,
    path: Vec<Name>,
}

impl Address {
    pub fn new(
        package: impl Into<PackageId>,
        path: impl IntoIterator<Item = impl Into<Name>>,
    ) -> Self {
        Self {
            package: package.into(),
            path: path.into_iter().map(Into::into).collect(),
        }
    }

    /// The package itself: a crate root, a TypeScript package directory.
    pub fn root(package: impl Into<PackageId>) -> Self {
        Self::new(package, std::iter::empty::<Name>())
    }

    pub fn package(&self) -> &PackageId {
        &self.package
    }

    pub fn path(&self) -> &[Name] {
        &self.path
    }

    pub fn is_root(&self) -> bool {
        self.path.is_empty()
    }

    pub fn starts_with(&self, prefix: &Address) -> bool {
        self.package == prefix.package && self.path.starts_with(&prefix.path)
    }

    /// The enclosing position; `None` at a package root.
    pub fn parent(&self) -> Option<Address> {
        let (_, rest) = self.path.split_last()?;
        Some(Address::new(self.package.clone(), rest.iter().cloned()))
    }

    pub fn join(&self, segment: impl Into<Name>) -> Address {
        let mut path = self.path.clone();
        path.push(segment.into());
        Address {
            package: self.package.clone(),
            path,
        }
    }

    pub fn extend(&self, rest: impl IntoIterator<Item = impl Into<Name>>) -> Address {
        let mut path = self.path.clone();
        path.extend(rest.into_iter().map(Into::into));
        Address {
            package: self.package.clone(),
            path,
        }
    }

    /// Replace the leading `from` with `to`. `None` when `self` is not under `from`.
    pub fn rebase(&self, from: &Address, to: &Address) -> Option<Address> {
        let rest = self.strip_prefix(from)?;
        Some(to.extend(rest.iter().cloned()))
    }

    /// Segments after `prefix`, if `self` is under it (same package).
    pub fn strip_prefix(&self, prefix: &Address) -> Option<&[Name]> {
        (self.package == prefix.package)
            .then(|| self.path.strip_prefix(prefix.path.as_slice()))
            .flatten()
    }

    /// The deepest position both are under; `None` across packages.
    pub fn lowest_common_ancestor(&self, other: &Address) -> Option<Address> {
        if self.package != other.package {
            return None;
        }
        let shared = self
            .path
            .iter()
            .zip(&other.path)
            .take_while(|(a, b)| a == b)
            .count();
        Some(Address::new(
            self.package.clone(),
            self.path[..shared].iter().cloned(),
        ))
    }

    /// The path within the package, joined; the package is not shown.
    pub fn display_with(&self, separator: &str) -> String {
        self.path
            .iter()
            .map(Name::as_str)
            .collect::<Vec<_>>()
            .join(separator)
    }
}

/// `package::a::b`; a nameless package (a lone TypeScript root) is left out
/// rather than shown as a leading `::`.
impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = self.package.as_str().is_empty();
        if !first {
            write!(f, "{}", self.package)?;
        }
        for segment in &self.path {
            if !first {
                f.write_str("::")?;
            }
            write!(f, "{segment}")?;
            first = false;
        }
        Ok(())
    }
}

/// A package a layout found: where it is and what it depends on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    /// How paths name it by default.
    pub id: PackageId,
    /// Its name in the manifest (`fff-search`), which is how dependencies
    /// refer to it whatever they rename it to.
    pub name: String,
    /// Directory holding the manifest, workspace-relative.
    pub root: PathBuf,
    /// What it depends on, workspace members and external packages alike.
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
}

/// One dependency as a manifest states it: the name paths use for it, and
/// the package that name stands for (`fff = { package = "fff-search" }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub used_as: PackageId,
    pub package: String,
}

/// Every package in a workspace, looked up by the deepest root containing a
/// path or by name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Packages {
    by_root: BTreeMap<PathBuf, Package>,
}

impl Packages {
    pub fn new(packages: impl IntoIterator<Item = Package>) -> Self {
        Self {
            by_root: packages.into_iter().map(|p| (p.root.clone(), p)).collect(),
        }
    }

    /// The package whose root is the deepest ancestor of `path`.
    pub fn containing(&self, path: &Path) -> Option<&Package> {
        path.ancestors().find_map(|dir| self.by_root.get(dir))
    }

    pub fn named(&self, id: &PackageId) -> Option<&Package> {
        self.by_root.values().find(|p| &p.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Package> {
        self.by_root.values()
    }

    /// Whether `id` is a package of this workspace, as opposed to one it
    /// only depends on.
    pub fn is_member(&self, id: &PackageId) -> bool {
        self.named(id).is_some()
    }

    /// What a path starting with `head`, written in package `from`, refers
    /// to: a member by its own id, a member through a dependency (renamed or
    /// not), or an external dependency under the name it is used as. `None`
    /// when `from` does not depend on anything of that name.
    pub fn referent(&self, from: &PackageId, head: &PackageId) -> Option<PackageId> {
        if self.is_member(head) {
            return Some(head.clone());
        }
        let dependency = self
            .named(from)?
            .dependencies
            .iter()
            .find(|d| &d.used_as == head)?;
        Some(
            self.iter()
                .find(|p| p.name == dependency.package)
                .map_or_else(|| head.clone(), |member| member.id.clone()),
        )
    }

    /// The name package `from` uses for `target` in paths: the dependency's
    /// key when renamed, otherwise the target's own id.
    pub fn name_for(&self, from: &PackageId, target: &PackageId) -> PackageId {
        let Some(member) = self.named(target) else {
            return target.clone();
        };
        self.named(from)
            .and_then(|p| p.dependencies.iter().find(|d| d.package == member.name))
            .map_or_else(|| target.clone(), |d| d.used_as.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebase_replaces_prefix_only() {
        let a = Address::new("k", ["foo", "bar", "X"]);
        let from = Address::new("k", ["foo", "bar"]);
        let to = Address::new("k", ["baz", "qux"]);
        assert_eq!(
            a.rebase(&from, &to),
            Some(Address::new("k", ["baz", "qux", "X"]))
        );
        assert_eq!(a.rebase(&Address::new("k", ["nope"]), &to), None);
    }

    #[test]
    fn nothing_holds_across_packages() {
        let a = Address::new("a", ["m", "x"]);
        let b = Address::new("b", ["m", "x"]);
        assert!(!a.starts_with(&Address::root("b")));
        assert_eq!(a.strip_prefix(&Address::root("b")), None);
        assert_eq!(a.lowest_common_ancestor(&b), None);
        assert_ne!(a, b);
    }

    #[test]
    fn lowest_common_ancestor_within_a_package() {
        let a = Address::new("k", ["grep", "types", "GrepMatch"]);
        let b = Address::new("k", ["grep", "sink"]);
        assert_eq!(
            a.lowest_common_ancestor(&b),
            Some(Address::new("k", ["grep"]))
        );
        assert_eq!(
            a.lowest_common_ancestor(&Address::new("k", ["other"])),
            Some(Address::root("k"))
        );
        assert_eq!(Address::root("k").parent(), None);
    }

    #[test]
    fn packages_find_the_deepest_root() {
        let packages = Packages::new([
            Package {
                id: "top".into(),
                name: "top".into(),
                root: PathBuf::from(""),
                dependencies: vec![Dependency {
                    used_as: "in".into(),
                    package: "inner-pkg".into(),
                }],
            },
            Package {
                id: "inner_pkg".into(),
                name: "inner-pkg".into(),
                root: PathBuf::from("crates/inner"),
                dependencies: Vec::new(),
            },
        ]);
        assert_eq!(
            packages
                .containing(Path::new("crates/inner/src/lib.rs"))
                .map(|p| p.id.as_str()),
            Some("inner_pkg")
        );
        assert_eq!(
            packages
                .containing(Path::new("src/main.rs"))
                .map(|p| p.id.as_str()),
            Some("top")
        );
        assert_eq!(
            packages
                .named(&"inner_pkg".into())
                .map(|p| p.root.as_path()),
            Some(Path::new("crates/inner"))
        );
    }

    #[test]
    fn renamed_dependencies_refer_to_members_and_are_spelled_back() {
        let packages = Packages::new([
            Package {
                id: "top".into(),
                name: "top".into(),
                root: PathBuf::from(""),
                dependencies: vec![
                    Dependency {
                        used_as: "in".into(),
                        package: "inner-pkg".into(),
                    },
                    Dependency {
                        used_as: "serde".into(),
                        package: "serde".into(),
                    },
                ],
            },
            Package {
                id: "inner_pkg".into(),
                name: "inner-pkg".into(),
                root: PathBuf::from("crates/inner"),
                dependencies: Vec::new(),
            },
        ]);
        let top: PackageId = "top".into();
        assert_eq!(
            packages.referent(&top, &"in".into()),
            Some("inner_pkg".into())
        );
        assert_eq!(
            packages.referent(&top, &"inner_pkg".into()),
            Some("inner_pkg".into())
        );
        assert_eq!(
            packages.referent(&top, &"serde".into()),
            Some("serde".into()),
            "external, by its own name"
        );
        assert_eq!(packages.referent(&top, &"nope".into()), None);
        assert_eq!(packages.name_for(&top, &"inner_pkg".into()), "in".into());
        assert_eq!(
            packages.name_for(&"inner_pkg".into(), &"top".into()),
            "top".into()
        );
    }
}
