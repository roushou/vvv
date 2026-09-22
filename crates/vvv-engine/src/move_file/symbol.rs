//! `move --symbol`: one declaration moved between files of its language.
//!
//! The [`Extraction`] is the text that travels. The [`SymbolMove`] is the
//! situation the graph establishes before anything is written — both files
//! parsed, the old and new addresses, who consumes the declaration, whether
//! the old file still uses it — and the operations over it: paths inside the
//! moved text re-spelled from the new file, the imports it needs brought
//! along, every consumer pointed at the new address, the old file kept
//! working, visibility kept wide enough, and finally the cut and the paste.
//! Each operation appends to one [`Change`]; only the paste depends on the
//! others, since edits inside the moved text travel with it.

use std::collections::BTreeSet;
use std::path::Path;

use vvv_core::{Address, Edit, Name, Parsed, Span, Surgery};

use super::{Extraction, MoveSet, Rebase, Widen};
use crate::change::Change;
use crate::command::{Command, Context};
use crate::graph::{Candidate, Namespace};
use crate::{
    Confidence, EngineError, MoveSymbol, MoveSymbolIntent, Notice, NoticeKind, Planned, Reach,
    ReferencesQuery, VfsError,
};

/// Plan moving one declaration to another file of its language. Nothing is
/// written; see [`Apply`](crate::Apply).
impl Command for MoveSymbolIntent {
    type Output = Planned<MoveSymbol>;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let from_path = cx.workspace.normalize(&self.from);
        let to_path = cx.workspace.normalize(&self.to);
        let graph = &mut *cx.graph;
        let source = graph.file(&from_path)?;
        let dest = graph
            .candidate(&to_path)
            .ok_or_else(|| VfsError::NotFound(to_path.clone()))?;
        if source.language() != dest.language() {
            return Err(EngineError::NoLanguage(to_path.clone().into()));
        }
        let ns = graph.namespace_of(&from_path)?;
        let extraction = Extraction::of(source.facts()?, source.text(), &self.name, |kind| {
            ns.is_addressable(kind)
        })
        .ok_or_else(|| EngineError::NoSuchSymbol {
            name: self.name.clone(),
            kind: None,
        })?;
        let old_module = ns.address(&from_path)?;
        let old = old_module.join(self.name.as_str());
        let consumers = graph.consumers(&ns, &old, &[&from_path, &to_path])?;
        let evidence =
            graph.references(&ReferencesQuery::new(self.name.as_str()).declared_in(&from_path))?;
        let candidates = graph.files(Some(&ns.id()));

        let mut mv = SymbolMove::new(&ns, &self.name, &source, &dest, extraction, consumers)?;
        mv.notice_resolved_uses(&evidence.occurrences);
        mv.respell_inner_paths();
        mv.provision()?;
        mv.rebase_consumers(&candidates)?;
        mv.keep_old_file_working();
        mv.drop_redundant_import();
        mv.widen_for_consumers();
        mv.cut_and_paste();
        let (change, from, to) = mv.finish();
        Planned::of(cx.workspace, change, |bound, files| MoveSymbol {
            intent: self.clone(),
            applied: false,
            history_id: None,
            from,
            to,
            notices: bound.notices,
            respellings: bound.respellings,
            files,
        })
    }
}

/// One declaration on its way from one file to another: what the graph
/// established about it, and the change the operations build.
struct SymbolMove<'a> {
    ns: &'a Namespace,
    surgery: &'a dyn Surgery,
    name: &'a str,
    from_path: &'a Path,
    to_path: &'a Path,
    old_module: Address,
    new_module: Address,
    old: Address,
    new: Address,
    source: Parsed<'a>,
    dest: Parsed<'a>,
    extraction: Extraction<'a>,
    /// Modules that name the declaration and must still reach it afterwards.
    consumers: Vec<Address>,
    /// Whether the old file still names the declaration outside the moved
    /// text and its imports, so it will need an import of the new address.
    old_file_still_uses: bool,
    change: Change,
    /// Edits inside the moved text, in the old file's coordinates; they
    /// travel with it rather than being written where they are.
    text_edits: Vec<Edit>,
    /// Import statements the destination gains, in order, no duplicates.
    new_imports: Vec<String>,
}

