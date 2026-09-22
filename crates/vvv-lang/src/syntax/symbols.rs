use ast_grep_core::tree_sitter::{LanguageExt, StrDoc};
use ast_grep_core::{Doc, Node};
use vvv_core::{Modifier, ModifierAt, Span, Symbol, SymbolRule};

/// Walks a tree and applies a plugin's [`SymbolRule`] table.
pub(crate) struct SymbolExtractor<'r> {
    rules: &'r [SymbolRule],
}

/// The rule table's node kinds as this grammar's ids.
struct Ids {
    nodes: Vec<u16>,
    leading: Vec<u16>,
}

/// An ancestor still open during the pre-order walk: its range, where its
/// own extent starts, and the run of leading siblings among its children
/// seen so far.
struct Frame {
    start: usize,
    end: usize,
    lead_start: usize,
    run: Option<usize>,
    prev_end: Option<usize>,
}

impl<'r> SymbolExtractor<'r> {
    pub(crate) fn new(rules: &'r [SymbolRule]) -> Self {
        Self { rules }
    }

    /// Declarations under `root`, in source order, paired with their node.
    /// `source` is the text `root` was parsed from — asking the root node
    /// for its text would validate the whole file as UTF-8 once more.
    pub(crate) fn extract<'t, L: LanguageExt>(
        &self,
        root: &Node<'t, StrDoc<L>>,
        source: &str,
    ) -> Vec<(Node<'t, StrDoc<L>>, Symbol)> {
        // Kinds as ids, so the walk compares numbers, not strings, per node.
        let lang = root.lang();
        let ids = Ids {
            nodes: self.rules.iter().map(|r| lang.kind_to_id(r.node)).collect(),
            leading: {
                let mut v: Vec<u16> = self
                    .rules
                    .iter()
                    .flat_map(|r| r.leading.iter())
                    .map(|k| lang.kind_to_id(k))
                    .collect();
                v.sort_unstable();
                v.dedup();
                v
            },
        };
        // One pre-order pass with one cursor. A stack of open ancestors, by
        // range, stands in for `children()` and `prev()`, which cost a cursor
        // or a walk from the first sibling per call.
        let mut out = Vec::new();
        let mut open: Vec<Frame> = Vec::new();
        for node in root.dfs() {
            let range = node.range();
            if range.start == range.end {
                continue; // a missing node; it declares nothing and leads nothing
            }
            while open
                .last()
                .is_some_and(|f| !(f.start <= range.start && range.end <= f.end))
            {
                open.pop();
            }
            let continuous = open.last().is_none_or(|p| {
                p.prev_end
                    .is_none_or(|end| source[end..range.start].matches('\n').count() <= 1)
            });
            let lead_start = match open.last().and_then(|p| p.run) {
                Some(start) if continuous => start,
                _ => range.start,
            };
            let ancestors = [
                open.last().map_or(range.start, |p| p.lead_start),
                open.len()
                    .checked_sub(2)
                    .map_or(range.start, |i| open[i].lead_start),
            ];
            let kind_id = node.kind_id();
            if ids.nodes.contains(&kind_id)
                && let Some(rule) = self.rule_for(&node)
            {
                out.push((
                    node.clone(),
                    self.symbol(&node, rule, lead_start, ancestors),
                ));
            }
            if let Some(parent) = open.last_mut() {
                parent.run = if ids.leading.contains(&kind_id) {
                    Some(if continuous {
                        parent.run.unwrap_or(range.start)
                    } else {
                        range.start
                    })
                } else {
                    None
                };
                parent.prev_end = Some(range.end);
            }
            open.push(Frame {
                start: range.start,
                end: range.end,
                lead_start,
                run: None,
                prev_end: None,
            });
        }
        out
    }

    /// `rule` matched `node`; `lead_start` is where its own leading run
    /// begins, `ancestors` where its parent's and grandparent's do.
    fn symbol<D: Doc>(
        &self,
        node: &Node<'_, D>,
        rule: &SymbolRule,
        lead_start: usize,
        ancestors: [usize; 2],
    ) -> Symbol {
        let name = match rule.name_field {
            Some(field) => node.field(field),
            None => Some(node.clone()),
        };
        let mut name = name.unwrap_or_else(|| node.clone());
        if let Some(inner) = rule.name_inner {
            while let Some(child) = name.field(inner) {
                name = child;
            }
        }
        // The statement that carries the modifier, if the language wraps
        // declarations (`export class X {}`), else the node.
        let wrapper = match rule.visibility {
            Some(ModifierAt::Parent(kind)) => node
                .ancestors()
                .take(2)
                .enumerate()
                .find(|(_, a)| a.kind() == kind),
            _ => None,
        };
        let visibility = match rule.visibility {
            Some(ModifierAt::Child(kind)) => node
                .children()
                .find(|c| c.kind() == kind)
                .map(|c| Self::modifier(&c)),
            Some(ModifierAt::Parent(_)) => wrapper.as_ref().map(|(_, w)| Self::leading_keywords(w)),
            None => None,
        };
        let (extent_start, extent_end) = match &wrapper {
            Some((depth, w)) => (ancestors[*depth], w.range().end),
            None => (lead_start, node.range().end),
        };
        let extent = if rule.leading.is_empty() && wrapper.is_none() {
            Span::from(node.range())
        } else {
            Span::new(extent_start, extent_end)
        };
        Symbol {
            kind: rule.kind,
            name: name.text().into_owned(),
            name_span: name.range().into(),
            span: node.range().into(),
            extent,
            visibility,
        }
    }

    fn modifier<D: Doc>(node: &Node<'_, D>) -> Modifier {
        Modifier {
            span: node.range().into(),
            text: node.text().into_owned(),
        }
    }

    /// The keywords a wrapper starts with (`export`, `export default`).
    fn leading_keywords<D: Doc>(wrapper: &Node<'_, D>) -> Modifier {
        let keywords: Vec<Node<'_, D>> = wrapper.children().take_while(|c| !c.is_named()).collect();
        let span = match (keywords.first(), keywords.last()) {
            (Some(first), Some(last)) => Span::new(first.range().start, last.range().end),
            _ => Span::new(wrapper.range().start, wrapper.range().start),
        };
        Modifier {
            text: keywords
                .iter()
                .map(|k| k.text().into_owned())
                .collect::<Vec<_>>()
                .join(" "),
            span,
        }
    }

    fn rule_for<D: Doc>(&self, node: &Node<'_, D>) -> Option<&SymbolRule> {
        let kind = node.kind();
        self.rules
            .iter()
            .filter(|rule| rule.node == kind)
            .filter(|rule| {
                rule.under
                    .is_none_or(|p| node.parent().is_some_and(|parent| parent.kind() == p))
            })
            .find(|rule| {
                rule.within
                    .is_none_or(|ancestor| Self::is_within(node, ancestor, rule.node))
            })
    }

    /// True when an ancestor of kind `ancestor` exists and no ancestor of kind
    /// `barrier` (a nested declaration of the same kind) comes first.
    fn is_within<D: Doc>(node: &Node<'_, D>, ancestor: &str, barrier: &str) -> bool {
        for parent in node.ancestors() {
            let kind = parent.kind();
            if kind == ancestor {
                return true;
            }
            if kind == barrier {
                return false;
            }
        }
        false
    }
}
