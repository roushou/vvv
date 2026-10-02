//! TypeScript and TSX structural forms retain their distinct parameter/body shapes.

use super::views::{CallableView, DirectChildren, SyntaxError, syntax_view};
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{CallKind, Grammar};

pub(super) struct Function<'tree, L: ast_grep_core::tree_sitter::LanguageExt> {
    node: ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>,
}

syntax_view! { impl Function {
    kinds: ["function_declaration", "function_signature", "generator_function_declaration", "method_definition", "method_signature", "abstract_method_signature", "arrow_function", "function_expression", "generator_function"],
    required: {}, optional: {name: "name", parameters: "parameters", parameter: "parameter", body: "body"},
} }
pub(super) enum Parameters<'tree, L: LanguageExt> {
    List(DirectChildren<'tree, L>),
    Single(Node<'tree, StrDoc<L>>),
    Absent,
}

pub(super) enum Body<'tree, L: LanguageExt> {
    Block(Block<'tree, L>),
    Expression(Node<'tree, StrDoc<L>>),
    Absent,
}

impl<'tree, L: LanguageExt> Function<'tree, L> {
    pub fn parameter_shape(&self) -> Result<Parameters<'tree, L>, SyntaxError> {
        Ok(if let Some(list) = self.parameters()? {
            Parameters::List(DirectChildren::new(list, &["comment"]))
        } else if let Some(single) = self.parameter()? {
            Parameters::Single(single)
        } else {
            Parameters::Absent
        })
    }

    pub fn body_shape(&self) -> Result<Body<'tree, L>, SyntaxError> {
        Ok(match self.body()? {
            Some(node) => match Block::cast(node.clone()) {
                Some(block) => Body::Block(block),
                None => Body::Expression(node),
            },
            None => Body::Absent,
        })
    }
    // Validate the structural forms without broadening lexical coverage.
    pub fn complete_fields(&self) -> bool {
        let parameters = match self.parameter_shape() {
            Ok(Parameters::List(mut children)) => {
                children.all(|child| !child.is_error() && !child.is_missing())
            }
            Ok(Parameters::Single(node)) => !node.is_error() && !node.is_missing(),
            Ok(Parameters::Absent) => true,
            Err(_) => false,
        };
        parameters
            && match self.body_shape() {
                Ok(Body::Block(block)) => !block.syntax().is_missing(),
                Ok(Body::Expression(node)) => !node.is_missing(),
                Ok(Body::Absent) => true,
                Err(_) => false,
            }
    }
}

impl<'tree, L: LanguageExt> CallableView<'tree, L> for Function<'tree, L> {
    fn syntax(&self) -> &Node<'tree, StrDoc<L>> {
        self.syntax()
    }

    fn anonymous(&self) -> bool {
        matches!(
            self.syntax().kind().as_ref(),
            "arrow_function" | "function_expression" | "generator_function"
        )
    }
}

pub(super) struct Block<'tree, L: ast_grep_core::tree_sitter::LanguageExt> {
    node: ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>,
}

syntax_view! {
    impl Block {
        kinds: ["statement_block"],
        required: {},
        optional: {}
    }
}

pub(super) struct Call<'tree, L: ast_grep_core::tree_sitter::LanguageExt> {
    node: ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>,
}

syntax_view! {
    impl Call {
        kinds: ["call_expression"],
        required: {
            function: "function"
        },
        optional: {}
    }
}

pub(super) struct MemberAccess<'tree, L: ast_grep_core::tree_sitter::LanguageExt> {
    node: ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>,
}

syntax_view! {
    impl MemberAccess {
        kinds: ["member_expression"],
        required: {
            receiver: "object",
            member: "property"
        },
        optional: {}
    }
}

