//! What a file can see, as far as syntax and the layout can tell: which
//! names its imports bring in and where they point. Used to decide whether a
//! token spelling the renamed name refers to the target declaration. Read
//! off the file's [`Fragment`] and retained with it. Construction reuses
//! resolved edges and captured prefixes for tokens inside qualified paths.

use std::collections::HashMap;

use crate::{Match, Reason};

use super::{Fragment, Namespace};
use vvv_core::{Address, ImportRef, ModulePath, Name, PathHead, Span};

pub struct Scope {
    /// The layout that places a path's prefix, and the project it does so
    /// in; an address in a package that is not one of its members is a
    /// dependency's, which cannot be the target.
    ns: Namespace,
    fragment: std::sync::Arc<Fragment>,
    /// This file's own module address, if it has one.
    module: Option<Address>,
    /// Last path segment → what it resolves to, from plain and grouped imports.
    names: HashMap<Name, Vec<(Address, bool)>>,
    /// Modules whose every name is in scope (`use a::b::*`, or any file
    /// import where the language hides which names were taken).
    opened: Vec<Address>,
    /// Qualified paths in the file that resolved, so a token inside one is
    /// judged by the path up to it rather than as a bare name.
    paths: Vec<Resolved>,
    /// Qualified paths still unresolved after Fragment propagated bindings.
    unresolved: Vec<(Span, ModulePath)>,
}

/// A path the layout placed, with the span that spells it. A group entry
/// (`Span` in `use text::{Line, Span}`) spells only what follows the
/// group's prefix, so the segment under a byte is counted from there.
struct Resolved {
    span: Span,
    path: ModulePath,
    address: Address,
    /// Leading segments the span leaves out: the group prefix's.
    skipped: usize,
    prefixes: Vec<Vec<Address>>,
    reexported: bool,
}

impl Resolved {
    fn of(
        import: &ImportRef,
        address: Address,
        prefixes: Vec<Vec<Address>>,
        reexported: bool,
    ) -> Self {
        let skipped = import
            .group
            .as_ref()
            .and_then(|g| import.path.strip_prefix(&g.prefix))
            .map_or(0, |rest| import.path.segments.len() - rest.len());
        Self {
            span: import.span,
            path: import.path.clone(),
            address,
            skipped,
            prefixes,
            reexported,
        }
    }

    /// The segment a token starting at `at` names, when it is not the
    /// path's last: what the token means is the path up to there.
    fn named_at(&self, at: usize) -> Option<usize> {
        let spelled = if self.skipped == 0 {
            self.path.clone()
        } else {
            ModulePath::new(
                self.path.syntax(),
                PathHead::Named,
                self.path.segments[self.skipped..].iter().cloned(),
            )
        };
        spelled
            .segment_at(at - self.span.start)
            .map(|i| i + self.skipped)
            .filter(|i| i + 1 < self.path.segments.len())
    }
}

/// Explicit/local bindings precede glob-opened modules. Keeping the two tiers
/// separate lets navigation respect language binding precedence.
pub(super) struct Bindings {
    pub direct: Vec<Address>,
    pub opened: Vec<Address>,
    pub explicit: bool,
}

impl Bindings {
    fn explicit(direct: Vec<Address>) -> Self {
        Self {
            direct,
            opened: vec![],
            explicit: true,
        }
    }
}

impl Scope {
    /// What `file` sees, read off its fragment's edges.
    pub fn new(
        ns: &Namespace,
        file: &std::path::Path,
        fragment: &std::sync::Arc<Fragment>,
    ) -> Self {
        let semantics = ns.semantics();
        let mut names: HashMap<Name, Vec<(Address, bool)>> = HashMap::new();
        let mut opened = Vec::new();
        let mut paths = Vec::new();
        let mut unresolved = Vec::new();
        for edge in &fragment.edges {
            let import = &edge.import;
            if edge.resolutions().is_empty() {
                unresolved.push((import.span, import.path.clone()));
                continue;
            }
            for address in edge.addresses() {
                if import.declares {
                    if import.glob || semantics.import_scopes_names {
                        opened.push(address.clone());
                    }
                    if let Some(bound) = import.binding() {
                        let destinations = names.entry(bound.clone()).or_default();
                        if !destinations.iter().any(|(held, _)| held == address) {
                            destinations.push((address.clone(), edge.reexported(file)));
                        }
                    }
                }
                paths.push(Resolved::of(
                    import,
                    address.clone(),
                    edge.prefixes.clone(),
                    edge.reexported(file),
                ));
            }
        }
        Self {
            ns: ns.clone(),
            fragment: fragment.clone(),
            module: fragment.module.clone(),
            names,
            opened,
            paths,
            unresolved,
        }
    }

    pub(super) fn is_current(&self, fragment: &std::sync::Arc<Fragment>) -> bool {
        std::sync::Arc::ptr_eq(&self.fragment, fragment)
    }

