use super::navigation::UnsupportedSyntax;
use crate::syntax::navigation::{BindingName, BindingSite};
use crate::syntax::views::syntax_view;
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{
    BindingNamespace, ConstructorShape, Facts, Grammar, PathSyntax, PatternAlternatives,
    PatternBinding, PatternReference, PatternRole, PatternScope, Span,
};

pub(super) struct PatternBindings {
    span: Span,
    names: Vec<BindingName>,
    references: Vec<PatternReference>,
    alternatives: Vec<PatternAlternatives>,
}

impl PatternBindings {
    pub fn emit(self, site: BindingSite, facts: &mut Facts) {
        if site.namespace == BindingNamespace::Value {
            facts.patterns.push(PatternScope {
                span: self.span,
                scope: site.scope,
                excluded: site.excluded.clone(),
                visible_from: site.visible_from,
                references: self.references,
                alternatives: self.alternatives,
            });
        }
        site.emit(self.names, facts);
    }
}

/// Structural classification, independent of binding interpretation.
enum PatternForm<'tree, L: LanguageExt> {
    Name,
    Ignored,
    Range,
    Path,
    Literal,
    Alternatives(OrPattern<'tree, L>),
    Capture(CapturedPattern<'tree, L>),
    Rest(RestPattern<'tree, L>),
    Record(StructPattern<'tree, L>),
    Reference(RefPattern<'tree, L>),
    Sequence(SequencePattern<'tree, L>),
    Container,
}

struct Pattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

impl<'tree, L: LanguageExt> Pattern<'tree, L> {
    fn new(node: Node<'tree, StrDoc<L>>) -> Self {
        Self { node }
    }

    fn syntax(&self) -> &Node<'tree, StrDoc<L>> {
        &self.node
    }

    fn form(&self) -> PatternForm<'tree, L> {
        match self.node.kind().as_ref() {
            "identifier" | "type_identifier" | "shorthand_field_identifier" => {
                return PatternForm::Name;
            }
            "_" | "mutable_specifier" => return PatternForm::Ignored,
            "range_pattern" => return PatternForm::Range,
            "scoped_identifier" => return PatternForm::Path,
            "integer_literal" | "float_literal" | "negative_literal" | "string_literal"
            | "raw_string_literal" | "char_literal" | "boolean_literal" => {
                return PatternForm::Literal;
            }
            _ => {}
        }
        if let Some(view) = SequencePattern::cast(self.node.clone()) {
            return PatternForm::Sequence(view);
        }
        if let Some(view) = OrPattern::cast(self.node.clone()) {
            return PatternForm::Alternatives(view);
        }
        if let Some(view) = CapturedPattern::cast(self.node.clone()) {
            return PatternForm::Capture(view);
        }
        if let Some(view) = RestPattern::cast(self.node.clone()) {
            return PatternForm::Rest(view);
        }
        if let Some(view) = StructPattern::cast(self.node.clone()) {
            return PatternForm::Record(view);
        }
        if let Some(view) = RefPattern::cast(self.node.clone()) {
            return PatternForm::Reference(view);
        }
        PatternForm::Container
    }

    fn mode(&self) -> BindingMode {
        if let Some(field) = FieldPattern::cast(self.node.clone()) {
            return field.mode();
        }
        BindingMode {
            by_ref: self.node.kind() == "ref_pattern",
            mutable: self.node.kind() == "mut_pattern"
                && self
                    .node
                    .children()
                    .any(|part| part.kind() == "mutable_specifier"),
        }
    }
}

pub(super) struct PatternNavigation<'tree, 'g, L: LanguageExt> {
    pattern: Pattern<'tree, L>,
    declaration: Node<'tree, StrDoc<L>>,
    grammar: &'g Grammar,
    depth: usize,
}

impl<'tree, 'g, L: LanguageExt> PatternNavigation<'tree, 'g, L> {
    pub fn new(
        node: Node<'tree, StrDoc<L>>,
        declaration: Node<'tree, StrDoc<L>>,
        grammar: &'g Grammar,
    ) -> Self {
        Self {
            pattern: Pattern::new(node),
            declaration,
            grammar,
            depth: 0,
        }
    }
    /// Inspect the entire pattern before publishing any possible binding names.
    pub fn bindings(&self) -> Result<PatternBindings, UnsupportedSyntax> {
        if self.depth >= 128 {
            return Err(UnsupportedSyntax::at(self.pattern.syntax()));
        }
        let mut pending = vec![self.pattern.syntax().clone()];
        let mut names = Vec::new();
        let mut references = Vec::new();
        let mut alternatives_constraints = Vec::new();
        while let Some(node) = pending.pop() {
            UnsupportedSyntax::validate(&node)?;
            let form = Pattern::new(node.clone()).form();
            if matches!(form, PatternForm::Name) {
                let mut name = BindingName::from_node(&node, &self.declaration, self.grammar);
                name.explicit |= node
                    .ancestors()
                    .take_while(|ancestor| ancestor.range() != self.declaration.range())
                    .any(|ancestor| Pattern::new(ancestor).mode().by_ref);
                name.explicit |= node
                    .parent()
                    .and_then(CapturedPattern::cast)
                    .is_some_and(|capture| capture.captures(&node));
                if let Some(reference) = self.reference(&node, PatternRole::Identifier, None) {
                    references.push(reference);
                }
                names.push(name);
                continue;
            }
            if matches!(form, PatternForm::Ignored) {
                continue;
            }
            if matches!(form, PatternForm::Range) {
                for endpoint in node.children().filter(Node::is_named) {
                    if let Some(reference) = self.reference(&endpoint, PatternRole::Constant, None)
                    {
                        references.push(reference);
                    }
                }
            } else if matches!(form, PatternForm::Path)
                && let Some(reference) = self.reference(&node, PatternRole::Identifier, None)
            {
                references.push(reference);
            }
            if matches!(
                form,
                PatternForm::Literal | PatternForm::Range | PatternForm::Path
            ) {
                if node
                    .dfs()
                    .any(|child| child.is_error() || child.is_missing())
                {
                    return Err(UnsupportedSyntax::at(&node));
                }
                continue;
            }
            if let PatternForm::Alternatives(view) = form {
                let alternatives = view.alternatives()?;
                let mut branches = Vec::new();
                for alternative in alternatives {
                    let branch = Self {
                        pattern: Pattern::new(alternative),
                        declaration: self.declaration.clone(),
                        grammar: self.grammar,
                        depth: self.depth + 1,
                    };
                    let bindings = branch.bindings()?;
                    branches.push(branch.signature(&bindings.names)?);
                    names.extend(bindings.names);
                    references.extend(bindings.references);
                    alternatives_constraints.extend(bindings.alternatives);
                }
                alternatives_constraints.push(PatternAlternatives { branches });
                continue;
            }
            if let PatternForm::Capture(view) = form {
                let parts = view.parts()?;
                pending.push(parts.pattern);
                pending.push(parts.binding);
                continue;
            }
            if let PatternForm::Rest(view) = form {
                if !view.valid_position() {
                    return Err(UnsupportedSyntax::at(&node));
                }
                continue;
            }
            if let PatternForm::Record(view) = form {
                let head = view
                    .constructor()
                    .map_err(|_| UnsupportedSyntax::at(view.syntax()))?;
                if let Some(reference) = self.reference(
                    &head,
                    PatternRole::Constructor,
                    Some(ConstructorShape::Record),
                ) {
                    references.push(reference);
                }
                let mut bindings = Vec::new();
                for field in view.fields()? {
                    if let StructField::Binding(field) = field? {
                        bindings.push(field.binding()?);
                    }
                }
                pending.extend(bindings.into_iter().rev());
                continue;
            }
            if let PatternForm::Reference(view) = form {
                pending.push(view.inner()?);
                continue;
            }
            // Typed closure parameters have their own parameter binding occurrence.
            if self
                .grammar
                .bindings
                .iter()
                .any(|rule| rule.node == node.kind() && rule.name == Some("pattern"))
            {
                continue;
            }
            if !self
                .grammar
                .pattern_containers
                .contains(&node.kind().as_ref())
            {
                return Err(UnsupportedSyntax::at(&node));
            }
            if let PatternForm::Sequence(view) = form {
                let constructor = view
                    .constructor()
                    .map_err(|_| UnsupportedSyntax::at(view.syntax()))?;
                if let Some(head) = &constructor {
                    if head.dfs().any(|part| part.is_error() || part.is_missing()) {
                        return Err(UnsupportedSyntax::at(head));
                    }
                    if let Some(reference) = self.reference(
                        head,
                        PatternRole::Constructor,
                        Some(ConstructorShape::Tuple),
                    ) {
                        references.push(reference);
                    }
                }
                let children = view.elements()?.collect::<Result<Vec<_>, _>>()?;
                pending.extend(children.into_iter().rev());
                continue;
            }
            let constructor = self
                .grammar
                .pattern_constructors
                .iter()
                .find(|(kind, _)| *kind == node.kind())
                .map(|(_, field)| {
                    node.field(field)
                        .ok_or_else(|| UnsupportedSyntax::at(&node))
                })
                .transpose()?;
            if let Some(head) = &constructor
                && head
                    .dfs()
                    .any(|child| child.is_error() || child.is_missing())
            {
                return Err(UnsupportedSyntax::at(head));
            }
            if let Some(head) = &constructor
                && let Some(reference) = self.reference(
                    head,
                    PatternRole::Constructor,
                    Some(ConstructorShape::Tuple),
                )
            {
                references.push(reference);
            }
            let children: Vec<_> = node
                .children()
                .filter(Node::is_named)
                .filter(|child| {
                    constructor
                        .as_ref()
                        .is_none_or(|head| head.range() != child.range())
                })
                .collect();
            pending.extend(children.into_iter().rev());
        }
        Ok(PatternBindings {
            span: self.pattern.syntax().range().into(),
            names,
            references,
            alternatives: alternatives_constraints,
        })
    }

    fn reference(
        &self,
        node: &Node<'tree, StrDoc<L>>,
        role: PatternRole,
        shape: Option<ConstructorShape>,
    ) -> Option<PatternReference> {
        if !matches!(
            node.kind().as_ref(),
            "identifier"
                | "type_identifier"
                | "shorthand_field_identifier"
                | "scoped_identifier"
                | "scoped_type_identifier"
        ) {
            return None;
        }
        let token = node
            .dfs()
            .filter(|part| self.grammar.identifiers.contains(&part.kind().as_ref()))
            .last()?;
        let mut spelling = node
            .dfs()
            .filter(|part| {
                self.grammar.identifiers.contains(&part.kind().as_ref())
                    || matches!(part.kind().as_ref(), "crate" | "self" | "super" | "Self")
            })
            .map(|part| part.text().into_owned())
            .collect::<Vec<_>>()
            .join("::");
        if node.text().trim_start().starts_with("::") {
            spelling.insert_str(0, "::");
        }
        Some(PatternReference {
            span: token.range().into(),
            path: PathSyntax::Scoped.parse(&spelling),
            role,
            shape,
        })
    }
    /// Compare explicit modes without inferring scrutinee types or match ergonomics.
    fn signature(
        &self,
        bindings: &[BindingName],
    ) -> Result<Vec<PatternBinding>, UnsupportedSyntax> {
        let by_span: std::collections::BTreeMap<_, _> = bindings
            .iter()
            .map(|binding| (binding.span, binding.name.as_str()))
            .collect();
        let modes = self.pattern.syntax().dfs().filter_map(|node| {
            let name = by_span.get(&vvv_core::Span::from(node.range()))?;
            let mut by_ref = false;
            let mut mutable = false;
            for owner in node
                .ancestors()
                .take_while(|owner| owner.range() != self.declaration.range())
            {
                let mode = Pattern::new(owner).mode();
                by_ref |= mode.by_ref;
                mutable |= mode.mutable;
            }
            Some((
                (*name).to_owned(),
                Span::from(node.range()),
                (by_ref, mutable),
            ))
        });
        let mut signature = std::collections::BTreeMap::new();
        let mut sites = Vec::new();
        for (name, span, mode) in modes {
            sites.push(PatternBinding {
                name: name.clone(),
                span,
                by_ref: mode.0,
                mutable: mode.1,
            });
            if signature
                .insert(name, mode)
                .is_some_and(|previous| previous != mode)
            {
                return Err(UnsupportedSyntax::at(self.pattern.syntax()));
            }
        }
        Ok(sites)
    }
}

/// Direct pattern children; wildcard tokens are included only where required.
struct PatternChildren<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
    index: usize,
    excluded: Option<Span>,
    wildcard: bool,
}

impl<'tree, L: LanguageExt> PatternChildren<'tree, L> {
    fn new(node: Node<'tree, StrDoc<L>>, excluded: Option<Span>, wildcard: bool) -> Self {
        Self {
            node,
            index: 0,
            excluded,
            wildcard,
        }
    }
}

impl<'tree, L: LanguageExt> Iterator for PatternChildren<'tree, L> {
    type Item = Node<'tree, StrDoc<L>>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let child = self.node.child(self.index)?;
            self.index += 1;
            if (child.is_named() || (self.wildcard && child.kind() == "_"))
                && self
                    .excluded
                    .is_none_or(|span| span != Span::from(child.range()))
            {
                return Some(child);
            }
        }
    }
}

