//! How to spell an edit in Rust: a path from one module to another, a
//! grouped `use` rewritten, a `mod` declaration moved between parents. Text
//! and facts in, edits out; placement questions go to the [`RustLayout`].

use std::path::Path;

use vvv_core::{
    Address, Edit, ImportGroup, ImportRef, Layout, ModulePath, Parsed, PathHead, Project,
    ReachKind, Regrouped, ResolveError, SideEdit, SourceText, Span, Surgery, Symbol, SymbolKind,
};

use super::layout::{RustLayout, SYNTAX};

#[derive(Debug, Clone, Copy, Default)]
pub struct RustSurgery;

/// A `mod name;` item.
struct ModDecl {
    span: Span,
    name_span: Span,
    name: String,
    text: String,
    /// The modifier's start and length, when the line has one.
    modifier: Option<(usize, usize)>,
}

/// A grouped entry that cannot stay under its group's prefix.
struct Leaving<'a> {
    group: &'a ImportGroup,
    /// The full path to write in a statement of its own, tail included.
    path: String,
}

impl RustSurgery {
    /// How Rust spells a reach vvv is willing to grant. `Everyone` is never
    /// inferred; `Declaring` is what no modifier already means.
    fn modifier_for(to: ReachKind) -> Option<&'static str> {
        match to {
            ReachKind::Parent => Some("pub(super)"),
            ReachKind::Package => Some("pub(crate)"),
            ReachKind::Declaring | ReachKind::Everyone | ReachKind::Path => None,
        }
    }

    /// `mod <name>;` items in a parsed file — declarations, not inline
    /// `mod x { … }` bodies.
    fn mod_declarations(file: &Parsed<'_>) -> Vec<ModDecl> {
        file.facts
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Module)
            .map(|s| (s, file.source.slice(s.span)))
            .filter(|(_, text)| text.trim_end().ends_with(';'))
            .map(|(s, text)| ModDecl {
                span: s.span,
                name_span: s.name_span,
                name: s.name.clone(),
                text: text.to_owned(),
                modifier: s
                    .visibility
                    .as_ref()
                    .map(|m| (m.span.start, m.span.end - m.span.start)),
            })
            .collect()
    }

    /// If `target` still sits under what the group's prefix means from
    /// `file`, the entry can be rewritten in place: the remaining segments.
    fn rest_within_group(
        project: &Project,
        file: &Path,
        group: &ImportGroup,
        target: &Address,
    ) -> Option<String> {
        let prefix_now = RustLayout.resolve(project, file, &group.prefix)?;
        let rest = target.strip_prefix(&prefix_now)?;
        (!rest.is_empty()).then(|| group.prefix.spell_segments(rest))
    }

    /// Take `leaving` entries out of their statement into statements of their
    /// own, keeping the original's visibility and indentation. When the
    /// statement's own list empties, the new statements replace it.
    fn leave_group(source: &SourceText, statement: Span, leaving: &[Leaving<'_>]) -> Vec<Edit> {
        let text = source.as_str();
        let line_start = text[..statement.start].rfind('\n').map_or(0, |i| i + 1);
        let indent = &text[line_start..statement.start];
        let statement_text = source.slice(statement);
        let visibility = statement_text
            .find("use ")
            .map_or("", |i| &statement_text[..i]);
        let statements: Vec<String> = leaving
            .iter()
            .map(|l| format!("{visibility}use {};", l.path))
            .collect();

        let top_level_leaving = leaving.iter().filter(|l| l.group.top_level).count();
        let top_level_items = leaving
            .iter()
            .find(|l| l.group.top_level)
            .map(|l| l.group.items);
        if Some(top_level_leaving) == top_level_items {
            return vec![Edit::replace(
                statement,
                statements.join(&format!("\n{indent}")),
            )];
        }

        let mut removals: Vec<Span> = leaving
            .iter()
            .map(|l| Self::item_removal(text, l.group.item))
            .collect();
        removals.sort();
        let mut merged: Vec<Span> = Vec::new();
        for span in removals {
            match merged.last_mut() {
                Some(last) if last.end >= span.start => *last = last.union(&span),
                _ => merged.push(span),
            }
        }
        let mut edits: Vec<Edit> = merged.into_iter().map(Edit::delete).collect();
        let insertion: String = statements
            .iter()
            .map(|s| format!("\n{indent}{s}"))
            .collect();
        edits.push(Edit::insert(statement.end, insertion));
        edits
    }

    /// `item` plus the separator that goes with it: the following comma and
    /// spacing (the whole line when the item has one to itself), or for the
    /// last entry the preceding comma.
    fn item_removal(text: &str, item: Span) -> Span {
        let after = &text[item.end..];
        let ws = after.len() - after.trim_start().len();
        if after[ws..].starts_with(',') {
            let comma_end = item.end + ws + 1;
            let rest = &text[comma_end..];
            let line_start = text[..item.start].rfind('\n').map_or(0, |i| i + 1);
            let owns_line = text[line_start..item.start].trim().is_empty();
            if let Some(nl) = rest.find('\n')
                && rest[..nl].trim().is_empty()
                && owns_line
            {
                return Span::new(line_start, comma_end + nl + 1);
            }
            let spaces = rest.len() - rest.trim_start_matches([' ', '\t']).len();
            return Span::new(item.start, comma_end + spaces);
        }
        let before = &text[..item.start];
        let trimmed = before.trim_end();
        if trimmed.ends_with(',') {
            return Span::new(trimmed.len() - 1, item.end);
        }
        item
    }
}