impl<'a> SymbolMove<'a> {
    fn new(
        ns: &'a Namespace,
        name: &'a str,
        source: &'a Candidate,
        dest: &'a Candidate,
        extraction: Extraction<'a>,
        consumers: Vec<Address>,
    ) -> Result<Self, EngineError> {
        let old_module = ns.address(source.path())?;
        let new_module = ns.address(dest.path())?;
        Ok(Self {
            ns,
            surgery: ns.surgery()?,
            name,
            from_path: source.path(),
            to_path: dest.path(),
            old: old_module.join(name),
            new: new_module.join(name),
            old_module,
            new_module,
            source: Parsed {
                path: source.path(),
                source: source.file().source(),
                facts: source.facts()?,
            },
            dest: Parsed {
                path: dest.path(),
                source: dest.file().source(),
                facts: dest.facts()?,
            },
            extraction,
            consumers,
            old_file_still_uses: false,
            change: Change::new(),
            text_edits: Vec::new(),
            new_imports: Vec::new(),
        })
    }

    /// What the resolved uses of the declaration say: a bare use left in the
    /// old file means it needs an import afterwards; every other file with
    /// one is a consumer too.
    fn notice_resolved_uses(&mut self, occurrences: &[crate::Occurrence]) {
        for m in occurrences
            .iter()
            .filter(|o| o.confidence == Confidence::Resolved)
            .map(|o| &o.m)
        {
            if m.path == self.from_path {
                let in_import = self
                    .source
                    .facts
                    .imports
                    .iter()
                    .any(|i| i.span.contains(&m.span));
                if !self.extraction.contains(m.span) && !in_import {
                    self.old_file_still_uses = true;
                }
            } else if m.path != self.to_path
                && let Ok(module) = self.ns.address(&m.path)
                && !self.consumers.contains(&module)
            {
                self.consumers.push(module);
            }
        }
    }

    /// Qualified paths inside the moved text, re-rendered from the new file.
    fn respell_inner_paths(&mut self) {
        for import in self
            .source
            .facts
            .imports
            .iter()
            .filter(|i| !i.declares && self.extraction.contains(i.span))
        {
            if let Some(resolved) = self.ns.resolve(self.from_path, &import.path) {
                let rendered =
                    self.surgery
                        .render(self.ns.project(), self.to_path, &resolved, &import.path);
                if rendered != import.path {
                    self.text_edits
                        .push(Edit::replace(import.span, rendered.to_string()));
                }
            }
        }
    }

    /// What the moved text uses: imports of the old file it relied on, and
    /// siblings declared beside it. Both become imports in the new file;
    /// siblings must also stay visible from there.
    fn provision(&mut self) -> Result<(), EngineError> {
        let used = self.extraction.names_used(self.source.facts);
        let already: BTreeSet<String> = self
            .dest
            .facts
            .imports
            .iter()
            .filter(|i| i.declares && !self.dest.is_group_prefix(i))
            .filter_map(|i| i.binding())
            .map(Name::to_string)
            .collect();
        let source = self.source;
        for import in source
            .facts
            .imports
            .iter()
            .filter(|i| i.declares && !self.extraction.contains(i.span))
        {
            if source.is_group_prefix(import) {
                continue;
            }
            let Some(name) = import.binding().map(Name::to_string) else {
                continue;
            };
            if !used.contains(&name) || already.contains(&name) || name == self.name {
                continue;
            }
            let statement = match self.ns.resolve(self.from_path, &import.path) {
                Some(address) if !address.is_root() => {
                    self.surgery
                        .import_statement(self.ns.project(), self.to_path, &address, &name)
                }
                _ => self.surgery.import_of_path(&import.path, &name),
            };
            if let Some(statement) = statement
                && !self.new_imports.contains(&statement)
            {
                self.new_imports.push(statement);
            }
        }
        let siblings: Vec<&vvv_core::Symbol> = source
            .facts
            .symbols
            .iter()
            .filter(|s| {
                s.name != self.name
                    && self.ns.is_addressable(s.kind)
                    && used.contains(&s.name)
                    && !already.contains(&s.name)
                    && !self.extraction.contains(s.span)
            })
            .collect();
        for sibling in siblings {
            let address = self.old_module.join(sibling.name.as_str());
            if let Some(statement) = self.surgery.import_statement(
                self.ns.project(),
                self.to_path,
                &address,
                &sibling.name,
            ) && !self.new_imports.contains(&statement)
            {
                self.new_imports.push(statement);
            }
            if !self
                .ns
                .reach(&self.old_module, sibling)
                .admits(&self.new_module)
            {
                let widen = Widen {
                    symbol: sibling,
                    source: self.source.source,
                    needs: Reach::narrowest(
                        &self.old_module,
                        std::slice::from_ref(&self.new_module),
                    ),
                    consumer: self.new_module.clone(),
                    notice_at: (
                        self.from_path.to_path_buf(),
                        self.source.source.position(sibling.name_span.start),
                    ),
                };
                match widen.plan(self.surgery) {
                    Ok(edit) => self.change.edit(self.from_path, edit),
                    Err(notice) => self.change.notice(notice),
                }
            }
        }
        Ok(())
    }

