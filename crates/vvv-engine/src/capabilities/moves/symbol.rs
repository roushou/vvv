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
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vvv_core::{Address, Edit, Name, Parsed, Span, Surgery};

use super::{Extraction, Rebase, Site, Widen};
use crate::change::Change;
use crate::graph::{Candidate, Fragment, Namespace, Node};
use crate::protocol::vocabulary::IntentLine;
use crate::report::{Document, MoveCounts};
use crate::{
    Confidence, EngineError, FileChange, Intent, Mutation, Notice, NoticeKind, Planned, Reach,
    ReferencesQuery, Respelling, VfsError,
};

/// Move one declaration — with what belongs to it in the text, and for Rust
/// its `impl` blocks — from the file declaring it to another file of the
/// same language, and make every reference follow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MoveSymbolIntent {
    pub name: String,
    /// The file declaring it.
    pub from: PathBuf,
    /// The file to declare it in; it must exist.
    pub to: PathBuf,
}

impl MoveSymbolIntent {
    pub fn new(name: impl Into<String>, from: impl Into<PathBuf>, to: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            from: from.into(),
            to: to.into(),
        }
    }
}

/// `vvv move --symbol`: one declaration moved between files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MoveSymbol {
    pub intent: MoveSymbolIntent,
    /// Preview or successful application with its history entry.
    #[serde(flatten)]
    pub state: crate::MutationState,
    /// The declaration's address before and after.
    pub from: Address,
    pub to: Address,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<Notice>,
    /// Consumers rewritten in place; every other edit in `files` is the
    /// declaration itself moving.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub respellings: Vec<Respelling>,
    pub files: Vec<FileChange>,
}

impl Mutation for MoveSymbol {
    fn into_mutation(self) -> crate::MutationAnswer {
        crate::MutationAnswer::MoveSymbol(self)
    }

    fn applied(&mut self, id: u64) {
        self.state = crate::MutationState::Applied { history_id: id };
    }
}