impl Surgery for RustSurgery {
    /// Keep the original's style: a path relative to the module stays
    /// relative (`self::` kept when it had one), `super::` chains stay as
    /// deep as they were when the target is still under them; anything
    /// else is spelled from the crate root, or from the crate's name across
    /// crates.
    fn render(
        &self,
        project: &Project,
        from: &Path,
        target: &Address,
        original: &ModulePath,
    ) -> ModulePath {
        let module = RustLayout::place(project, from).ok().map(|p| p.module);
        if let Some(module) = module {
            match &original.head {
                head @ (PathHead::Here | PathHead::Named) => {
                    if let Some(rest) = target.strip_prefix(&module)
                        && !rest.is_empty()
                    {
                        return ModulePath::new(SYNTAX, head.clone(), rest.iter().cloned());
                    }
                }
                PathHead::Up(n) => {
                    let base = (0..*n).try_fold(module.clone(), |m, _| m.parent());
                    if let Some(base) = base
                        && let Some(rest) = target.strip_prefix(&base)
                        && !rest.is_empty()
                    {
                        return ModulePath::new(SYNTAX, PathHead::Up(*n), rest.iter().cloned());
                    }
                }
                PathHead::Package | PathHead::SelfType | PathHead::Root => {}
            }
            if module.package() == target.package() {
                return RustLayout::from_crate_root(target);
            }
            let name = project
                .packages
                .name_for(module.package(), target.package());
            return RustLayout::from_crate_named(&name, target);
        }
        RustLayout::from_crate_named(target.package(), target)
    }

    fn import_statement(
        &self,
        project: &Project,
        from: &Path,
        target: &Address,
        _name: &str,
    ) -> Option<String> {
        // Spelled as a bare path would be: relative when the target is under
        // the module, from the crate root otherwise.
        let bare = ModulePath::new(SYNTAX, PathHead::Named, std::iter::empty::<&str>());
        Some(format!(
            "use {};",
            self.render(project, from, target, &bare)
        ))
    }

    fn import_of_path(&self, path: &ModulePath, _name: &str) -> Option<String> {
        Some(format!("use {path};"))
    }