struct OrPattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl OrPattern {
        kinds: ["or_pattern"],
        required: {},
        optional: {}
    }
}

impl<'tree, L: LanguageExt> OrPattern<'tree, L> {
    fn alternatives(&self) -> Result<PatternChildren<'tree, L>, UnsupportedSyntax> {
        let children = PatternChildren::new(self.syntax().clone(), None, true);
        if self
            .syntax()
            .children()
            .any(|node| node.is_named() || node.kind() == "_")
        {
            Ok(children)
        } else {
            Err(UnsupportedSyntax::at(self.syntax()))
        }
    }
}

struct CapturedPattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl CapturedPattern {
        kinds: ["captured_pattern"],
        required: {},
        optional: {}
    }
}

struct CaptureParts<'tree, L: LanguageExt> {
    binding: Node<'tree, StrDoc<L>>,
    pattern: Node<'tree, StrDoc<L>>,
}

impl<'tree, L: LanguageExt> CapturedPattern<'tree, L> {
    fn parts(&self) -> Result<CaptureParts<'tree, L>, UnsupportedSyntax> {
        let mut children = PatternChildren::new(self.syntax().clone(), None, true);
        let binding = children
            .next()
            .ok_or_else(|| UnsupportedSyntax::at(self.syntax()))?;
        let pattern = children
            .next()
            .ok_or_else(|| UnsupportedSyntax::at(self.syntax()))?;
        if binding.kind() != "identifier" || children.next().is_some() {
            return Err(UnsupportedSyntax::at(self.syntax()));
        }
        Ok(CaptureParts { binding, pattern })
    }

    fn contains_rest(&self) -> bool {
        self.syntax()
            .children()
            .any(|part| part.kind() == "remaining_field_pattern")
    }

    fn captures(&self, name: &Node<'tree, StrDoc<L>>) -> bool {
        self.parts()
            .is_ok_and(|parts| parts.binding.range() == name.range())
    }
}