/// Plan moving one declaration to another file of its language. Nothing is
/// written; see [`Apply`](crate::Apply).
impl MoveSymbolIntent {
    /// Plan without writing files.
    pub fn plan(self, engine: &crate::Engine) -> Result<Planned<MoveSymbol>, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.plan_in(&mut graph, engine.workspace())
    }

    pub(crate) fn plan_in(
        self,
        graph: &mut crate::graph::Graph,
        workspace: &crate::Workspace,
    ) -> Result<Planned<MoveSymbol>, EngineError> {
        let from_path = workspace.normalize(&self.from);
        let to_path = workspace.normalize(&self.to);
        let source = graph.file(&from_path)?;
        let dest = graph
            .candidate(&to_path)
            .ok_or_else(|| VfsError::NotFound(to_path.clone()))?;
        if source.language() != dest.language() {
            return Err(EngineError::NoLanguage(to_path.clone().into()));
        }
        let ns = graph.namespace_of(&from_path)?;
        let extraction = Extraction::of(source.facts()?, source.file(), &self.name, |kind| {
            ns.is_addressable(kind)
        })?
        .ok_or_else(|| EngineError::NoSuchSymbol {
            name: self.name.clone(),
            kind: None,
        })?;
        let old_module = ns.address(&from_path)?;
        let old = old_module.join(self.name.as_str());
        let consumers = graph.consumers(&ns, &old, &[&from_path, &to_path])?;
        let evidence =
            graph.references(&ReferencesQuery::new(self.name.as_str()).declared_in(&from_path))?;
        let nodes = graph.fragments(&ns)?;

        let mut mv = SymbolMove::new(&ns, &self.name, &source, &dest, extraction, consumers)?;
        mv.notice_resolved_uses(&evidence.occurrences);
        mv.provision()?;
        mv.rebase_consumers(&nodes)?;
        mv.keep_old_file_working()?;
        mv.drop_redundant_import()?;
        mv.widen_for_consumers();
        mv.cut_and_paste()?;
        let (change, from, to) = mv.finish();
        Planned::of(
            workspace,
            change,
            Intent::MoveSymbol(self.clone()),
            |bound, files| MoveSymbol {
                intent: self.clone(),
                state: crate::MutationState::Preview,
                from,
                to,
                notices: bound.notices,
                respellings: bound.respellings,
                files,
            },
        )
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
    source_fragment: std::sync::Arc<Fragment>,
    dest_fragment: std::sync::Arc<Fragment>,
    source: Parsed<'a>,
    dest: Parsed<'a>,
    source_witness: &'a crate::workspace::SourceWitness,
    dest_witness: &'a crate::workspace::SourceWitness,
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
            source_fragment: source.fragment(ns)?,
            dest_fragment: dest.fragment(ns)?,
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
            source_witness: source.file().witness(),
            dest_witness: dest.file().witness(),
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
        for edge in self
            .source_fragment
            .imports()
            .filter(|e| !self.extraction.contains(e.import.span))
        {
            let import = &edge.import;
            if source.is_group_prefix(import) {
                continue;
            }
            let Some(name) = import.binding().map(Name::to_string) else {
                continue;
            };
            if !used.contains(&name) || already.contains(&name) || name == self.name {
                continue;
            }
            let statement = match edge.address() {
                Some(address) if !address.is_root() => {
                    self.surgery
                        .import_statement(self.ns.project(), self.to_path, address, &name)
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
                    Ok(edit) => self.change.edit(self.source_witness, edit)?,
                    Err(notice) => self.change.notice(notice),
                }
            }
        }
        Ok(())
    }

    /// Every consumer: imports and qualified paths are rebased to the new
    /// address. Self-references inside the moved text travel with it and are
    /// not rows of the preview.
    fn rebase_consumers(&mut self, nodes: &[Node]) -> Result<(), EngineError> {
        let rebase = Rebase::new(self.ns, self.old.clone(), self.new.clone());
        for node in nodes.iter().filter(|c| c.path() != self.to_path) {
            let site = if node.path() == self.from_path {
                let (moving, staying) = Site::partition(node, self.to_path, &self.extraction);
                let rewritten = rebase.rewrite(&moving)?;
                self.text_edits.extend(rewritten.edits);
                for notice in rewritten.notices {
                    self.change.notice(notice);
                }
                staying
            } else {
                Site::of(node, node.path())
            };
            let (contribution, _) = rebase.rewrite(&site)?.into_change()?;
            self.change.merge(contribution)?;
        }
        Ok(())
    }

    /// A bare use left in the old file needs an import of the new address.
    fn keep_old_file_working(&mut self) -> Result<(), EngineError> {
        if self.old_file_still_uses
            && let Some(statement) = self.surgery.import_statement(
                self.ns.project(),
                self.from_path,
                &self.new,
                self.name,
            )
        {
            self.change.edit(
                self.source_witness,
                Edit::insert(
                    self.surgery.import_insertion(&self.source),
                    format!("{statement}\n"),
                ),
            )?;
        }
        Ok(())
    }

    /// An import of the declaration in the file it moves to is now
    /// redundant: delete the statement, or say so when it shares a group.
    fn drop_redundant_import(&mut self) -> Result<(), EngineError> {
        let text = self.dest.source.as_str();
        for edge in self.dest_fragment.imports() {
            let import = &edge.import;
            if edge.address() != Some(&self.old) {
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
                    self.change
                        .edit(self.dest_witness, Edit::delete(statement))?;
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
        Ok(())
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
    fn cut_and_paste(&mut self) -> Result<(), EngineError> {
        let moved = self.extraction.assemble(&self.text_edits)?;
        for cut in self.extraction.cuts() {
            self.change.edit(self.source_witness, Edit::delete(cut))?;
        }
        if !self.new_imports.is_empty() {
            let at = self.surgery.import_insertion(&self.dest);
            let block: String = self.new_imports.iter().map(|s| format!("{s}\n")).collect();
            self.change
                .edit(self.dest_witness, Edit::insert(at, block))?;
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
            self.dest_witness,
            Edit::insert(at, format!("{separator}{moved}\n")),
        )?;
        Ok(())
    }

    fn finish(self) -> (Change, Address, Address) {
        (self.change, self.old, self.new)
    }
}

impl Document {
    pub(crate) fn move_symbol(result: &MoveSymbol) -> Self {
        let mut report = Self::new();
        report.title(IntentLine(&Intent::MoveSymbol(result.intent.clone())));
        let structural = report.moved(
            result.state,
            &result.files,
            &result.respellings,
            &result.notices,
        );
        report.moved_summary(
            result.state,
            MoveCounts {
                respellings: result.respellings.len(),
                structural,
                notices: result.notices.len(),
                files: result.files.len(),
            },
        );
        report
    }
}