impl<'tree, L: LanguageExt> Call<'tree, L> {
    pub fn callee(&self, grammar: &Grammar) -> Option<(Node<'tree, StrDoc<L>>, CallKind)> {
        let mut callee = self.function().ok()?;
        let mut kind = CallKind::Direct;
        if let Some(member) = MemberAccess::cast(callee.clone()) {
            debug_assert_eq!(member.syntax().range(), callee.range());
            let _receiver = member.receiver().ok();
            if let Ok(property) = member.member() {
                callee = property;
                kind = CallKind::Member;
            }
        }
        if !grammar.identifiers.contains(&callee.kind().as_ref()) {
            kind = CallKind::Indirect;
        }
        Some((callee, kind))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast_grep_language::{Tsx, TypeScript};

    #[test]
    fn arrows_retain_single_parameters_and_expression_bodies() {
        let tree = TypeScript.ast_grep("const f = value => value + 1;");
        let node = tree
            .root()
            .dfs()
            .find(|node| node.kind() == "arrow_function")
            .unwrap();
        let function = Function::cast(node).unwrap();
        assert!(
            matches!(function.parameter_shape().unwrap(), Parameters::Single(node) if node.text() == "value")
        );
        assert!(
            matches!(function.body_shape().unwrap(), Body::Expression(node) if node.kind() == "binary_expression")
        );
        assert!(function.anonymous());
    }

    #[test]
    fn signatures_and_tsx_methods_retain_body_and_parameter_shapes() {
        let tree = Tsx.ast_grep("declare function f(value: number): number; class C { m(value: number) { return <div/>; } }");
        let root = tree.root();
        let mut forms = root.dfs().filter_map(Function::cast);
        let signature = forms.next().unwrap();
        let Parameters::List(mut nodes) = signature.parameter_shape().unwrap() else {
            panic!("expected list");
        };
        assert_eq!(nodes.next().unwrap().kind(), "required_parameter");
        assert!(matches!(signature.body_shape().unwrap(), Body::Absent));
        let method = forms.next().unwrap();
        assert!(matches!(method.body_shape().unwrap(), Body::Block(_)));
        assert!(!method.anonymous());
    }
}

pub(super) struct Import<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl Import {
        kinds: ["import_statement"],
        required: {
            source: "source"
        },
        optional: {}
    }
}

impl<'tree, L: LanguageExt> Import<'tree, L> {
    pub fn type_only(&self) -> bool {
        self.syntax().text().starts_with("import type ")
    }
}

pub(super) struct Export<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl Export {
        kinds: ["export_statement"],
        required: {},
        optional: {
            source: "source"
        }
    }
}

impl<'tree, L: LanguageExt> Export<'tree, L> {
    pub fn type_only(&self) -> bool {
        self.syntax().text().starts_with("export type ")
    }
}

pub(super) struct NamedSpecifier<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl NamedSpecifier {
        kinds: ["import_specifier", "export_specifier"],
        required: {
            name: "name"
        },
        optional: {
            alias: "alias"
        }
    }
}

impl<'tree, L: LanguageExt> NamedSpecifier<'tree, L> {
    pub fn type_only(&self) -> bool {
        self.syntax().text().starts_with("type ")
    }
}

pub(super) struct ImportClause<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl ImportClause {
        kinds: ["import_clause", "namespace_import"],
        required: {},
        optional: {}
    }
}

impl<'tree, L: LanguageExt> ImportClause<'tree, L> {
    pub fn name(&self) -> Option<Node<'tree, StrDoc<L>>> {
        self.syntax()
            .children()
            .find(|node| node.kind() == "identifier")
    }
}

/// Source-bearing statements preserve local exports with no source.
pub(super) enum SourceStatement<'tree, L: LanguageExt> {
    Import(Import<'tree, L>),
    Export(Export<'tree, L>),
}

impl<'tree, L: LanguageExt> SourceStatement<'tree, L> {
    pub fn cast(node: Node<'tree, StrDoc<L>>) -> Option<Self> {
        match node.kind().as_ref() {
            "import_statement" => Import::cast(node).map(Self::Import),
            "export_statement" => Export::cast(node).map(Self::Export),
            _ => None,
        }
    }

    pub fn source(&self) -> Result<Option<Node<'tree, StrDoc<L>>>, SyntaxError> {
        match self {
            Self::Import(import) => import.source().map(Some),
            Self::Export(export) => export.source(),
        }
    }

    pub fn type_only(&self) -> bool {
        match self {
            Self::Import(import) => import.type_only(),
            Self::Export(export) => export.type_only(),
        }
    }
}