struct RestPattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl RestPattern {
        kinds: ["remaining_field_pattern"],
        required: {},
        optional: {}
    }
}

impl<'tree, L: LanguageExt> RestPattern<'tree, L> {
    fn valid_position(&self) -> bool {
        self.syntax().parent().is_some_and(|parent| {
            SequencePattern::cast(parent.clone()).is_some()
                || (CapturedPattern::cast(parent.clone()).is_some()
                    && parent
                        .parent()
                        .is_some_and(|owner| SlicePattern::cast(owner).is_some()))
        })
    }
}

/// Sequence element validation is lazy and retains no child vector.
struct SequenceElements<'tree, L: LanguageExt> {
    children: PatternChildren<'tree, L>,
    rest: bool,
}

impl<'tree, L: LanguageExt> SequenceElements<'tree, L> {
    fn new(node: Node<'tree, StrDoc<L>>, excluded: Option<Span>) -> Self {
        Self {
            children: PatternChildren::new(node, excluded, false),
            rest: false,
        }
    }
}

impl<'tree, L: LanguageExt> Iterator for SequenceElements<'tree, L> {
    type Item = Result<Node<'tree, StrDoc<L>>, UnsupportedSyntax>;
    fn next(&mut self) -> Option<Self::Item> {
        let child = self.children.next()?;
        let rest = child.kind() == "remaining_field_pattern"
            || CapturedPattern::cast(child.clone()).is_some_and(|capture| capture.contains_rest());
        if rest && self.rest {
            return Some(Err(UnsupportedSyntax::at(&self.children.node)));
        }
        self.rest |= rest;
        Some(Ok(child))
    }
}