    /// Every consumer: imports and qualified paths are rebased to the new
    /// address. Self-references inside the moved text travel with it and are
    /// not rows of the preview.
    fn rebase_consumers(&mut self, candidates: &[Candidate]) -> Result<(), EngineError> {
        let no_moves = MoveSet::default();
        let rebase = Rebase::of_addresses(
            self.ns.layout(),
            self.surgery,
            self.ns.project(),
            &no_moves,
            self.old.clone(),
            self.new.clone(),
        );
        for candidate in candidates.iter().filter(|c| c.path() != self.to_path) {
            let mut rewrite = rebase.rewrite(candidate)?;
            if candidate.path() == self.from_path {
                let (moving, staying): (Vec<Edit>, Vec<Edit>) = rewrite
                    .change
                    .take_edits(self.from_path)
                    .into_iter()
                    .partition(|e| self.extraction.contains(e.span));
                self.text_edits.extend(moving);
                rewrite.change.edits(self.from_path, staying);
                let extraction = &self.extraction;
                rewrite
                    .change
                    .retain_respellings(|r| !extraction.contains(r.span));
            }
            self.change.merge(rewrite.change);
        }
        Ok(())
    }

    /// A bare use left in the old file needs an import of the new address.
    fn keep_old_file_working(&mut self) {
        if self.old_file_still_uses
            && let Some(statement) = self.surgery.import_statement(
                self.ns.project(),
                self.from_path,
                &self.new,
                self.name,
            )
        {
            self.change.edit(
                self.from_path,
                Edit::insert(
                    self.surgery.import_insertion(&self.source),
                    format!("{statement}\n"),
                ),
            );
        }
    }

    /// An import of the declaration in the file it moves to is now
    /// redundant: delete the statement, or say so when it shares a group.
    fn drop_redundant_import(&mut self) {
        let text = self.dest.source.as_str();
        for import in self.dest.facts.imports.iter().filter(|i| i.declares) {
            if self.ns.resolve(self.to_path, &import.path).as_ref() != Some(&self.old) {
                continue;
            }
            match &import.group {
                None => {
                    let statement = Span::new(
                        text[..import.span.start].rfind('\n').map_or(0, |i| i + 1),
                        text[import.span.end..]
                            .find('\n')
                            .map_or(text.len(), |nl| import.span.end + nl + 1),
                    );
                    self.change.edit(self.to_path, Edit::delete(statement));
                }
                Some(_) => self.change.notice(Notice {
                    path: self.to_path.into(),
                    start: self.dest.source.position(import.span.start),
                    kind: NoticeKind::RedundantImport {
                        import: import.path.to_string(),
                    },
                }),
            }
        }
    }

    /// The item's own reach at its new home: widened for the consumers it
    /// no longer admits, as narrowly as real code writes.
    fn widen_for_consumers(&mut self) {
        let reach = self.ns.reach(&self.new_module, self.extraction.symbol);
        let blind: Vec<Address> = self
            .consumers
            .iter()
            .filter(|c| !reach.admits(c))
            .cloned()
            .collect();
        let Some(first) = blind.first() else {
            return;
        };
        let widen = Widen {
            symbol: self.extraction.symbol,
            source: self.source.source,
            needs: Reach::narrowest(&self.new_module, &blind),
            consumer: first.clone(),
            notice_at: (
                self.to_path.to_path_buf(),
                self.dest.source.position(self.dest.source.len()),
            ),
        };
        match widen.plan(self.surgery) {
            Ok(edit) => self.text_edits.push(edit),
            Err(notice) => self.change.notice(notice),
        }
    }

    /// Cut the pieces out of the old file; paste them, with their edits,
    /// where the destination keeps items, its new imports above.
    fn cut_and_paste(&mut self) {
        let moved = self.extraction.assemble(&self.text_edits);
        for cut in self.extraction.cuts() {
            self.change.edit(self.from_path, Edit::delete(cut));
        }
        if !self.new_imports.is_empty() {
            let at = self.surgery.import_insertion(&self.dest);
            let block: String = self.new_imports.iter().map(|s| format!("{s}\n")).collect();
            self.change.edit(self.to_path, Edit::insert(at, block));
        }
        let at = self.surgery.item_insertion(&self.dest);
        let text = self.dest.source.as_str();
        let separator = if text.is_empty() || text.ends_with("\n\n") {
            ""
        } else if text.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        self.change.edit(
            self.to_path,
            Edit::insert(at, format!("{separator}{moved}\n")),
        );
    }

    fn finish(self) -> (Change, Address, Address) {
        (self.change, self.old, self.new)
    }
}