#[cfg(test)]
mod import_tests {
    use super::*;
    use ast_grep_language::TypeScript;

    #[test]
    fn source_statements_keep_type_only_and_local_export_distinctions() {
        let tree = TypeScript.ast_grep("import type { Item as Local } from './a'; export { Local }; export type { Item } from './a';");
        let root = tree.root();
        let statements: Vec<_> = root.dfs().filter_map(SourceStatement::cast).collect();
        assert!(statements[0].type_only());
        assert_eq!(statements[0].source().unwrap().unwrap().text(), "'./a'");
        assert!(statements[1].source().unwrap().is_none());
        assert!(!statements[1].type_only());
        assert!(statements[2].type_only());
        let specifier = root.dfs().find_map(NamedSpecifier::cast).unwrap();
        assert_eq!(specifier.name().unwrap().text(), "Item");
        assert_eq!(specifier.alias().unwrap().unwrap().text(), "Local");
    }
}

pub(super) struct Class<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl Class {
        kinds: ["class_declaration", "abstract_class_declaration"],
        required: {
            name: "name",
            body: "body"
        },
        optional: {}
    }
}

pub(super) struct Interface<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl Interface {
        kinds: ["interface_declaration"],
        required: {
            name: "name",
            body: "body"
        },
        optional: {}
    }
}

pub(super) struct Enum<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl Enum {
        kinds: ["enum_declaration"],
        required: {
            name: "name",
            body: "body"
        },
        optional: {}
    }
}

pub(super) struct NamedDeclaration<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! { impl NamedDeclaration {
    kinds: ["type_alias_declaration", "enum_assignment", "public_field_definition", "property_signature"],
    required: {name: "name"}, optional: {},
} }
pub(super) enum Declaration<'tree, L: LanguageExt> {
    Function(Function<'tree, L>),
    Class(Class<'tree, L>),
    Interface(Interface<'tree, L>),
    Enum(Enum<'tree, L>),
    Variable(VariableDeclarator<'tree, L>),
    Named(NamedDeclaration<'tree, L>),
}

impl<'tree, L: LanguageExt> Declaration<'tree, L> {
    pub fn cast(node: Node<'tree, StrDoc<L>>) -> Option<Self> {
        Some(match node.kind().as_ref() {
            "class_declaration" | "abstract_class_declaration" => Self::Class(Class::cast(node)?),
            "interface_declaration" => Self::Interface(Interface::cast(node)?),
            "enum_declaration" => Self::Enum(Enum::cast(node)?),
            "type_alias_declaration"
            | "enum_assignment"
            | "public_field_definition"
            | "property_signature" => Self::Named(NamedDeclaration::cast(node)?),
            "variable_declarator" => Self::Variable(VariableDeclarator::cast(node)?),
            _ => Self::Function(Function::cast(node)?),
        })
    }

    pub fn field(
        &self,
        field: &str,
    ) -> Option<Result<Option<Node<'tree, StrDoc<L>>>, SyntaxError>> {
        Some(match (self, field) {
            (Self::Function(view), "name") => view.name(),
            (Self::Class(view), "name") => view.name().map(Some),
            (Self::Interface(view), "name") => view.name().map(Some),
            (Self::Enum(view), "name") => view.name().map(Some),
            (Self::Named(view), "name") => view.name().map(Some),
            (Self::Variable(view), "name") => view.name().map(Some),
            (Self::Function(view), "body") => view.body(),
            (Self::Class(view), "body") => view.body().map(Some),
            (Self::Interface(view), "body") => view.body().map(Some),
            (Self::Enum(view), "body") => view.body().map(Some),
            _ => return None,
        })
    }
}

/// Parameters can carry a pattern or a property name; coverage remains rule-owned.
pub(super) struct Parameter<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl Parameter {
        kinds: ["required_parameter", "optional_parameter"],
        required: {},
        optional: {
            pattern: "pattern",
            name: "name"
        }
    }
}

pub(super) struct TypeParameter<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl TypeParameter {
        kinds: ["type_parameter"],
        required: {
            name: "name"
        },
        optional: {}
    }
}

pub(super) struct VariableDeclaration<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl VariableDeclaration {
        kinds: ["lexical_declaration", "variable_declaration"],
        required: {},
        optional: {}
    }
}

pub(super) struct VariableDeclarator<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl VariableDeclarator {
        kinds: ["variable_declarator"],
        required: {
            name: "name"
        },
        optional: {}
    }
}

