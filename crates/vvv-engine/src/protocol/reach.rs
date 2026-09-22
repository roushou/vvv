//! Who may name a declaration, computed from its modifier and its module.

use serde::{Deserialize, Serialize};
use vvv_core::{Address, PackageId, ReachKind};

/// The set of modules that may name a declaration: a kind applied to the
/// declaring module's address. Never declared, always computed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reach {
    Within(Address),
    Package(PackageId),
    Everyone,
}

impl Reach {
    /// `kind` applied to `declaring`. A `Path` kind needs its module
    /// resolved by the caller; unresolved, it is treated as the declaring
    /// module, the narrowest reading.
    pub fn of(kind: ReachKind, declaring: &Address, path: Option<Address>) -> Self {
        match kind {
            ReachKind::Declaring => Self::Within(declaring.clone()),
            ReachKind::Parent => {
                Self::Within(declaring.parent().unwrap_or_else(|| declaring.clone()))
            }
            ReachKind::Package => Self::Package(declaring.package().clone()),
            ReachKind::Everyone => Self::Everyone,
            ReachKind::Path => Self::Within(path.unwrap_or_else(|| declaring.clone())),
        }
    }

    /// The narrowest kind real code writes that admits every consumer of a
    /// declaration in `declaring`: the parent when they all sit under it,
    /// else the package; another package means `pub`, which is never
    /// inferred.
    pub fn narrowest(declaring: &Address, consumers: &[Address]) -> ReachKind {
        let mut lca = declaring.clone();
        for consumer in consumers {
            match lca.lowest_common_ancestor(consumer) {
                Some(shared) => lca = shared,
                None => return ReachKind::Everyone,
            }
        }
        if Some(&lca) == declaring.parent().as_ref() {
            ReachKind::Parent
        } else if &lca == declaring {
            ReachKind::Declaring
        } else {
            ReachKind::Package
        }
    }

    /// Whether every module `other` admits, this reach admits too.
    pub fn covers(&self, other: &Reach) -> bool {
        match (self, other) {
            (Self::Everyone, _) => true,
            (Self::Package(p), Self::Package(q)) => p == q,
            (Self::Package(p), Self::Within(a)) => a.package() == p,
            (Self::Within(a), Self::Within(b)) => b.starts_with(a),
            _ => false,
        }
    }

    /// Whether code in module `from` may name the declaration.
    pub fn admits(&self, from: &Address) -> bool {
        match self {
            Self::Within(module) => from.starts_with(module),
            Self::Package(package) => from.package() == package,
            Self::Everyone => true,
        }
    }
}

impl std::fmt::Display for Reach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Within(module) if module.is_root() => write!(f, "{} (root)", module.package()),
            Self::Within(module) => write!(f, "within {module}"),
            Self::Package(package) => write!(f, "package {package}"),
            Self::Everyone => f.write_str("everyone"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reach_admits_by_prefix_package_or_everyone() {
        let declaring = Address::new("k", ["a", "b"]);
        let sibling = Address::new("k", ["a", "c"]);
        let child = Address::new("k", ["a", "b", "d"]);
        let elsewhere = Address::new("other", ["a", "b"]);
        let private = Reach::of(ReachKind::Declaring, &declaring, None);
        assert!(private.admits(&child) && !private.admits(&sibling));
        let parent = Reach::of(ReachKind::Parent, &declaring, None);
        assert!(parent.admits(&sibling) && !parent.admits(&Address::root("k")));
        let package = Reach::of(ReachKind::Package, &declaring, None);
        assert!(package.admits(&Address::root("k")) && !package.admits(&elsewhere));
        assert!(Reach::of(ReachKind::Everyone, &declaring, None).admits(&elsewhere));
        assert_eq!(
            Reach::of(ReachKind::Parent, &Address::root("k"), None),
            Reach::Within(Address::root("k"))
        );
    }
}