    /// Every address supported by this occurrence's module bindings.
    /// Unlike rename classification, navigation preserves competing imports.
    pub(super) fn navigation(&self, span: Span, name: &str, fragment: &Fragment) -> Bindings {
        if let Some(resolved) = self
            .paths
            .iter()
            .filter(|r| r.span.contains(&span))
            .min_by_key(|r| r.span.end - r.span.start)
        {
            let addresses = match resolved.named_at(span.start) {
                Some(index) => resolved.prefixes.get(index).cloned().unwrap_or_default(),
                None => self
                    .paths
                    .iter()
                    .filter(|p| p.span == resolved.span)
                    .map(|p| p.address.clone())
                    .collect(),
            };
            return Bindings::explicit(addresses);
        }

        if self.unresolved.iter().any(|(path, _)| path.contains(&span)) {
            return Bindings::explicit(vec![]);
        }
        let bound: Vec<_> = fragment
            .imports()
            .filter(|e| e.import.binding().is_some_and(|b| b.as_str() == name))
            .collect();
        let explicit = !bound.is_empty();
        let mut direct: Vec<_> = bound
            .into_iter()
            .flat_map(|e| e.addresses().cloned())
            .collect();
        if let Some(module) = &self.module {
            direct.push(module.join(name));
        }
        Bindings {
            direct,
            opened: self.opened.iter().map(|a| a.join(name)).collect(),
            explicit,
        }
    }

    /// Judge a token spelling `name` against a declaration: `aliases` is
    /// every address that reaches it — its own first, then the re-exports
    /// the graph followed — and `others` the modules where the same name is
    /// declared too. The answer is the ground; its
    /// [`Confidence`](crate::Confidence) follows from it.
    pub fn classify(
        &self,
        token: &Match,
        name: &str,
        aliases: &[Address],
        others: &[Address],
    ) -> Reason {
        let target = &aliases[0];
        let target_module = target.parent();

        // Inside a qualified path the path decides — up to the token: `batch`
        // in `commands::batch::BatchCmd` names the module, not the command.
        if let Some(resolved) = self.paths.iter().find(|r| r.span.contains(&token.span)) {
            let index = resolved.named_at(token.span.start);
            let addresses: Vec<_> = match index {
                Some(index) => resolved.prefixes.get(index).cloned().unwrap_or_default(),
                None => self
                    .paths
                    .iter()
                    .filter(|p| p.span == resolved.span)
                    .map(|p| p.address.clone())
                    .collect(),
            };
            let Some(address) = addresses.first() else {
                return Reason::Unresolved;
            };
            if addresses.iter().any(|other| other != address) {
                return Reason::Unresolved;
            }
            let reason = if resolved.reexported {
                Reason::ReExport
            } else if index == Some(0) && self.imported(&resolved.path.prefix(0)).is_some() {
                Reason::Imported
            } else {
                Reason::Path
            };
            return self.compare(address, aliases, others, reason);
        }

        if self.unresolved_path(token) {
            return Reason::Unresolved;
        }
        if self.module.is_some() && self.module == target_module {
            return Reason::Declaring;
        }
        if self.module.as_ref().is_some_and(|m| others.contains(m)) {
            return Reason::OtherDeclaration;
        }
        if let Some(addresses) = self.names.get(name) {
            let (address, reexported) = &addresses[0];
            if addresses.iter().any(|(other, _)| other != address) {
                return Reason::Unresolved;
            }
            return self.compare(
                address,
                aliases,
                others,
                if *reexported {
                    Reason::ReExport
                } else {
                    Reason::Imported
                },
            );
        }
        if self
            .opened
            .iter()
            .any(|m| Some(m) == target_module.as_ref())
        {
            return Reason::Opened;
        }
        // A glob import of a module that re-exports the declaration opens
        // it too.
        if self
            .opened
            .iter()
            .any(|m| aliases[1..].iter().any(|a| a.parent().as_ref() == Some(m)))
        {
            return Reason::ReExport;
        }
        if self.opened.iter().any(|m| others.contains(m)) {
            return Reason::OtherDeclaration;
        }
        Reason::Unresolved
    }

    /// An unresolved path's tail is never judged as a bare name. Fragment
    /// already followed visible imported bindings that could supply its head.
    /// `Self::X` is not a path but the enclosing type, so it stays bare.
    fn unresolved_path(&self, token: &Match) -> bool {
        self.unresolved.iter().any(|(span, path)| {
            span.end == token.span.end
                && span.start < token.span.start
                && path.last().is_some_and(|last| *last == token.text)
                && path.head != PathHead::SelfType
        })
    }

    /// Where a path leads when its head is a name an import binds: the
    /// import's address plus the rest.
    fn imported(&self, path: &ModulePath) -> Option<Address> {
        if path.head != PathHead::Named {
            return None;
        }
        let bindings = self.names.get(path.first()?)?;
        let base = &bindings.first()?.0;
        if bindings.iter().any(|(other, _)| other != base) {
            return None;
        }
        Some(base.extend(path.segments[1..].iter().cloned()))
    }

    /// The target itself resolves, on the ground `via` (an import or a
    /// path); an alias of it resolves through the re-export the graph
    /// followed. A path into a module known to declare the same name is
    /// another declaration; one into a package outside the workspace is
    /// external (a dependency cannot re-export what this workspace
    /// declares). Anything else — a path vvv cannot follow — is unknown,
    /// never assumed to be someone else's.
    fn compare(
        &self,
        address: &Address,
        aliases: &[Address],
        others: &[Address],
        via: Reason,
    ) -> Reason {
        let target = &aliases[0];
        if address == target {
            return via;
        }
        if aliases.contains(address) {
            return Reason::ReExport;
        }
        let same_name = address.path().last() == target.path().last();
        let known_other = address.parent().is_some_and(|m| others.contains(&m));
        let external = address.package() != target.package()
            && !self.ns.project().packages.is_member(address.package());
        match (same_name, known_other, external) {
            (true, true, _) => Reason::OtherDeclaration,
            (true, _, true) => Reason::External,
            _ => Reason::Unresolved,
        }
    }
}