pub(super) struct ArrayPattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! { impl ArrayPattern { kinds: ["array_pattern"], required: {}, optional: {}, children: {elements: []} } }
pub(super) struct ObjectPattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! { impl ObjectPattern { kinds: ["object_pattern"], required: {}, optional: {}, children: {entries: []} } }
pub(super) struct PairPattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl PairPattern {
        kinds: ["pair_pattern"],
        required: {
            key: "key",
            value: "value"
        },
        optional: {}
    }
}

pub(super) struct AssignmentPattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl AssignmentPattern {
        kinds: ["assignment_pattern", "object_assignment_pattern"],
        required: {
            left: "left",
            right: "right"
        },
        optional: {}
    }
}

pub(super) struct RestPattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! { impl RestPattern { kinds: ["rest_pattern"], required: {}, optional: {}, children: {elements: []} } }

/// Concrete forms retain syntax distinctions without implying binding support.
enum PatternForm<'tree, L: LanguageExt> {
    Array(ArrayPattern<'tree, L>),
    Object(ObjectPattern<'tree, L>),
    Pair(PairPattern<'tree, L>),
    Assignment(AssignmentPattern<'tree, L>),
    Rest(RestPattern<'tree, L>),
    Other(Node<'tree, StrDoc<L>>),
}

struct Pattern<'tree, L: LanguageExt> {
    form: PatternForm<'tree, L>,
}

impl<'tree, L: LanguageExt> Pattern<'tree, L> {
    fn new(node: Node<'tree, StrDoc<L>>) -> Self {
        let form = match node.kind().as_ref() {
            "array_pattern" => PatternForm::Array(ArrayPattern::cast(node).unwrap()),
            "object_pattern" => PatternForm::Object(ObjectPattern::cast(node).unwrap()),
            "pair_pattern" => PatternForm::Pair(PairPattern::cast(node).unwrap()),
            "assignment_pattern" | "object_assignment_pattern" => {
                PatternForm::Assignment(AssignmentPattern::cast(node).unwrap())
            }
            "rest_pattern" => PatternForm::Rest(RestPattern::cast(node).unwrap()),
            _ => PatternForm::Other(node),
        };
        Self { form }
    }

    fn syntax(&self) -> &Node<'tree, StrDoc<L>> {
        match &self.form {
            PatternForm::Array(view) => view.syntax(),
            PatternForm::Object(view) => view.syntax(),
            PatternForm::Pair(view) => view.syntax(),
            PatternForm::Assignment(view) => view.syntax(),
            PatternForm::Rest(view) => view.syntax(),
            PatternForm::Other(node) => node,
        }
    }

    fn field(&self, field: &str) -> Option<Node<'tree, StrDoc<L>>> {
        let value = match (&self.form, field) {
            (PatternForm::Pair(view), "key") => Some(view.key()),
            (PatternForm::Pair(view), "value") => Some(view.value()),
            (PatternForm::Assignment(view), "left") => Some(view.left()),
            (PatternForm::Assignment(view), "right") => Some(view.right()),
            _ => None,
        };
        value
            .and_then(Result::ok)
            .or_else(|| self.syntax().field(field))
    }

    fn children(&self) -> DirectChildren<'tree, L> {
        match &self.form {
            PatternForm::Array(view) => view.elements(),
            PatternForm::Object(view) => view.entries(),
            PatternForm::Rest(view) => view.elements(),
            _ => DirectChildren::new(self.syntax().clone(), &[]),
        }
    }
}

impl<'tree, L: LanguageExt> super::bindings::PatternView<'tree, L> for Pattern<'tree, L> {
    fn from_node(node: Node<'tree, StrDoc<L>>) -> Self {
        Self::new(node)
    }

    fn syntax(&self) -> &Node<'tree, StrDoc<L>> {
        self.syntax()
    }

    fn field(&self, field: &str) -> Option<Node<'tree, StrDoc<L>>> {
        self.field(field)
    }

    fn children(&self) -> impl Iterator<Item = Node<'tree, StrDoc<L>>> {
        self.children()
    }
}