    /// After the last `use`; with none, after the leading `//!` docs and
    /// `#![…]` attributes, which must stay first.
    fn import_insertion(&self, file: &Parsed<'_>) -> usize {
        let text = file.source.as_str();
        if let Some(end) = file
            .facts
            .imports
            .iter()
            .filter(|i| i.declares)
            .map(|i| i.span.end)
            .max()
        {
            return text[end..].find('\n').map_or(text.len(), |nl| end + nl + 1);
        }
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//!") || trimmed.starts_with("#![") || trimmed.trim().is_empty()
            {
                offset += line.len();
            } else {
                break;
            }
        }
        offset
    }

    fn regroup(
        &self,
        project: &Project,
        from: &Path,
        source: &SourceText,
        entries: &[(ImportRef, Address)],
    ) -> Regrouped {
        let Some(statement) = entries.first().and_then(|(r, _)| r.group.as_ref()) else {
            return Regrouped::default();
        };
        let statement = statement.statement;
        let mut edits = Vec::new();
        let mut leaving: Vec<Leaving<'_>> = Vec::new();

        for (import, target) in entries {
            let Some(group) = &import.group else {
                continue;
            };
            match Self::rest_within_group(project, from, group, target) {
                Some(rest) => edits.push(Edit::replace(import.span, rest)),
                None => leaving.push(Leaving {
                    group,
                    // The entry's tail: ` as Alias`, `::*`, `::{self, X}`.
                    path: format!(
                        "{}{}",
                        self.render(project, from, target, &import.path),
                        source.slice(Span::new(import.span.end, group.item.end))
                    ),
                }),
            }
        }

        if !leaving.is_empty() {
            edits.extend(Self::leave_group(source, statement, &leaving));
        }
        Regrouped {
            edits,
            skipped: Vec::new(),
        }
    }

    /// Move the `mod` declaration: a rename in place when the parent is
    /// unchanged, otherwise a removal from the old parent and an insertion
    /// after the new parent's last `mod` line.
    fn widen(&self, symbol: &Symbol, _source: &SourceText, to: ReachKind) -> Option<Edit> {
        let text = Self::modifier_for(to)?;
        Some(match &symbol.visibility {
            Some(modifier) => Edit::replace(modifier.span, text),
            None => Edit::insert(symbol.span.start, format!("{text} ")),
        })
    }

    fn relocate(
        &self,
        project: &Project,
        from: &Path,
        to: &Path,
        touched: &[Parsed<'_>],
        widen_to: Option<ReachKind>,
    ) -> Result<Vec<SideEdit>, ResolveError> {
        let sites = RustLayout::move_sites(project, from, to)?;
        let parsed = |path: &Path| {
            touched
                .iter()
                .find(|p| p.path == path)
                .ok_or_else(|| ResolveError::Missing(path.into()))
        };
        let old_parent = parsed(&sites.old_parent)?;
        let decl = Self::mod_declarations(old_parent)
            .into_iter()
            .find(|d| d.name == sites.old_name)
            .ok_or_else(|| ResolveError::NoDeclaration {
                declaration: format!("mod {};", sites.old_name),
                parent: sites.old_parent.clone().into(),
            })?;

        if sites.old_parent == sites.new_parent {
            return Ok(vec![SideEdit {
                path: sites.old_parent,
                edit: Edit::replace(decl.name_span, sites.new_name),
            }]);
        }

        let rel = decl.name_span.start - decl.span.start;
        let mut declaration = decl.text.clone();
        declaration.replace_range(rel..rel + decl.name.len(), &sites.new_name);
        // The engine judged who still needs to see the module from its new
        // parent; widen only when it said so, and only to what it said.
        if let Some(text) = widen_to.and_then(Self::modifier_for) {
            match decl.modifier {
                Some((start, len)) => {
                    declaration.replace_range(
                        start - decl.span.start..start - decl.span.start + len,
                        text,
                    );
                }
                None => declaration.insert_str(0, &format!("{text} ")),
            }
        }

        let old_text = old_parent.source.as_str();
        let item = decl.span;
        let delete_end = if old_text[item.end..].starts_with('\n') {
            item.end + 1
        } else {
            item.end
        };
        let new_parent = parsed(&sites.new_parent)?;
        let insert = match Self::mod_declarations(new_parent).last() {
            Some(last) => Edit::insert(last.span.end, format!("\n{declaration}")),
            None => Edit::insert(0, format!("{declaration}\n")),
        };
        Ok(vec![
            SideEdit {
                path: sites.old_parent,
                edit: Edit::delete(Span::new(item.start, delete_end)),
            },
            SideEdit {
                path: sites.new_parent,
                edit: insert,
            },
        ])
    }
}