struct TuplePattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl TuplePattern {
        kinds: ["tuple_pattern"],
        required: {},
        optional: {}
    }
}

impl<'tree, L: LanguageExt> TuplePattern<'tree, L> {
    fn elements(&self) -> SequenceElements<'tree, L> {
        SequenceElements::new(self.syntax().clone(), None)
    }
}

struct TupleStructPattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl TupleStructPattern {
        kinds: ["tuple_struct_pattern"],
        required: {
            constructor: "type"
        },
        optional: {}
    }
}

impl<'tree, L: LanguageExt> TupleStructPattern<'tree, L> {
    fn elements(&self) -> Result<SequenceElements<'tree, L>, UnsupportedSyntax> {
        let head = self
            .constructor()
            .map_err(|_| UnsupportedSyntax::at(self.syntax()))?;
        Ok(SequenceElements::new(
            self.syntax().clone(),
            Some(head.range().into()),
        ))
    }
}

struct SlicePattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl SlicePattern {
        kinds: ["slice_pattern"],
        required: {},
        optional: {}
    }
}

impl<'tree, L: LanguageExt> SlicePattern<'tree, L> {
    fn elements(&self) -> SequenceElements<'tree, L> {
        SequenceElements::new(self.syntax().clone(), None)
    }
}

enum SequencePattern<'tree, L: LanguageExt> {
    Tuple(TuplePattern<'tree, L>),
    Constructor(TupleStructPattern<'tree, L>),
    Slice(SlicePattern<'tree, L>),
}

impl<'tree, L: LanguageExt> SequencePattern<'tree, L> {
    fn cast(node: Node<'tree, StrDoc<L>>) -> Option<Self> {
        match node.kind().as_ref() {
            "tuple_pattern" => TuplePattern::cast(node).map(Self::Tuple),
            "tuple_struct_pattern" => TupleStructPattern::cast(node).map(Self::Constructor),
            "slice_pattern" => SlicePattern::cast(node).map(Self::Slice),
            _ => None,
        }
    }

    fn syntax(&self) -> &Node<'tree, StrDoc<L>> {
        match self {
            Self::Tuple(view) => view.syntax(),
            Self::Constructor(view) => view.syntax(),
            Self::Slice(view) => view.syntax(),
        }
    }

    fn constructor(&self) -> Result<Option<Node<'tree, StrDoc<L>>>, UnsupportedSyntax> {
        match self {
            Self::Constructor(view) => view
                .constructor()
                .map(Some)
                .map_err(|_| UnsupportedSyntax::at(view.syntax())),
            _ => Ok(None),
        }
    }

    fn elements(&self) -> Result<SequenceElements<'tree, L>, UnsupportedSyntax> {
        match self {
            Self::Tuple(view) => Ok(view.elements()),
            Self::Constructor(view) => view.elements(),
            Self::Slice(view) => Ok(view.elements()),
        }
    }
}