enum BindingDeclaration<'tree, L: LanguageExt> {
    Parameter(Parameter<'tree, L>),
    TypeParameter(TypeParameter<'tree, L>),
    Variables(VariableDeclaration<'tree, L>),
    Scoped(Declaration<'tree, L>, Node<'tree, StrDoc<L>>),
}

impl<'tree, L: LanguageExt> BindingDeclaration<'tree, L> {
    fn cast(node: Node<'tree, StrDoc<L>>) -> Option<Self> {
        Some(match node.kind().as_ref() {
            "required_parameter" | "optional_parameter" => Self::Parameter(Parameter::cast(node)?),
            "type_parameter" => Self::TypeParameter(TypeParameter::cast(node)?),
            "lexical_declaration" | "variable_declaration" => {
                Self::Variables(VariableDeclaration::cast(node)?)
            }
            _ => Self::Scoped(Declaration::cast(node.clone())?, node),
        })
    }

    fn syntax(&self) -> &Node<'tree, StrDoc<L>> {
        match self {
            Self::Parameter(view) => view.syntax(),
            Self::TypeParameter(view) => view.syntax(),
            Self::Variables(view) => view.syntax(),
            Self::Scoped(_, node) => node,
        }
    }

    fn field(&self, field: &str) -> Option<Node<'tree, StrDoc<L>>> {
        let value = match (self, field) {
            (Self::Parameter(view), "pattern") => Some(view.pattern()),
            (Self::Parameter(view), "name") => Some(view.name()),
            (Self::TypeParameter(view), "name") => Some(view.name().map(Some)),
            (Self::Scoped(view, _), _) => view.field(field),
            _ => None,
        };
        value
            .and_then(Result::ok)
            .unwrap_or_else(|| self.syntax().field(field))
    }
}

pub(super) struct TypeScriptNavigation<'a, 'g> {
    shared: &'a super::navigation::NavigationFacts<'g>,
}

impl<'a, 'g> TypeScriptNavigation<'a, 'g> {
    pub fn new(shared: &'a super::navigation::NavigationFacts<'g>) -> Self {
        Self { shared }
    }

    pub fn extract<L: LanguageExt>(
        &self,
        node: &Node<'_, StrDoc<L>>,
        facts: &mut vvv_core::Facts,
        coverage: &mut super::navigation::NavigationCoverage,
    ) {
        let kind = node.kind();
        if matches!(kind.as_ref(), "function_declaration" | "method_definition")
            && let Some(function) = Function::cast(node.clone())
            && !function.complete_fields()
        {
            coverage.block(node.range().into());
        }
        let mut rules = self
            .shared
            .grammar
            .bindings
            .iter()
            .filter(|rule| rule.node == kind)
            .peekable();
        if rules.peek().is_none() {
            return;
        }
        let Some(declaration) = BindingDeclaration::cast(node.clone()) else {
            self.shared.table_bindings(node, facts, coverage);
            return;
        };
        for rule in rules {
            let Some(scope) = self.shared.scope(node, rule, coverage) else {
                continue;
            };
            let Some(pattern) = rule.name.and_then(|field| declaration.field(field)) else {
                coverage.block(scope.range().into());
                continue;
            };
            let Some(names) = super::bindings::PatternNames::new(self.shared.grammar)
                .names(&Pattern::new(pattern))
            else {
                coverage.block(scope.range().into());
                continue;
            };
            let names = names
                .iter()
                .map(|name| {
                    super::navigation::BindingName::from_node(name, node, self.shared.grammar)
                })
                .collect();
            self.shared
                .site(node, &scope, rule, coverage)
                .emit(names, facts);
        }
    }
}

