//! What a file can see, as far as syntax and the layout can tell: which
//! names its imports bring in and where they point. Used to decide whether a
//! token spelling the renamed name refers to the target declaration. Read
//! off the file's [`Fragment`], so it costs no resolution of its own, and
//! kept with the file for as long as the fragment is.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::{Match, Reason};

use super::{Fragment, Namespace};
use vvv_core::{Address, ImportRef, ModulePath, Name, PathHead, Span};

pub struct Scope {
    /// The layout that places a path's prefix, and the project it does so
    /// in; an address in a package that is not one of its members is a
    /// dependency's, which cannot be the target.
    ns: Namespace,
    file: PathBuf,
    /// This file's own module address, if it has one.
    module: Option<Address>,
    /// Last path segment → what it resolves to, from plain and grouped imports.
    names: HashMap<Name, Address>,
    /// Modules whose every name is in scope (`use a::b::*`, or any file
    /// import where the language hides which names were taken).
    opened: Vec<Address>,
    /// Qualified paths in the file that resolved, so a token inside one is
    /// judged by the path up to it rather than as a bare name.
    paths: Vec<Resolved>,
    /// Qualified paths the layout could not resolve on its own: their head
    /// may be an imported name (`b::X` after `use a::b`).
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
}

impl Resolved {
    fn of(import: &ImportRef, address: Address) -> Self {
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

impl Scope {
    /// What `file` sees, read off its fragment's edges.
    pub fn of(ns: &Namespace, file: &std::path::Path, fragment: &Fragment) -> Self {
        let semantics = ns.semantics();
        let mut names = HashMap::new();
        let mut opened = Vec::new();
        let mut paths = Vec::new();
        let mut unresolved = Vec::new();
        for edge in &fragment.edges {
            let import = &edge.import;
            let Some(address) = &edge.address else {
                unresolved.push((import.span, import.path.clone()));
                continue;
            };
            if import.declares {
                if import.glob || semantics.import_scopes_names {
                    opened.push(address.clone());
                }
                if let Some(bound) = import.binding() {
                    names.insert(bound.clone(), address.clone());
                }
            }
            paths.push(Resolved::of(import, address.clone()));
        }
        Self {
            ns: ns.clone(),
            file: file.to_path_buf(),
            module: fragment.module.clone(),
            names,
            opened,
            paths,
            unresolved,
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
            let Some(index) = resolved.named_at(token.span.start) else {
                return self.compare(&resolved.address, aliases, others, Reason::Path);
            };
            let prefix = resolved.path.prefix(index);
            // The head of a path is a name in scope before it is a path:
            // `Span` in `Span::new` is the import, `scope` in `scope::Scope`
            // the child module.
            let (address, reason) = match self.imported(&prefix) {
                Some(address) if index == 0 => (address, Reason::Imported),
                Some(address) => (address, Reason::Path),
                None => match self.ns.resolve(&self.file, &prefix) {
                    Some(address) => (address, Reason::Path),
                    None => (resolved.address.clone(), Reason::Path),
                },
            };
            return self.compare(&address, aliases, others, reason);
        }
        if let Some(reason) = self.qualified(token, aliases, others) {
            return reason;
        }
        if self.module.is_some() && self.module == target_module {
            return Reason::Declaring;
        }
        if self.module.as_ref().is_some_and(|m| others.contains(m)) {
            return Reason::OtherDeclaration;
        }
        if let Some(address) = self.names.get(name) {
            return self.compare(address, aliases, others, Reason::Imported);
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

    /// A token written as the tail of a path (`b::c::X`) is judged by that
    /// path, never as a bare name: `b` imported gives the import's address
    /// plus the rest; otherwise the layout reads the whole path. A head it
    /// cannot place (another crate, a type) leaves the token unresolved.
    /// `Self::X` is not a path but the enclosing type, so it stays bare.
    fn qualified(&self, token: &Match, aliases: &[Address], others: &[Address]) -> Option<Reason> {
        let (_, path) = self.unresolved.iter().find(|(span, path)| {
            span.end == token.span.end
                && span.start < token.span.start
                && path.last().is_some_and(|last| *last == token.text)
        })?;
        if path.head == PathHead::SelfType {
            return None;
        }
        let Some(address) = self.imported(path) else {
            return Some(Reason::Unresolved);
        };
        Some(self.compare(&address, aliases, others, Reason::Path))
    }

    /// Where a path leads when its head is a name an import binds: the
    /// import's address plus the rest.
    fn imported(&self, path: &ModulePath) -> Option<Address> {
        if path.head != PathHead::Named {
            return None;
        }
        let base = self.names.get(path.first()?)?;
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