struct RefPattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl RefPattern {
        kinds: ["ref_pattern"],
        required: {},
        optional: {}
    }
}

impl<'tree, L: LanguageExt> RefPattern<'tree, L> {
    fn inner(&self) -> Result<Node<'tree, StrDoc<L>>, UnsupportedSyntax> {
        let mut children = self
            .syntax()
            .children()
            .filter(Node::is_named)
            .filter(|child| child.kind() != "mutable_specifier");
        let inner = children
            .next()
            .ok_or_else(|| UnsupportedSyntax::at(self.syntax()))?;
        if children.next().is_some() {
            return Err(UnsupportedSyntax::at(self.syntax()));
        }
        Ok(inner)
    }
}

/// A field exposes its label and value separately, including shorthand bindings.
pub(super) struct FieldPattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl FieldPattern {
        kinds: ["field_pattern"],
        required: {
            label: "name"
        },
        optional: {
            pattern: "pattern"
        }
    }
}
#[derive(Default)]
struct BindingMode {
    by_ref: bool,
    mutable: bool,
}

impl<'tree, L: LanguageExt> FieldPattern<'tree, L> {
    /// An explicitly named field label is distinct from its binding pattern.
    /// Retain unfinished captures so lexical coverage excludes the same tokens.
    pub fn explicit_label(&self) -> Option<Node<'tree, StrDoc<L>>> {
        self.pattern()
            .ok()
            .flatten()
            .or_else(|| self.syntax().field("pattern"))?;
        self.label().ok().or_else(|| self.syntax().field("name"))
    }

    fn binding(&self) -> Result<Node<'tree, StrDoc<L>>, UnsupportedSyntax> {
        let label = self
            .label()
            .map_err(|_| UnsupportedSyntax::at(self.syntax()))?;
        if let Some(pattern) = self
            .pattern()
            .map_err(|_| UnsupportedSyntax::at(self.syntax()))?
        {
            Ok(pattern)
        } else if label.kind() == "shorthand_field_identifier" {
            Ok(label)
        } else {
            Err(UnsupportedSyntax::at(self.syntax()))
        }
    }

    fn mode(&self) -> BindingMode {
        let mut mode = BindingMode::default();
        for child in self.syntax().children() {
            mode.by_ref |= child.kind() == "ref";
            mode.mutable |= child.kind() == "mutable_specifier";
        }
        mode
    }
}

struct StructPattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl StructPattern {
        kinds: ["struct_pattern"],
        required: {
            constructor: "type"
        },
        optional: {}
    }
}

enum StructField<'tree, L: LanguageExt> {
    Binding(FieldPattern<'tree, L>),
    Rest,
}

struct StructFields<'tree, L: LanguageExt> {
    children: PatternChildren<'tree, L>,
    rest: bool,
}

impl<'tree, L: LanguageExt> StructPattern<'tree, L> {
    fn fields(&self) -> Result<StructFields<'tree, L>, UnsupportedSyntax> {
        let head = self
            .constructor()
            .map_err(|_| UnsupportedSyntax::at(self.syntax()))?;
        if head.dfs().any(|node| node.is_error() || node.is_missing()) {
            return Err(UnsupportedSyntax::at(&head));
        }
        Ok(StructFields {
            children: PatternChildren::new(self.syntax().clone(), Some(head.range().into()), false),
            rest: false,
        })
    }
}