#[cfg(test)]
mod binding_tests {
    use super::*;
    use ast_grep_language::{Tsx, TypeScript};

    #[test]
    fn parameter_views_preserve_optional_and_constructor_parameter_patterns() {
        let tree = TypeScript.ast_grep("function f(required: number, optional?: number) {} class C { constructor(public value: number) {} }");
        let root = tree.root();
        let mut parameters = root.dfs().filter_map(Parameter::cast);
        assert_eq!(
            parameters
                .next()
                .unwrap()
                .pattern()
                .unwrap()
                .unwrap()
                .text(),
            "required"
        );
        assert_eq!(
            parameters
                .next()
                .unwrap()
                .pattern()
                .unwrap()
                .unwrap()
                .text(),
            "optional"
        );
        let property = parameters.next().unwrap();
        assert_eq!(property.pattern().unwrap().unwrap().text(), "value");
        assert!(property.name().unwrap().is_none());
    }

    #[test]
    fn destructuring_views_keep_labels_defaults_rest_and_array_holes_distinct() {
        let tree = TypeScript.ast_grep("function f({ original: renamed, shorthand = fallback, ...rest }, [first = fallback, , ...tail]) {}");
        let root = tree.root();
        let object = root.dfs().find_map(ObjectPattern::cast).unwrap();
        let mut entries = object.entries();
        let pair = PairPattern::cast(entries.next().unwrap()).unwrap();
        assert_eq!(pair.key().unwrap().text(), "original");
        assert_eq!(pair.value().unwrap().text(), "renamed");
        let default = AssignmentPattern::cast(entries.next().unwrap()).unwrap();
        assert_eq!(default.left().unwrap().text(), "shorthand");
        assert_eq!(default.right().unwrap().text(), "fallback");
        let rest = RestPattern::cast(entries.next().unwrap()).unwrap();
        assert_eq!(rest.elements().next().unwrap().text(), "rest");
        let array = root.dfs().find_map(ArrayPattern::cast).unwrap();
        let mut elements = array.elements();
        assert!(AssignmentPattern::cast(elements.next().unwrap()).is_some());
        assert!(RestPattern::cast(elements.next().unwrap()).is_some());
        assert!(elements.next().is_none());
    }

    #[test]
    fn variable_declaration_owners_and_tsx_patterns_retain_parser_shapes() {
        let tree = Tsx.ast_grep("const { value: local } = input; let [first] = input; var last = first; const view = <div/>;");
        let root = tree.root();
        let declarations: Vec<_> = root.dfs().filter_map(VariableDeclaration::cast).collect();
        assert_eq!(declarations.len(), 4);
        assert_eq!(declarations[2].syntax().kind(), "variable_declaration");
        let names: Vec<_> = root
            .dfs()
            .filter_map(VariableDeclarator::cast)
            .map(|view| view.name().unwrap().kind().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "object_pattern",
                "array_pattern",
                "identifier",
                "identifier"
            ]
        );
    }

    #[test]
    fn shared_policy_preserves_constructor_exclusion_and_complete_pattern_publication() {
        use crate::syntax::bindings::{PatternNames, TablePattern};
        let grammar = Grammar {
            pattern_containers: &[
                "object_pattern",
                "pair_pattern",
                "array_pattern",
                "rest_pattern",
            ],
            pattern_constructors: &[("pair_pattern", "key")],
            ..Grammar::EMPTY
        };
        let tree = TypeScript.ast_grep("function f({ x: first, y: [second, ...tail] }) {}");
        let node = tree
            .root()
            .dfs()
            .find(|node| node.kind() == "object_pattern")
            .unwrap();
        let policy = PatternNames::new(&grammar);
        let typed = policy.names(&Pattern::new(node.clone())).unwrap();
        let generic = policy.names(&TablePattern::new(node)).unwrap();
        assert_eq!(
            typed
                .iter()
                .map(|node| node.text().into_owned())
                .collect::<Vec<_>>(),
            ["first", "second", "tail"]
        );
        assert_eq!(
            typed.iter().map(Node::range).collect::<Vec<_>>(),
            generic.iter().map(Node::range).collect::<Vec<_>>()
        );
        let unsupported = TypeScript.ast_grep("function f({ x: first, y: second = fallback }) {}");
        let node = unsupported
            .root()
            .dfs()
            .find(|node| node.kind() == "object_pattern")
            .unwrap();
        assert!(policy.names(&Pattern::new(node.clone())).is_none());
        assert!(policy.names(&TablePattern::new(node)).is_none());
    }

    #[test]
    fn shared_policy_does_not_duplicate_independently_lowered_parameters() {
        use crate::syntax::bindings::{PatternNames, TablePattern};
        let grammar = Grammar {
            pattern_containers: &["formal_parameters"],
            bindings: &[vvv_core::BindingRule {
                node: "required_parameter",
                name: Some("pattern"),
                scopes: &["function_declaration"],
                kind: vvv_core::SymbolKind::Parameter,
                namespace: vvv_core::BindingNamespace::Value,
                after: false,
            }],
            ..Grammar::EMPTY
        };
        let tree = TypeScript.ast_grep("function f(first: number, second: number) {}");
        let node = tree
            .root()
            .dfs()
            .find(|node| node.kind() == "formal_parameters")
            .unwrap();
        let policy = PatternNames::new(&grammar);
        assert!(
            policy
                .names(&Pattern::new(node.clone()))
                .unwrap()
                .is_empty()
        );
        assert!(policy.names(&TablePattern::new(node)).unwrap().is_empty());
    }

    #[test]
    fn checked_pattern_fields_preserve_unfinished_captures_for_rule_consumers() {
        use crate::syntax::bindings::{PatternView, TablePattern};
        let tree = TypeScript.ast_grep("const { x: } = input;");
        let node = tree
            .root()
            .dfs()
            .find(|node| node.kind() == "pair_pattern")
            .unwrap();
        let typed = Pattern::new(node.clone());
        let generic = TablePattern::new(node);
        for field in ["key", "value", "custom"] {
            assert_eq!(
                typed.field(field).map(|node| node.range()),
                generic.field(field).map(|node| node.range())
            );
        }
    }

    struct BindingCases {
        sources: &'static [&'static str],
    }

    impl BindingCases {
        fn verify<L: LanguageExt>(&self, typed: &crate::syntax::AstGrepSearcher<L>) {
            let tables = typed
                .clone()
                .with_navigation_syntax(crate::syntax::NavigationSyntax::Tables);
            for source in self.sources {
                assert_eq!(
                    format!("{:?}", typed.facts(source).unwrap()),
                    format!("{:?}", tables.facts(source).unwrap()),
                    "{source}"
                );
            }
        }
    }
    #[test]
    fn binding_interpretation_preserves_supported_names_and_conservative_barriers() {
        let cases = BindingCases {
            sources: &[
                "function f<T>(required: T, optional?: T) { required; optional; } class C<U> { m(value: U) { value; } }",
                "function f({ original: renamed }, [first], ...rest: number[]) { renamed; first; rest; }",
                "function f(value: number) { let local = value; { const other = value; other; } value; }",
                "function f(value: number) { var local = value; value; }",
                "function f(value: number) { const arrow = (inner: number) => inner + value; value; }",
                "class C { constructor(public value: number) { value; } m(value: number) { value; } }",
                "function f(value: number) { class Local {} interface I {} type Alias = number; enum E { A } value; }",
            ],
        };
        cases.verify(crate::typescript::TypeScript::default().searcher());
        cases.verify(crate::typescript::Tsx::default().searcher());
    }
}