impl<'tree, L: LanguageExt> Iterator for StructFields<'tree, L> {
    type Item = Result<StructField<'tree, L>, UnsupportedSyntax>;
    fn next(&mut self) -> Option<Self::Item> {
        let child = self.children.next()?;
        Some(UnsupportedSyntax::validate(&child).and_then(|()| {
            if self.rest {
                return Err(UnsupportedSyntax::at(&child));
            }
            if child.kind() == "remaining_field_pattern" {
                self.rest = true;
                Ok(StructField::Rest)
            } else {
                FieldPattern::cast(child.clone())
                    .map(StructField::Binding)
                    .ok_or_else(|| UnsupportedSyntax::at(&child))
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::rust::Rust;
    use vvv_core::{Language, Span, SymbolKind};

    #[test]
    fn struct_fields_publish_only_bindings_and_keep_markers_per_name() {
        let source = "fn f(point: Point) { let crate::Point { plain, ref borrowed, mut changed, x: renamed, y: ref other, z: ref mut writable, nested: Some((left, right)), .. } = point; }";
        let facts = Rust::new().facts(source).unwrap();
        let bindings: Vec<_> = facts
            .lexical
            .iter()
            .filter(|binding| binding.symbol.kind == SymbolKind::Variable)
            .map(|binding| (binding.symbol.name.as_str(), binding.explicit))
            .collect();
        assert_eq!(
            bindings,
            [
                ("plain", false),
                ("borrowed", true),
                ("changed", true),
                ("renamed", false),
                ("other", true),
                ("writable", true),
                ("left", false),
                ("right", false)
            ]
        );
        for label in ["x:", "y:", "z:", "nested:"] {
            let start = source.find(label).unwrap();
            let span = Span::new(start, start + label.len() - 1);
            assert!(!facts.lexical_tokens.contains(&span), "{label}");
            assert!(!facts.navigation.contains(&span), "{label}");
        }
        assert!(
            !facts
                .lexical
                .iter()
                .any(|binding| matches!(binding.symbol.name.as_str(), "Point" | "Some" | "nested"))
        );
    }

    #[test]
    fn every_binding_owner_accepts_struct_patterns() {
        for source in [
            "fn f(Point { x }: Point) { x; }",
            "fn f() { let closure = |Point { x }: Point| x; }",
            "fn f() { let Point { x } = input; x; }",
            "fn f() { let Some(Point { x }) = input else { return; }; x; }",
            "fn f() { if let Some(Point { x }) = input { x; } }",
            "fn f() { match input { Point { x } => x } }",
            "fn f() { for Point { x } in input { x; } }",
            "fn f() { while let Some(Point { x }) = input { x; } }",
        ] {
            let facts = Rust::new().facts(source).unwrap();
            let binding = facts
                .lexical
                .iter()
                .find(|binding| binding.symbol.name == "x")
                .unwrap_or_else(|| panic!("{source}"));
            assert_eq!(
                &source[binding.symbol.name_span.start..binding.symbol.name_span.end],
                "x"
            );
            let use_start = source.rfind('x').unwrap();
            assert!(
                facts
                    .lexical_tokens
                    .contains(&Span::new(use_start, use_start + 1)),
                "{source}"
            );
        }
    }

    #[test]
    fn unsupported_or_malformed_field_discards_all_binding_names() {
        for pattern in [
            "Point { x: prefix, y: pattern!() }",
            "Point { x: prefix, y: [.., ..] }",
            "Point { x: prefix, y: }",
        ] {
            let source = format!("fn f() {{ let {pattern} = input; prefix; }}");
            let facts = Rust::new().facts(&source).unwrap();
            assert!(
                !facts
                    .lexical
                    .iter()
                    .any(|binding| binding.symbol.kind == SymbolKind::Variable),
                "{source}"
            );
            let start = source.rfind("prefix").unwrap();
            assert!(
                !facts.lexical_tokens.contains(&Span::new(start, start + 6)),
                "{source}"
            );
        }
    }

    #[test]
    fn alternatives_captures_literals_ranges_and_rest_keep_exact_sites() {
        for (pattern, expected) in [
            ("Some(value) | Other(value)", vec!["value", "value"]),
            ("whole @ Some(inner)", vec!["whole", "inner"]),
            ("[first, tail @ .., last]", vec!["first", "tail", "last"]),
            ("(first, .., last)", vec!["first", "last"]),
            (
                "Some((value, 0 | 1..=9)) | Other((value, -1))",
                vec!["value", "value"],
            ),
            ("Point { x: whole @ _, y: 0, .. }", vec!["whole"]),
            (
                "(true, 'x', \"text\", r\"raw\", -2, 1.5, value)",
                vec!["value"],
            ),
            ("0..=LIMIT", vec![]),
        ] {
            let source = format!("fn f() {{ match input {{ {pattern} => 0, _ => 1 }} }}");
            let facts = Rust::new().facts(&source).unwrap();
            let names: Vec<_> = facts
                .lexical
                .iter()
                .filter(|binding| binding.symbol.kind == SymbolKind::Variable)
                .map(|binding| binding.symbol.name.as_str())
                .collect();
            assert_eq!(names, expected, "{pattern}");
        }
    }

    #[test]
    fn incompatible_alternatives_and_invalid_rest_publish_nothing() {
        for pattern in [
            "(ref value, mut value) | (mut value, mut value)",
            "(prefix, [.., ..])",
            "(prefix, tail @ ..)",
            "Point { x: prefix, .., y }",
            "(prefix, pattern!())",
        ] {
            let source = format!("fn f() {{ match input {{ {pattern} => 0, _ => 1 }} }}");
            let facts = Rust::new().facts(&source).unwrap();
            assert!(
                !facts
                    .lexical
                    .iter()
                    .any(|binding| binding.symbol.kind == SymbolKind::Variable),
                "{pattern}"
            );
        }
    }

    #[test]
    fn captures_work_for_all_owners_without_escaping_failure_or_nested_items() {
        for source in [
            "fn f(whole @ (inner,): (usize,)) { inner; }",
            "fn f() { let closure = |whole @ (inner,): (usize,)| inner; }",
            "fn f() { let whole @ (inner,) = input; inner; }",
            "fn f() { let whole @ Some(inner) = input else { return; }; inner; }",
            "fn f() { if let whole @ Some(inner) = input { inner; } }",
            "fn f() { match input { whole @ Some(inner) => inner, _ => 0 } }",
            "fn f() { for whole @ (inner,) in input { inner; } }",
            "fn f() { while let whole @ Some(inner) = input { inner; } }",
        ] {
            let facts = Rust::new().facts(source).unwrap();
            assert!(
                facts
                    .lexical
                    .iter()
                    .any(|binding| binding.symbol.name == "whole" && binding.explicit),
                "{source}"
            );
            assert!(
                facts
                    .lexical
                    .iter()
                    .any(|binding| binding.symbol.name == "inner"),
                "{source}"
            );
        }
        let source = "fn f() { if let whole @ Some(inner) = input { inner; fn nested() { inner; } } else { inner; } inner; }";
        let facts = Rust::new().facts(source).unwrap();
        let binding = facts
            .lexical
            .iter()
            .find(|binding| binding.symbol.name == "inner")
            .unwrap();
        let outside = source.rfind("inner;").unwrap();
        assert!(!binding.scope.contains(&Span::new(outside, outside + 5)));
        let nested = source.find("nested() { inner").unwrap() + "nested() { ".len();
        assert!(
            binding
                .excluded
                .iter()
                .any(|span| span.contains(&Span::new(nested, nested + 5)))
        );
    }

    #[test]
    fn alternative_depth_is_bounded_without_publishing_a_prefix() {
        let pattern = std::iter::repeat_n("Some(value)", 130)
            .collect::<Vec<_>>()
            .join(" | ");
        let source = format!("fn f() {{ match input {{ {pattern} => value, _ => 0 }} }}");
        let facts = Rust::new().facts(&source).unwrap();
        assert!(
            !facts
                .lexical
                .iter()
                .any(|binding| binding.symbol.name == "value")
        );
    }

    #[test]
    fn alternative_constraints_are_deferred_without_losing_exact_sites() {
        let source =
            "fn f() { match input { MIN | MAX => 0, Some(left) | Other(right) => 0, _ => 1 } }";
        let facts = Rust::new().facts(source).unwrap();
        let alternatives: Vec<_> = facts
            .patterns
            .iter()
            .flat_map(|pattern| &pattern.alternatives)
            .collect();
        assert_eq!(alternatives.len(), 2);
        assert_eq!(alternatives[0].branches[0][0].name, "MIN");
        assert_eq!(alternatives[0].branches[1][0].name, "MAX");
        assert_eq!(alternatives[1].branches[0][0].name, "left");
        assert_eq!(alternatives[1].branches[1][0].name, "right");
    }

    #[test]
    fn range_names_never_resolve_as_lexical_variables() {
        let source =
            "fn f(LIMIT: usize) { match input { value @ 0..=LIMIT => value, _ => LIMIT } }";
        let facts = Rust::new().facts(source).unwrap();
        let start = source.find("LIMIT =>").unwrap();
        assert!(!facts.lexical_tokens.contains(&Span::new(start, start + 5)));
        assert!(facts.navigation.contains(&Span::new(start, start + 5)));
        let use_start = source.find("=> value").unwrap() + 3;
        assert!(
            facts
                .lexical_tokens
                .contains(&Span::new(use_start, use_start + 5))
        );
    }

    #[test]
    fn rust_struct_binding_modes_and_visibility_match_the_contract() {
        struct Point {
            x: usize,
            y: usize,
            z: usize,
        }
        let x = 10;
        let point = Point { x: 1, y: 2, z: 3 };
        let Point {
            x: renamed,
            ref y,
            mut z,
        } = point;
        z += renamed;
        assert_eq!(x, 10);
        assert_eq!(*y, 2);
        assert_eq!(z, 4);
        let Some(Point { x, .. }) = Some(point) else {
            return;
        };
        assert_eq!(x, 1);
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;
    use ast_grep_language::Rust;

    #[test]
    fn record_fields_separate_labels_values_shorthand_and_rest() {
        let tree = Rust.ast_grep(
            "fn f() { let crate::Point { x: Some((left, right)), ref mut y, .. } = input; }",
        );
        let record = tree.root().dfs().find_map(StructPattern::cast).unwrap();
        assert_eq!(record.constructor().unwrap().text(), "crate::Point");
        let mut fields = record.fields().unwrap();
        let StructField::Binding(renamed) = fields.next().unwrap().unwrap() else {
            panic!("expected field");
        };
        assert_eq!(renamed.label().unwrap().text(), "x");
        assert_eq!(renamed.binding().unwrap().kind(), "tuple_struct_pattern");
        assert!(!renamed.mode().by_ref);
        let StructField::Binding(shorthand) = fields.next().unwrap().unwrap() else {
            panic!("expected field");
        };
        assert_eq!(shorthand.binding().unwrap().text(), "y");
        assert!(shorthand.mode().by_ref);
        assert!(shorthand.mode().mutable);
        assert!(matches!(fields.next().unwrap(), Ok(StructField::Rest)));
        assert!(fields.next().is_none());
    }

    #[test]
    fn sequence_views_keep_constructors_and_nested_elements_separate() {
        let tree = Rust.ast_grep("fn f() { let Some((left, [first, tail @ .., last])) = input; }");
        let constructor = tree
            .root()
            .dfs()
            .find_map(TupleStructPattern::cast)
            .unwrap();
        assert_eq!(constructor.constructor().unwrap().text(), "Some");
        let elements = constructor
            .elements()
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(elements.len(), 1);
        let tuple = TuplePattern::cast(elements[0].clone()).unwrap();
        let elements = tuple.elements().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0].text(), "left");
        let slice = SlicePattern::cast(elements[1].clone()).unwrap();
        let elements = slice.elements().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(elements.len(), 3);
        let capture = CapturedPattern::cast(elements[1].clone()).unwrap();
        let parts = capture.parts().unwrap();
        assert_eq!(parts.binding.text(), "tail");
        assert_eq!(parts.pattern.kind(), "remaining_field_pattern");
        assert!(RestPattern::cast(parts.pattern).unwrap().valid_position());
    }

    #[test]
    fn alternatives_preserve_nested_branches_and_wildcards() {
        let tree = Rust.ast_grep("fn f() { match input { Some(value) | Other(value) | _ => 0 } }");
        let pattern = tree.root().dfs().find_map(OrPattern::cast).unwrap();
        let alternatives: Vec<_> = pattern.alternatives().unwrap().collect();
        assert_eq!(alternatives.len(), 2);
        assert_eq!(alternatives[0].kind(), "or_pattern");
        assert_eq!(alternatives[1].kind(), "_");
        let nested = OrPattern::cast(alternatives[0].clone()).unwrap();
        assert_eq!(nested.alternatives().unwrap().count(), 2);
    }

    #[test]
    fn captures_keep_wildcard_targets_and_reference_patterns_keep_their_inner_form() {
        let tree =
            Rust.ast_grep("fn f() { match input { whole @ _ => 0, Some(ref mut value) => 1 } }");
        let capture = tree.root().dfs().find_map(CapturedPattern::cast).unwrap();
        let parts = capture.parts().unwrap();
        assert_eq!(parts.binding.text(), "whole");
        assert_eq!(parts.pattern.kind(), "_");
        let reference = tree.root().dfs().find_map(RefPattern::cast).unwrap();
        assert!(
            reference
                .inner()
                .ok()
                .unwrap()
                .dfs()
                .any(|node| node.kind() == "identifier" && node.text() == "value")
        );
    }

    #[test]
    fn sequence_rest_validation_includes_captured_rests_and_checks_their_owner() {
        for source in [
            "fn f() { let [head, tail @ .., ..] = input; }",
            "fn f() { let (head, .., ..) = input; }",
        ] {
            let tree = Rust.ast_grep(source);
            let pattern = tree.root().dfs().find_map(SequencePattern::cast).unwrap();
            assert!(
                pattern
                    .elements()
                    .ok()
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .is_err()
            );
        }
        let tree = Rust.ast_grep("fn f() { let (head, tail @ ..) = input; }");
        let rest = tree.root().dfs().find_map(RestPattern::cast).unwrap();
        assert!(!rest.valid_position());
    }

    #[test]
    fn record_fields_reject_any_field_after_rest() {
        let tree = Rust.ast_grep("fn f() { let Point { x, .., y } = input; }");
        let record = tree.root().dfs().find_map(StructPattern::cast).unwrap();
        let mut fields = record.fields().unwrap();
        assert!(matches!(
            fields.next().unwrap(),
            Ok(StructField::Binding(_))
        ));
        assert!(matches!(fields.next().unwrap(), Ok(StructField::Rest)));
        assert!(fields.next().unwrap().is_err());
    }
}
