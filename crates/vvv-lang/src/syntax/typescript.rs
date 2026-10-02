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
    const NAVIGATION_KINDS: &'static [&'static str] = &[
        "function_declaration",
        "method_definition",
        "arrow_function",
        "function_expression",
        "generator_function_declaration",
        "generator_function",
    ];

    fn navigation_owner(&self) -> bool {
        Self::NAVIGATION_KINDS.contains(&self.syntax().kind().as_ref())
    }

    pub(super) fn complete_header(&self) -> bool {
        let Ok(Some(body)) = self.body() else {
            return false;
        };
        self.complete_fields()
            && !self
                .syntax()
                .children()
                .filter(|child| child.range() != body.range())
                .any(|child| child.dfs().any(|node| node.is_error() || node.is_missing()))
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

/// Transparent written wrappers around a callable value; no value/type inference.
pub(super) struct CallableValue<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

impl<'tree, L: LanguageExt> CallableValue<'tree, L> {
    pub fn new(node: Node<'tree, StrDoc<L>>) -> Self {
        Self { node }
    }

    pub fn function(&self) -> Option<Function<'tree, L>> {
        let mut node = self.node.clone();
        let mut steps = 0;
        for _ in 0..128 {
            if let Some(function) = Function::cast(node.clone()) {
                return function.complete_header().then_some(function);
            }
            let assertion = node.kind() == "type_assertion";
            if !matches!(
                node.kind().as_ref(),
                "parenthesized_expression"
                    | "non_null_expression"
                    | "as_expression"
                    | "satisfies_expression"
                    | "type_assertion"
            ) {
                return None;
            }
            let mut children = node
                .children()
                .filter(|child| child.is_named() && child.kind() != "comment");
            let first = children.next()?;
            let second = children.next();
            let paired = matches!(
                node.kind().as_ref(),
                "as_expression" | "satisfies_expression" | "type_assertion"
            );
            if children.next().is_some() || paired != second.is_some() {
                return None;
            }
            drop(children);
            let inner = if assertion { second? } else { first };
            for child in node
                .children()
                .filter(|child| child.range() != inner.range())
            {
                for descendant in child.dfs() {
                    steps += 1;
                    if steps > 1_024 || descendant.is_missing() || descendant.is_error() {
                        return None;
                    }
                }
            }
            node = inner;
        }
        None
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
        optional: {},
        children: {statements: ["comment"]}
    }
}

impl<'tree, L: LanguageExt> Block<'tree, L> {
    fn callable_body(&self) -> bool {
        self.syntax()
            .parent()
            .and_then(Function::cast)
            .filter(Function::navigation_owner)
            .and_then(|function| function.body().ok().flatten())
            .is_some_and(|body| body.range() == self.syntax().range())
    }
}

pub(super) struct Program<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! { impl Program {
    kinds: ["program"], required: {}, optional: {},
    children: {statements: ["comment", "hash_bang_line"]},
} }

impl<'tree, L: LanguageExt> Program<'tree, L> {
    fn module(&self) -> bool {
        self.statements().any(|statement| {
            matches!(
                statement.kind().as_ref(),
                "import_statement" | "export_statement"
            )
        })
    }
}

pub(super) struct StaticBlock<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! { impl StaticBlock {
    kinds: ["class_static_block"], required: {body: "body"}, optional: {},
} }

pub(super) struct Namespace<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! { impl Namespace {
    kinds: ["internal_module", "module"], required: {name: "name"}, optional: {body: "body"},
} }

/// A written declaration environment, independent of the declarations it contains.
struct DeclarationScope<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

impl<'tree, L: LanguageExt> DeclarationScope<'tree, L> {
    fn cast(node: Node<'tree, StrDoc<L>>) -> Option<Self> {
        matches!(
            node.kind().as_ref(),
            "program" | "statement_block" | "switch_body"
        )
        .then_some(Self { node })
    }

    fn statements(&self) -> ScopeStatements<'tree, L> {
        ScopeStatements {
            direct: Block::cast(self.node.clone()).map_or_else(
                || DirectChildren::new(self.node.clone(), &["comment", "hash_bang_line"]),
                |block| block.statements(),
            ),
            case: None,
            case_value: None,
        }
    }

    fn unwrap(mut node: Node<'tree, StrDoc<L>>) -> Node<'tree, StrDoc<L>> {
        // Export/ambient wrappers retain the underlying declaration's exact handles.
        for _ in 0..128 {
            if node.kind() == "expression_statement" {
                let namespace = node
                    .children()
                    .find(Node::is_named)
                    .and_then(Namespace::cast);
                if let Some(namespace) = namespace {
                    node = namespace.syntax().clone();
                    continue;
                }
                break;
            }
            if !matches!(
                node.kind().as_ref(),
                "export_statement" | "ambient_declaration"
            ) {
                break;
            }
            let Some(child) = node.children().find(|child| {
                child.is_named() && child.kind() != "comment" && child.kind() != "decorator"
            }) else {
                break;
            };
            node = child;
        }
        node
    }

    fn callable(&self) -> Option<Function<'tree, L>> {
        if !Block::cast(self.node.clone()).is_some_and(|block| block.callable_body()) {
            return None;
        }
        let function = self.node.parent().and_then(Function::cast)?;
        (function.navigation_owner()
            && function.body().ok().flatten()?.range() == self.node.range())
        .then_some(function)
    }

    fn var_owner(&self) -> bool {
        self.node.kind() == "program"
            || self.callable().is_some()
            || self.node.parent().is_some_and(|parent| {
                StaticBlock::cast(parent.clone()).is_some_and(|owner| {
                    owner
                        .body()
                        .is_ok_and(|body| body.range() == self.node.range())
                }) || Namespace::cast(parent).is_some_and(|owner| {
                    owner
                        .body()
                        .ok()
                        .flatten()
                        .is_some_and(|body| body.range() == self.node.range())
                })
            })
    }

    fn strict(&self, module: bool) -> bool {
        if module {
            return true;
        }
        std::iter::once(self.node.clone())
            .chain(self.node.ancestors())
            .any(|node| {
                if matches!(
                    node.kind().as_ref(),
                    "class" | "class_declaration" | "abstract_class_declaration"
                ) {
                    return true;
                }
                if let Some(scope) = Self::cast(node)
                    && (scope.node.kind() == "program" || scope.callable().is_some())
                {
                    for statement in scope.statements() {
                        if statement.kind() != "expression_statement" {
                            break;
                        }
                        let Some(expression) = statement.children().find(Node::is_named) else {
                            break;
                        };
                        if expression.kind() != "string" {
                            break;
                        }
                        if matches!(
                            expression.text().as_ref(),
                            "\"use strict\"" | "'use strict'"
                        ) {
                            return true;
                        }
                    }
                }
                false
            })
    }
}

/// Switch cases share one scope; case expressions are not declarations.
struct ScopeStatements<'tree, L: LanguageExt> {
    direct: DirectChildren<'tree, L>,
    case: Option<DirectChildren<'tree, L>>,
    case_value: Option<vvv_core::Span>,
}

impl<'tree, L: LanguageExt> Iterator for ScopeStatements<'tree, L> {
    type Item = Node<'tree, StrDoc<L>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(case) = &mut self.case {
                if let Some(statement) = case.next() {
                    if self.case_value == Some(statement.range().into()) {
                        continue;
                    }
                    return Some(DeclarationScope::unwrap(statement));
                }
                self.case = None;
            }
            let statement = self.direct.next()?;
            if matches!(statement.kind().as_ref(), "switch_case" | "switch_default") {
                self.case_value = statement.field("value").map(|value| value.range().into());
                self.case = Some(DirectChildren::new(statement, &["comment"]));
                continue;
            }
            return Some(DeclarationScope::unwrap(statement));
        }
    }
}

pub(super) struct Catch<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl Catch {
        kinds: ["catch_clause"],
        required: {body: "body"},
        optional: {parameter: "parameter"}
    }
}

pub(super) struct ForLoop<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl ForLoop {
        kinds: ["for_statement"],
        required: {initializer: "initializer", condition: "condition", body: "body"},
        optional: {increment: "increment"}
    }
}

impl<'tree, L: LanguageExt> ForLoop<'tree, L> {
    fn complete_header(&self) -> bool {
        let Ok(body) = self.body() else {
            return false;
        };
        self.initializer().is_ok()
            && self.condition().is_ok()
            && self.increment().is_ok()
            && !self
                .syntax()
                .children()
                .filter(|child| child.range() != body.range())
                .any(|child| child.dfs().any(|node| node.is_error() || node.is_missing()))
    }
}

pub(super) struct Iteration<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl Iteration {
        kinds: ["for_in_statement"],
        required: {pattern: "left", iterable: "right", operator: "operator", body: "body"},
        optional: {kind: "kind", value: "value"}
    }
}

pub(super) enum Loop<'tree, L: LanguageExt> {
    For(ForLoop<'tree, L>),
    Iteration(Iteration<'tree, L>),
}

impl<'tree, L: LanguageExt> Loop<'tree, L> {
    fn cast(node: Node<'tree, StrDoc<L>>) -> Option<Self> {
        match node.kind().as_ref() {
            "for_statement" => Some(Self::For(ForLoop::cast(node)?)),
            "for_in_statement" => Some(Self::Iteration(Iteration::cast(node)?)),
            _ => None,
        }
    }

    fn syntax(&self) -> &Node<'tree, StrDoc<L>> {
        match self {
            Self::For(view) => view.syntax(),
            Self::Iteration(view) => view.syntax(),
        }
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

struct EnumBody<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! { impl EnumBody {
    kinds: ["enum_body"], required: {}, optional: {}, children: {members: ["comment"]},
} }

impl<'tree, L: LanguageExt> Enum<'tree, L> {
    fn bindings(
        &self,
        navigation: &TypeScriptNavigation<'_, '_>,
        facts: &mut vvv_core::Facts,
        coverage: &mut super::navigation::NavigationCoverage,
    ) -> Option<()> {
        let body = EnumBody::cast(self.body().ok()?)?;
        let rule = navigation.local_rule()?;
        let mut declarations = Vec::new();
        for member in body.members() {
            if declarations.len() >= 1_024
                || member
                    .dfs()
                    .any(|node| node.is_error() || node.is_missing())
            {
                return None;
            }
            let name = if member.kind() == "enum_assignment" {
                super::declarations::Declaration::new(
                    member.clone(),
                    super::navigation::NavigationSyntax::TypeScript,
                )
                .field("name")?
            } else {
                member.clone()
            };
            if name.kind() != "property_identifier" {
                return None;
            }
            let mut site = navigation
                .shared
                .site(&member, body.syntax(), rule, coverage);
            site.kind = vvv_core::SymbolKind::Variant;
            let mut binding = super::navigation::BindingName::from_node(
                &name,
                &member,
                navigation.shared.grammar,
            );
            binding.explicit = true;
            declarations.push((
                site,
                binding,
                vvv_core::Span::new(body.syntax().range().start, member.range().end),
            ));
        }
        for (site, name, uninitialized) in declarations {
            site.emit_with_initialization(vec![name], &[uninitialized], facts);
        }
        let mut site = navigation
            .shared
            .site(self.syntax(), body.syntax(), rule, coverage);
        site.kind = vvv_core::SymbolKind::Enum;
        let mut name = super::navigation::BindingName::from_node(
            &self.name().ok()?,
            self.syntax(),
            navigation.shared.grammar,
        );
        name.explicit = true;
        site.emit(vec![name], facts);
        Some(())
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
            (Self::Variable(view), "value") => view.value(),
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
            name: "name",
            value: "value"
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
        optional: {kind: "kind"},
        children: {declarators: ["comment"]}
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
        optional: {value: "value"}
    }
}

impl<'tree, L: LanguageExt> VariableDeclarator<'tree, L> {
    pub fn signature_start(&self, extent_start: usize) -> usize {
        let first = self
            .syntax()
            .parent()
            .and_then(VariableDeclaration::cast)
            .and_then(|declaration| declaration.declarators().next());
        if first.is_some_and(|first| first.range() != self.syntax().range()) {
            self.syntax().range().start
        } else {
            extent_start
        }
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

/// Parameter evaluation order is distinct from the source order of default values.
struct BindingEvaluation<'tree, L: LanguageExt> {
    expression: Node<'tree, StrDoc<L>>,
    initialized: usize,
}

struct PatternBinding<'tree, L: LanguageExt> {
    name: Node<'tree, StrDoc<L>>,
    declaration: Node<'tree, StrDoc<L>>,
    site: super::navigation::BindingSite,
}

enum BindingOwner {
    Parameters,
    Block,
    Var,
    Assignment,
}

/// Checked patterns shared by callable headers, lexical blocks, and hoisted vars.
struct PatternBindings<'tree, L: LanguageExt> {
    owner: BindingOwner,
    bindings: Vec<PatternBinding<'tree, L>>,
    evaluations: Vec<BindingEvaluation<'tree, L>>,
    labels: Vec<vvv_core::Span>,
    steps: usize,
}

impl<'tree, L: LanguageExt> PatternBindings<'tree, L> {
    fn new(owner: BindingOwner) -> Self {
        Self {
            owner,
            bindings: Vec::new(),
            evaluations: Vec::new(),
            labels: Vec::new(),
            steps: 0,
        }
    }

    fn declaration(
        &mut self,
        declaration: VariableDeclaration<'tree, L>,
        scope: &Node<'tree, StrDoc<L>>,
        shared: &super::navigation::NavigationFacts<'_>,
        rule: &vvv_core::BindingRule,
        coverage: &mut super::navigation::NavigationCoverage,
    ) -> Option<()> {
        if declaration
            .syntax()
            .dfs()
            .any(|child| child.is_error() || child.is_missing())
        {
            return None;
        }
        let kind = declaration.kind().ok()?;
        let var = matches!(self.owner, BindingOwner::Var)
            && declaration.syntax().kind() == "variable_declaration";
        if !var
            && !kind
                .as_ref()
                .is_some_and(|kind| matches!(kind.text().as_ref(), "let" | "const"))
        {
            return None;
        }
        let mut declarators = declaration.declarators().peekable();
        declarators.peek()?;
        for node in declarators {
            let declarator = VariableDeclarator::cast(node.clone())?;
            let pattern = declarator.name().ok()?;
            if matches!(
                pattern.kind().as_ref(),
                "rest_pattern" | "assignment_pattern"
            ) {
                return None;
            }
            if let Some(value) = declarator.value().ok()? {
                self.evaluate(value);
            } else if (kind.as_ref().is_some_and(|kind| kind.text() == "const")
                && !declaration
                    .syntax()
                    .ancestors()
                    .any(|node| node.kind() == "ambient_declaration"))
                || pattern.kind() != "identifier"
            {
                return None;
            }
            let site = shared.site(&node, scope, rule, coverage);
            self.pattern(Pattern::new(pattern), &node, &site, 0)?;
        }
        Some(())
    }

    fn evaluate(&mut self, expression: Node<'tree, StrDoc<L>>) {
        if matches!(self.owner, BindingOwner::Var | BindingOwner::Assignment) {
            return;
        }
        self.evaluations.push(BindingEvaluation {
            expression,
            initialized: self.bindings.len(),
        });
    }

    fn pattern(
        &mut self,
        pattern: Pattern<'tree, L>,
        declaration: &Node<'tree, StrDoc<L>>,
        site: &super::navigation::BindingSite,
        depth: usize,
    ) -> Option<()> {
        self.steps += 1;
        if depth >= 128 || self.steps > 1024 {
            return None;
        }
        match pattern.form {
            PatternForm::Other(name)
                if matches!(
                    name.kind().as_ref(),
                    "identifier" | "shorthand_property_identifier_pattern"
                ) =>
            {
                self.bindings.push(PatternBinding {
                    name,
                    declaration: declaration.clone(),
                    site: super::navigation::BindingSite {
                        declaration: site.declaration,
                        scope: site.scope,
                        excluded: site.excluded.clone(),
                        visible_from: site.visible_from,
                        namespace: site.namespace,
                        kind: site.kind,
                    },
                });
            }
            PatternForm::Other(target)
                if matches!(self.owner, BindingOwner::Assignment)
                    && matches!(
                        target.kind().as_ref(),
                        "member_expression" | "subscript_expression"
                    ) =>
            {
                if target
                    .dfs()
                    .any(|node| node.is_missing() || node.is_error())
                    || target.field("optional_chain").is_some()
                {
                    return None;
                }
            }
            PatternForm::Array(view) => {
                self.sequence(view.elements(), declaration, site, depth, false)?;
            }
            PatternForm::Object(view) => {
                self.sequence(view.entries(), declaration, site, depth, true)?;
            }
            PatternForm::Pair(view) => {
                let key = view.key().ok()?;
                if key.kind() == "computed_property_name" {
                    self.evaluate(key);
                } else {
                    self.labels.push(key.range().into());
                }
                self.pattern(
                    Pattern::new(view.value().ok()?),
                    declaration,
                    site,
                    depth + 1,
                )?;
            }
            PatternForm::Assignment(view) => {
                self.evaluate(view.right().ok()?);
                self.pattern(
                    Pattern::new(view.left().ok()?),
                    declaration,
                    site,
                    depth + 1,
                )?;
            }
            PatternForm::Rest(view) => {
                let mut elements = view.elements().filter(|node| node.kind() != "comment");
                let inner = elements.next()?;
                if elements.next().is_some()
                    || !matches!(
                        inner.kind().as_ref(),
                        "identifier" | "array_pattern" | "object_pattern"
                    ) && !(matches!(self.owner, BindingOwner::Assignment)
                        && matches!(
                            inner.kind().as_ref(),
                            "member_expression" | "subscript_expression"
                        ))
                {
                    return None;
                }
                self.pattern(Pattern::new(inner), declaration, site, depth + 1)?;
            }
            _ => return None,
        }
        Some(())
    }

    fn sequence(
        &mut self,
        children: DirectChildren<'tree, L>,
        declaration: &Node<'tree, StrDoc<L>>,
        site: &super::navigation::BindingSite,
        depth: usize,
        object: bool,
    ) -> Option<()> {
        let mut rest = false;
        for child in children {
            if child.kind() == "comment" {
                continue;
            }
            if rest {
                return None;
            }
            rest = child.kind() == "rest_pattern";
            if rest
                && child
                    .next_all()
                    .find(|sibling| sibling.kind() != "comment")
                    .is_some_and(|sibling| sibling.kind() == ",")
            {
                return None;
            }
            if object {
                if rest {
                    let view = RestPattern::cast(child.clone())?;
                    let mut elements = view.elements().filter(|node| node.kind() != "comment");
                    let target = elements.next()?;
                    if (target.kind() != "identifier"
                        && !(matches!(self.owner, BindingOwner::Assignment)
                            && matches!(
                                target.kind().as_ref(),
                                "member_expression" | "subscript_expression"
                            )))
                        || elements.next().is_some()
                    {
                        return None;
                    }
                } else if !matches!(
                    child.kind().as_ref(),
                    "pair_pattern"
                        | "object_assignment_pattern"
                        | "shorthand_property_identifier_pattern"
                ) {
                    return None;
                }
            }
            self.pattern(Pattern::new(child), declaration, site, depth + 1)?;
        }
        Some(())
    }

    fn emit(
        self,
        facts: &mut vvv_core::Facts,
        coverage: &mut super::navigation::NavigationCoverage,
        grammar: &Grammar,
    ) {
        for label in self.labels {
            coverage.block(label);
        }
        for (index, binding) in self.bindings.into_iter().enumerate() {
            let evaluations = self
                .evaluations
                .iter()
                .filter(|evaluation| evaluation.initialized <= index)
                .map(|evaluation| vvv_core::Span::from(evaluation.expression.range()));
            let mut name = super::navigation::BindingName::from_node(
                &binding.name,
                &binding.declaration,
                grammar,
            );
            match self.owner {
                BindingOwner::Parameters => {
                    binding.site.emit_with_initialization(
                        vec![name],
                        &evaluations.collect::<Vec<_>>(),
                        facts,
                    );
                }
                BindingOwner::Assignment => coverage.reference(binding.name.range().into()),
                BindingOwner::Var => {
                    name.explicit = true;
                    binding.site.emit(vec![name], facts);
                }
                BindingOwner::Block => {
                    name.explicit = true;
                    let mut uninitialized = vec![vvv_core::Span::new(
                        binding.site.scope.start,
                        binding.site.declaration.start,
                    )];
                    uninitialized.extend(evaluations);
                    if let Some(case) = binding.declaration.ancestors().find(|node| {
                        matches!(node.kind().as_ref(), "switch_case" | "switch_default")
                    }) && let Some(body) = case
                        .parent()
                        .filter(|body| vvv_core::Span::from(body.range()) == binding.site.scope)
                    {
                        // Other cases may execute without this declaration ever initializing.
                        uninitialized.extend(
                            body.children()
                                .filter(|node| {
                                    node.is_named()
                                        && node.kind() != "comment"
                                        && node.range() != case.range()
                                })
                                .map(|node| vvv_core::Span::from(node.range())),
                        );
                    }
                    binding
                        .site
                        .emit_with_initialization(vec![name], &uninitialized, facts);
                }
            }
        }
    }
}

/// One callable body's hoisted declarations, excluding nested callable/type owners.
struct VarBindings<'tree, L: LanguageExt> {
    body: DeclarationScope<'tree, L>,
    bindings: PatternBindings<'tree, L>,
    steps: usize,
}

impl<'tree, L: LanguageExt> VarBindings<'tree, L> {
    fn new(body: DeclarationScope<'tree, L>) -> Self {
        Self {
            body,
            bindings: PatternBindings::new(BindingOwner::Var),
            steps: 0,
        }
    }

    fn collect(
        &mut self,
        node: Node<'tree, StrDoc<L>>,
        navigation: &TypeScriptNavigation<'_, '_>,
        coverage: &mut super::navigation::NavigationCoverage,
        depth: usize,
    ) -> Option<()> {
        self.steps += 1;
        if self.steps > 1_024 || depth >= 128 || node.is_error() || node.is_missing() {
            return None;
        }
        if Function::cast(node.clone()).is_some()
            || Self::type_boundary(&node)
            || node.kind() == "class_static_block"
        {
            return Some(());
        }
        if let Some(declaration) = VariableDeclaration::cast(node.clone())
            && node.kind() == "variable_declaration"
        {
            self.bindings.declaration(
                declaration,
                &self.body.node,
                navigation.shared,
                navigation.var_rule::<L>()?,
                coverage,
            )?;
            return Some(());
        }
        if let Some(iteration) = Iteration::cast(node.clone())
            && iteration
                .kind()
                .ok()?
                .is_some_and(|kind| kind.text() == "var")
        {
            let pattern = iteration.pattern().ok()?;
            let iterable = iteration.iterable().ok()?;
            let operator = iteration.operator().ok()?;
            if !matches!(operator.text().as_ref(), "in" | "of")
                || iteration.value().ok()?.is_some()
                || pattern
                    .dfs()
                    .chain(iterable.dfs())
                    .any(|child| child.is_error() || child.is_missing())
            {
                return None;
            }
            let site = navigation.shared.site(
                &pattern,
                &self.body.node,
                navigation.iteration_rule::<L>()?,
                coverage,
            );
            self.bindings
                .pattern(Pattern::new(pattern.clone()), &pattern, &site, 0)?;
            return self.collect(iteration.body().ok()?, navigation, coverage, depth + 1);
        }
        for child in DirectChildren::new(node, &["comment"]) {
            self.collect(child, navigation, coverage, depth + 1)?;
        }
        Some(())
    }

    fn type_boundary(node: &Node<'tree, StrDoc<L>>) -> bool {
        matches!(
            node.kind().as_ref(),
            "class"
                | "class_declaration"
                | "abstract_class_declaration"
                | "interface_declaration"
                | "internal_module"
                | "module"
                | "type_alias_declaration"
        )
    }
}

/// Parameter and body environments, with explicit declaration sites preserved.
struct CallableScope<'tree, L: LanguageExt> {
    function: Function<'tree, L>,
    body: vvv_core::Span,
}

impl<'tree, L: LanguageExt> CallableScope<'tree, L> {
    fn new(function: Function<'tree, L>, body: &Block<'tree, L>) -> Self {
        Self {
            function,
            body: body.syntax().range().into(),
        }
    }

    fn separate_parameters(&self) -> Option<bool> {
        let mut steps = 0;
        match self.function.parameter_shape().ok()? {
            Parameters::Single(_) | Parameters::Absent => Some(false),
            Parameters::List(parameters) => {
                for node in parameters {
                    let parameter = Parameter::cast(node)?;
                    if parameter.value().ok()?.is_some() {
                        return Some(true);
                    }
                    let pattern = parameter.pattern().ok()?.or(parameter.name().ok()?)?;
                    for node in pattern.dfs() {
                        steps += 1;
                        if steps > 1_024 || node.is_error() || node.is_missing() {
                            return None;
                        }
                        if matches!(
                            node.kind().as_ref(),
                            "assignment_pattern"
                                | "object_assignment_pattern"
                                | "computed_property_name"
                        ) {
                            return Some(true);
                        }
                    }
                }
                Some(false)
            }
        }
    }

    fn declarations(
        &self,
        names: &[String],
        facts: &vvv_core::Facts,
    ) -> Option<Vec<vvv_core::LexicalBinding>> {
        if names.is_empty() {
            return Some(Vec::new());
        }
        let mut parameters = Vec::new();
        for binding in &facts.lexical {
            if binding.namespace != vvv_core::BindingNamespace::Value
                || !names.contains(&binding.symbol.name)
            {
                continue;
            }
            if binding.scope == self.body
                && !matches!(
                    binding.symbol.kind,
                    vvv_core::SymbolKind::Function | vvv_core::SymbolKind::Parameter
                )
            {
                return None;
            }
            if binding.symbol.kind == vvv_core::SymbolKind::Parameter
                && binding.scope.contains(&self.body)
                && binding.scope.end == self.function.syntax().range().end
                && vvv_core::Span::from(self.function.syntax().range()).contains(&binding.scope)
                && binding.symbol.name_span.end <= self.body.start
            {
                parameters.push(binding.clone());
            }
        }
        if parameters.is_empty() || self.separate_parameters()? {
            return Some(Vec::new());
        }
        for parameter in &mut parameters {
            parameter.scope = self.body;
            parameter.visible_from = self.body.start;
            parameter.uninitialized.clear();
        }
        parameters.retain(|parameter| {
            !facts.lexical.iter().any(|binding| {
                binding.scope == self.body && binding.symbol.name_span == parameter.symbol.name_span
            })
        });
        Some(parameters)
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

/// Parse-local ownership evidence needed once every declaration scope is available.
struct CatchEnvironment {
    body: vvv_core::Span,
    simple: bool,
}

struct NamespaceEnvironment {
    name: vvv_core::Span,
    body: vvv_core::Span,
}

pub(super) struct TypeScriptNavigation<'a, 'g> {
    shared: &'a super::navigation::NavigationFacts<'g>,
    var_owners: std::collections::BTreeSet<vvv_core::Span>,
    module: bool,
    var_sites: std::collections::BTreeSet<vvv_core::Span>,
    catches: std::collections::BTreeMap<vvv_core::Span, CatchEnvironment>,
    namespaces: Vec<NamespaceEnvironment>,
}

impl<'a, 'g> TypeScriptNavigation<'a, 'g> {
    pub fn new(shared: &'a super::navigation::NavigationFacts<'g>) -> Self {
        Self {
            shared,
            var_owners: std::collections::BTreeSet::new(),
            module: false,
            var_sites: std::collections::BTreeSet::new(),
            catches: std::collections::BTreeMap::new(),
            namespaces: Vec::new(),
        }
    }

    fn var_rule<L: LanguageExt>(&self) -> Option<&'g vvv_core::BindingRule> {
        let mut rules = self
            .shared
            .grammar
            .bindings
            .iter()
            .filter(|rule| rule.node == "variable_declaration");
        let rule = rules.next()?;
        (rules.next().is_none()
            && rule.name.is_none()
            && rule.scopes == Function::<L>::NAVIGATION_KINDS
            && rule.kind == vvv_core::SymbolKind::Variable
            && rule.namespace == vvv_core::BindingNamespace::Value
            && !rule.after)
            .then_some(rule)
    }

    fn vars<L: LanguageExt>(
        &mut self,
        node: &Node<'_, StrDoc<L>>,
        facts: &mut vvv_core::Facts,
        coverage: &mut super::navigation::NavigationCoverage,
    ) -> bool {
        let mut scope = None;
        for ancestor in node.ancestors() {
            if let Some(owner) = DeclarationScope::cast(ancestor.clone())
                && owner.var_owner()
            {
                scope = Some(owner);
                break;
            }
            if VarBindings::type_boundary(&ancestor) || Function::cast(ancestor).is_some() {
                return false;
            }
        }
        let Some(scope) = scope else {
            return false;
        };
        if let Some(function) = scope.callable()
            && !coverage.supports(function.syntax().range().into())
        {
            coverage.block(scope.node.range().into());
            return true;
        }
        let span = scope.node.range().into();
        if !self.var_owners.insert(span) {
            return true;
        }
        let callable = scope.callable().and_then(|owner| {
            Block::cast(scope.node.clone()).map(|body| CallableScope::new(owner, &body))
        });
        let mut vars = VarBindings::new(scope);
        let peers = vars
            .collect(vars.body.node.clone(), self, coverage, 0)
            .and_then(|()| {
                let names = vars
                    .bindings
                    .bindings
                    .iter()
                    .map(|binding| binding.name.text().into_owned())
                    .collect::<Vec<_>>();
                match callable {
                    Some(scope) => scope.declarations(&names, facts),
                    None => Some(Vec::new()),
                }
            });
        if let Some(peers) = peers {
            self.var_sites.extend(
                vars.bindings
                    .bindings
                    .iter()
                    .map(|binding| vvv_core::Span::from(binding.name.range())),
            );
            facts.lexical.extend(peers);
            vars.bindings.emit(facts, coverage, self.shared.grammar);
        } else {
            coverage.block(span);
        }
        true
    }

    fn local_rule(&self) -> Option<&'g vvv_core::BindingRule> {
        let mut rules = self.shared.grammar.bindings.iter().filter(|rule| {
            rule.node == "lexical_declaration" && rule.scopes.contains(&"statement_block")
        });
        let rule = rules.next()?;
        (rules.next().is_none()
            && rule.name.is_none()
            && rule.kind == vvv_core::SymbolKind::Variable
            && rule.namespace == vvv_core::BindingNamespace::Value
            && !rule.after)
            .then_some(rule)
    }

    fn locals<L: LanguageExt>(
        &self,
        scope: &DeclarationScope<'_, L>,
        rule: &vvv_core::BindingRule,
        facts: &mut vvv_core::Facts,
        coverage: &mut super::navigation::NavigationCoverage,
    ) -> Option<()> {
        let mut bindings = PatternBindings::new(BindingOwner::Block);
        let mut named = Vec::new();
        for statement in scope.statements() {
            if statement.is_error() || statement.is_missing() {
                return None;
            }
            if statement.kind() == "lexical_declaration" {
                bindings.declaration(
                    VariableDeclaration::cast(statement)?,
                    &scope.node,
                    self.shared,
                    rule,
                    coverage,
                )?;
                continue;
            }
            let (kind, namespaces, temporal) = match statement.kind().as_ref() {
                "class_declaration" | "abstract_class_declaration" => (
                    vvv_core::SymbolKind::Class,
                    &[
                        vvv_core::BindingNamespace::Value,
                        vvv_core::BindingNamespace::Type,
                    ][..],
                    true,
                ),
                "interface_declaration" => (
                    vvv_core::SymbolKind::Interface,
                    &[vvv_core::BindingNamespace::Type][..],
                    false,
                ),
                "type_alias_declaration" => (
                    vvv_core::SymbolKind::TypeAlias,
                    &[vvv_core::BindingNamespace::Type][..],
                    false,
                ),
                "enum_declaration" => (
                    vvv_core::SymbolKind::Enum,
                    &[
                        vvv_core::BindingNamespace::Value,
                        vvv_core::BindingNamespace::Type,
                    ][..],
                    true,
                ),
                "internal_module" | "module" => (
                    vvv_core::SymbolKind::Module,
                    &[
                        vvv_core::BindingNamespace::Value,
                        vvv_core::BindingNamespace::Type,
                    ][..],
                    true,
                ),
                _ => continue,
            };
            // Built-in declaration policy is opt-in; custom grammar bindings stay table-owned.
            if !matches!(
                statement.kind().as_ref(),
                "abstract_class_declaration" | "internal_module" | "module"
            ) {
                let mut rules = self
                    .shared
                    .grammar
                    .bindings
                    .iter()
                    .filter(|rule| rule.node == statement.kind());
                let Some(rule) = rules.next() else { continue };
                if rules.next().is_some()
                    || rule.name.is_some()
                    || rule.scopes != ["statement_block"]
                {
                    continue;
                }
            }
            if named.len() >= 1_024 {
                return None;
            }
            let declaration = super::declarations::Declaration::new(
                statement.clone(),
                super::navigation::NavigationSyntax::TypeScript,
            );
            let name = if let Some(namespace) = Namespace::cast(statement.clone()) {
                namespace.name().ok()?
            } else {
                declaration.field("name")?
            };
            if !matches!(name.kind().as_ref(), "identifier" | "type_identifier")
                || statement
                    .dfs()
                    .any(|node| node.is_missing() || node.is_error())
            {
                return None;
            }
            for namespace in namespaces {
                let mut site = self.shared.site(&statement, &scope.node, rule, coverage);
                site.kind = kind;
                site.namespace = *namespace;
                let uninitialized = if temporal && *namespace == vvv_core::BindingNamespace::Value {
                    let end = Namespace::cast(statement.clone())
                        .and_then(|namespace| namespace.body().ok().flatten())
                        .map_or(statement.range().end, |body| body.range().start);
                    vec![vvv_core::Span::new(scope.node.range().start, end)]
                } else {
                    Vec::new()
                };
                let mut binding = super::navigation::BindingName::from_node(
                    &name,
                    &statement,
                    self.shared.grammar,
                );
                binding.explicit = true;
                named.push((site, binding, uninitialized));
            }
        }
        bindings.emit(facts, coverage, self.shared.grammar);
        for (site, name, uninitialized) in named {
            site.emit_with_initialization(vec![name], &uninitialized, facts);
        }
        Some(())
    }

    fn function_rule(&self, kind: &str) -> Option<&'g vvv_core::BindingRule> {
        if !matches!(
            kind,
            "function_declaration" | "generator_function_declaration"
        ) {
            return None;
        }
        let mut rules = self
            .shared
            .grammar
            .bindings
            .iter()
            .filter(|rule| rule.node == kind);
        let rule = rules.next()?;
        (rules.next().is_none()
            && rule.name == Some("name")
            && rule.scopes == ["statement_block"]
            && rule.kind == vvv_core::SymbolKind::Function
            && rule.namespace == vvv_core::BindingNamespace::Value
            && !rule.after)
            .then_some(rule)
    }

    fn functions<L: LanguageExt>(
        &self,
        scope: &DeclarationScope<'_, L>,
        facts: &mut vvv_core::Facts,
        coverage: &mut super::navigation::NavigationCoverage,
    ) -> Option<()> {
        let mut declarations = Vec::new();
        for statement in scope.statements() {
            let signature = statement.kind() == "function_signature";
            let rule = if signature {
                self.function_rule("function_declaration")
            } else {
                self.function_rule(statement.kind().as_ref())
            };
            let Some(rule) = rule else { continue };
            if !scope.var_owner()
                && !scope.strict(self.module)
                && statement.kind() == "function_declaration"
                && !statement.children().any(|child| child.kind() == "async")
            {
                return None;
            }
            if declarations.len() >= 1_024 {
                return None;
            }
            let function = Function::cast(statement)?;
            if (!signature && !function.complete_header())
                || (signature
                    && function
                        .syntax()
                        .dfs()
                        .any(|node| node.is_missing() || node.is_error()))
            {
                return None;
            }
            let name = function.name().ok().flatten()?;
            if !matches!(name.kind().as_ref(), "identifier") || name.is_missing() || name.is_error()
            {
                return None;
            }
            let mut binding = super::navigation::BindingName::from_node(
                &name,
                function.syntax(),
                self.shared.grammar,
            );
            binding.explicit = true;
            declarations.push((
                self.shared
                    .site(function.syntax(), &scope.node, rule, coverage),
                binding,
            ));
        }
        if let Some(owner) = scope.callable() {
            let callable = CallableScope {
                function: owner,
                body: scope.node.range().into(),
            };
            let names = declarations
                .iter()
                .map(|(_, name)| name.name.clone())
                .collect::<Vec<_>>();
            facts.lexical.extend(callable.declarations(&names, facts)?);
        }
        for (site, name) in declarations {
            site.emit(vec![name], facts);
        }
        Some(())
    }

    fn block_function_owner<L: LanguageExt>(
        &self,
        node: &Node<'_, StrDoc<L>>,
        coverage: &mut super::navigation::NavigationCoverage,
    ) {
        let owner = std::iter::once(node.clone())
            .chain(node.ancestors())
            .filter_map(DeclarationScope::cast)
            .find(DeclarationScope::var_owner);
        coverage
            .block(owner.map_or_else(|| node.range().into(), |owner| owner.node.range().into()));
    }

    fn iteration_rule<L: LanguageExt>(&self) -> Option<&'g vvv_core::BindingRule> {
        let mut rules = self
            .shared
            .grammar
            .bindings
            .iter()
            .filter(|rule| rule.node == "for_in_statement");
        let rule = rules.next()?;
        (rules.next().is_none()
            && rule.name.is_none()
            && rule.scopes == Function::<L>::NAVIGATION_KINDS
            && rule.kind == vvv_core::SymbolKind::Variable
            && rule.namespace == vvv_core::BindingNamespace::Value
            && !rule.after)
            .then_some(rule)
    }

    fn loop_bindings<L: LanguageExt>(
        &self,
        owner: &Loop<'_, L>,
        rule: &vvv_core::BindingRule,
        facts: &mut vvv_core::Facts,
        coverage: &mut super::navigation::NavigationCoverage,
    ) -> Option<()> {
        let mut bindings = PatternBindings::new(BindingOwner::Block);
        match owner {
            Loop::For(view) => {
                if !view.complete_header() {
                    return None;
                }
                let initializer = view.initializer().ok()?;
                if initializer.kind() != "lexical_declaration" {
                    // Assignment headers add no bindings; the callable owns hoisted vars.
                    return Some(());
                }
                bindings.declaration(
                    VariableDeclaration::cast(initializer)?,
                    owner.syntax(),
                    self.shared,
                    rule,
                    coverage,
                )?;
            }
            Loop::Iteration(view) => {
                let kind = view.kind().ok()?;
                if kind.as_ref().is_some_and(|kind| kind.text() == "var") {
                    // Unmodeled var headers retain table policy, including legacy initializers.
                    self.shared.table_bindings(owner.syntax(), facts, coverage);
                    return Some(());
                }
                view.body().ok()?;
                let pattern = view.pattern().ok()?;
                let iterable = view.iterable().ok()?;
                let operator = view.operator().ok()?;
                if !matches!(operator.text().as_ref(), "in" | "of")
                    || view.value().ok()?.is_some()
                    || pattern
                        .dfs()
                        .chain(iterable.dfs())
                        .any(|child| child.is_error() || child.is_missing())
                {
                    return None;
                }
                let Some(kind) = kind else {
                    let mut targets = PatternBindings::new(BindingOwner::Assignment);
                    let site = self.shared.site(&pattern, owner.syntax(), rule, coverage);
                    targets.pattern(Pattern::new(pattern.clone()), &pattern, &site, 0)?;
                    targets.emit(facts, coverage, self.shared.grammar);
                    return Some(());
                };
                if !matches!(kind.text().as_ref(), "let" | "const") {
                    return None;
                }
                bindings.evaluate(iterable);
                let site = self.shared.site(&pattern, owner.syntax(), rule, coverage);
                bindings.pattern(Pattern::new(pattern.clone()), &pattern, &site, 0)?;
            }
        }
        bindings.emit(facts, coverage, self.shared.grammar);
        Some(())
    }

    fn parameter_rule<L: LanguageExt>(
        &self,
        parameter: &Node<'_, StrDoc<L>>,
        owner: &Node<'_, StrDoc<L>>,
    ) -> Option<&'g vvv_core::BindingRule> {
        let mut rules = self.shared.grammar.bindings.iter().filter(|rule| {
            rule.node == parameter.kind() && rule.scopes.contains(&owner.kind().as_ref())
        });
        let rule = rules.next()?;
        (rules.next().is_none()
            && rule.name == Some("pattern")
            && rule.kind == vvv_core::SymbolKind::Parameter
            && rule.namespace == vvv_core::BindingNamespace::Value
            && !rule.after)
            .then_some(rule)
    }

    fn owner_rule(
        &self,
        node: &str,
        field: &str,
        kind: vvv_core::SymbolKind,
    ) -> Option<&'g vvv_core::BindingRule> {
        let mut rules = self
            .shared
            .grammar
            .bindings
            .iter()
            .filter(|rule| rule.node == node);
        let rule = rules.next()?;
        (rules.next().is_none()
            && rule.name == Some(field)
            && rule.scopes == [node]
            && rule.kind == kind
            && rule.namespace == vvv_core::BindingNamespace::Value
            && !rule.after)
            .then_some(rule)
    }

    fn catch_bindings<L: LanguageExt>(
        &self,
        owner: &Catch<'_, L>,
        rule: &vvv_core::BindingRule,
        facts: &mut vvv_core::Facts,
        coverage: &mut super::navigation::NavigationCoverage,
    ) -> Option<()> {
        owner.body().ok()?;
        let mut bindings = PatternBindings::new(BindingOwner::Parameters);
        if let Some(pattern) = owner.parameter().ok()? {
            if pattern
                .dfs()
                .any(|child| child.is_error() || child.is_missing())
                || matches!(
                    pattern.kind().as_ref(),
                    "rest_pattern" | "assignment_pattern"
                )
            {
                return None;
            }
            let site = self.shared.site(&pattern, owner.syntax(), rule, coverage);
            bindings.pattern(Pattern::new(pattern.clone()), &pattern, &site, 0)?;
        }
        bindings.emit(facts, coverage, self.shared.grammar);
        Some(())
    }

    fn parameters<L: LanguageExt>(
        &self,
        function: &Function<'_, L>,
        facts: &mut vvv_core::Facts,
        coverage: &mut super::navigation::NavigationCoverage,
    ) -> Option<()> {
        let mut bindings = PatternBindings::new(BindingOwner::Parameters);
        let parameters = match function.parameter_shape().ok()? {
            Parameters::List(parameters) => parameters,
            Parameters::Single(node) => {
                let rule = self.owner_rule(
                    "arrow_function",
                    "parameter",
                    vvv_core::SymbolKind::Parameter,
                )?;
                let site = self.shared.site(&node, function.syntax(), rule, coverage);
                bindings.pattern(Pattern::new(node.clone()), &node, &site, 0)?;
                bindings.emit(facts, coverage, self.shared.grammar);
                return Some(());
            }
            Parameters::Absent => return None,
        };
        let expression_name = if matches!(
            function.syntax().kind().as_ref(),
            "function_expression" | "generator_function"
        ) {
            function.name().ok()?
        } else {
            None
        };
        let mut rest = false;
        for node in parameters {
            if rest {
                return None;
            }
            let Some(rule) = self.parameter_rule(&node, function.syntax()) else {
                if !matches!(
                    function.syntax().kind().as_ref(),
                    "function_declaration" | "method_definition"
                ) {
                    return None;
                }
                continue;
            };
            if node
                .dfs()
                .any(|child| child.is_error() || child.is_missing())
            {
                return None;
            }
            let parameter = Parameter::cast(node.clone())?;
            let pattern = parameter.pattern().ok()??;
            rest = pattern.kind() == "rest_pattern";
            if rest
                && node
                    .next_all()
                    .find(|sibling| sibling.kind() != "comment")
                    .is_some_and(|sibling| sibling.kind() == ",")
            {
                return None;
            }
            if let Some(value) = parameter.value().ok()? {
                if pattern.kind() == "rest_pattern" {
                    return None;
                }
                bindings.evaluate(value);
            }
            let mut site = self.shared.site(&node, function.syntax(), rule, coverage);
            if let Some(name) = &expression_name {
                // The function-expression name encloses its parameter environment.
                site.scope.start = name.range().end;
                site.visible_from = site.scope.start;
            }
            bindings.pattern(Pattern::new(pattern), &node, &site, 0)?;
        }
        if let Some(name) = expression_name
            && let Some(rule) = self.owner_rule(
                function.syntax().kind().as_ref(),
                "name",
                vvv_core::SymbolKind::Function,
            )
        {
            let site = self
                .shared
                .site(function.syntax(), function.syntax(), rule, coverage);
            let mut binding = super::navigation::BindingName::from_node(
                &name,
                function.syntax(),
                self.shared.grammar,
            );
            binding.explicit = true;
            site.emit(vec![binding], facts);
            if let Some(binding) = facts.lexical.last()
                && let Some(signature) =
                    super::signatures::Signatures::new(self.shared.grammar.signatures).navigation(
                        function.syntax(),
                        &binding.symbol,
                        super::navigation::NavigationSyntax::TypeScript,
                    )
                && !facts
                    .signatures
                    .iter()
                    .any(|existing| existing.name_span == signature.name_span)
            {
                facts.signatures.push(signature);
            }
        }
        bindings.emit(facts, coverage, self.shared.grammar);
        Some(())
    }

    pub(super) fn validate(
        &self,
        root: vvv_core::Span,
        facts: &mut vvv_core::Facts,
        coverage: &mut super::navigation::NavigationCoverage,
    ) {
        // Validate after all owners publish, so source traversal order cannot hide conflicts.
        let mut invalid_vars = std::collections::BTreeSet::new();
        let mut names = std::collections::BTreeMap::<&str, Vec<&vvv_core::LexicalBinding>>::new();
        for binding in &facts.lexical {
            names.entry(&binding.symbol.name).or_default().push(binding);
        }
        for binding in facts
            .lexical
            .iter()
            .filter(|binding| self.var_sites.contains(&binding.symbol.name_span))
        {
            let conflict = names[binding.symbol.name.as_str()].iter().any(|other| {
                if other.namespace != binding.namespace
                    || other.symbol.name != binding.symbol.name
                    || self.var_sites.contains(&other.symbol.name_span)
                    || other.symbol.name_span == binding.symbol.name_span
                {
                    return false;
                }
                if other.scope == binding.scope {
                    return !matches!(
                        other.symbol.kind,
                        vvv_core::SymbolKind::Function | vvv_core::SymbolKind::Parameter
                    );
                }
                binding.scope.contains(&other.scope)
                    && other.scope.contains(&binding.symbol.name_span)
                    && !self
                        .catches
                        .get(&other.scope)
                        .is_some_and(|catch| catch.simple)
            });
            if conflict {
                coverage.block(binding.scope);
                invalid_vars.insert(binding.scope);
            }
        }
        for function in facts
            .lexical
            .iter()
            .filter(|binding| binding.symbol.kind == vvv_core::SymbolKind::Function)
        {
            if names[function.symbol.name.as_str()].iter().any(|other| {
                other.scope == function.scope
                    && other.namespace == function.namespace
                    && other.symbol.name == function.symbol.name
                    && !matches!(
                        other.symbol.kind,
                        vvv_core::SymbolKind::Function | vvv_core::SymbolKind::Parameter
                    )
                    && !self.var_sites.contains(&other.symbol.name_span)
            }) {
                coverage.block(function.scope);
            }
        }
        for (catch_span, catch) in &self.catches {
            for parameter in facts
                .lexical
                .iter()
                .filter(|binding| binding.scope == *catch_span)
            {
                if names[parameter.symbol.name.as_str()].iter().any(|other| {
                    other.scope == catch.body && other.namespace == parameter.namespace
                }) {
                    coverage.block(catch.body);
                }
            }
        }
        let root_span = root;
        for import in facts.named_imports.iter().filter(|import| !import.reexport) {
            if names.get(import.local.as_str()).is_some_and(|bindings| {
                bindings.iter().any(|binding| {
                    binding.scope == root_span
                        && binding.symbol.name == import.local
                        && (!import.type_only
                            || binding.namespace == vvv_core::BindingNamespace::Type)
                })
            }) {
                coverage.block(root_span);
            }
        }
        for namespace in &self.namespaces {
            let binding = facts
                .lexical
                .iter()
                .find(|binding| binding.symbol.name_span == namespace.name);
            if let Some(binding) = binding
                && names[binding.symbol.name.as_str()].iter().any(|other| {
                    other.scope == binding.scope
                        && other.namespace == binding.namespace
                        && other.symbol.name_span != namespace.name
                })
            {
                // Merged namespace bodies need cross-body member ownership evidence.
                coverage.block(namespace.body);
            }
        }
        drop(names);
        facts.lexical.retain(|binding| {
            !invalid_vars.contains(&binding.scope)
                || (!self.var_sites.contains(&binding.symbol.name_span)
                    && binding.symbol.kind != vvv_core::SymbolKind::Parameter)
        });
    }

    pub fn extract<L: LanguageExt>(
        &mut self,
        node: &Node<'_, StrDoc<L>>,
        facts: &mut vvv_core::Facts,
        coverage: &mut super::navigation::NavigationCoverage,
    ) {
        let kind = node.kind();
        if let Some(program) = Program::cast(node.clone()) {
            self.module = program.module();
        }
        if let Some(namespace) = Namespace::cast(node.clone()) {
            if let Ok(name) = namespace.name()
                && name.kind() == "identifier"
                && let Ok(Some(body)) = namespace.body()
                && !node
                    .ancestors()
                    .any(|parent| parent.kind() == "ambient_declaration")
            {
                self.namespaces.push(NamespaceEnvironment {
                    name: name.range().into(),
                    body: body.range().into(),
                });
                coverage.model(node.range().into());
            } else {
                coverage.block(node.range().into());
            }
        }
        if (kind == "variable_declaration"
            || (kind == "for_in_statement"
                && self.iteration_rule::<L>().is_some()
                && Iteration::cast(node.clone()).is_some_and(|iteration| {
                    iteration
                        .kind()
                        .ok()
                        .flatten()
                        .is_some_and(|kind| kind.text() == "var")
                })))
            && self.var_rule::<L>().is_some()
            && self.vars(node, facts, coverage)
        {
            return;
        }
        if matches!(kind.as_ref(), "for_statement" | "for_in_statement")
            && let Some(owner) = Loop::cast(node.clone())
        {
            let rule = match &owner {
                Loop::For(_) => self
                    .local_rule()
                    .filter(|rule| rule.scopes.contains(&"for_statement")),
                Loop::Iteration(_) => self.iteration_rule::<L>(),
            };
            if let Some(rule) = rule {
                if self.loop_bindings(&owner, rule, facts, coverage).is_none() {
                    coverage.block(node.range().into());
                }
                return;
            }
        }
        if let Some(scope) = DeclarationScope::cast(node.clone()) {
            if let Some(rule) = self.local_rule()
                && self.locals(&scope, rule, facts, coverage).is_none()
            {
                coverage.block(node.range().into());
            }
            if (self.function_rule("function_declaration").is_some()
                || self
                    .function_rule("generator_function_declaration")
                    .is_some())
                && self.functions(&scope, facts, coverage).is_none()
            {
                self.block_function_owner(&scope.node, coverage);
            }
        } else if kind == "lexical_declaration"
            && self.local_rule().is_some()
            && node
                .parent()
                .map(DeclarationScope::unwrap)
                .is_some_and(|parent| {
                    DeclarationScope::cast(parent.clone()).is_some()
                        || matches!(
                            parent.kind().as_ref(),
                            "export_statement" | "switch_case" | "switch_default"
                        )
                        || (parent.kind() == "for_statement"
                            && self
                                .local_rule()
                                .is_some_and(|rule| rule.scopes.contains(&"for_statement")))
                })
        {
            return;
        }
        if matches!(
            kind.as_ref(),
            "class" | "class_declaration" | "abstract_class_declaration"
        ) {
            let declaration = super::declarations::Declaration::new(
                node.clone(),
                super::navigation::NavigationSyntax::TypeScript,
            );
            if let (Some(name), Some(body), Some(rule)) = (
                declaration.field("name"),
                declaration.field("body"),
                self.local_rule(),
            ) {
                for namespace in [
                    vvv_core::BindingNamespace::Type,
                    vvv_core::BindingNamespace::Value,
                ] {
                    if namespace == vvv_core::BindingNamespace::Type
                        && declaration.shadows(name.text().as_ref())
                    {
                        continue;
                    }
                    let mut site = self.shared.site(node, &body, rule, coverage);
                    site.namespace = namespace;
                    site.kind = vvv_core::SymbolKind::Class;
                    let mut binding =
                        super::navigation::BindingName::from_node(&name, node, self.shared.grammar);
                    binding.explicit = true;
                    site.emit(vec![binding], facts);
                }
            }
        }
        if let Some(enumeration) = Enum::cast(node.clone())
            && self.local_rule().is_some()
            && enumeration.bindings(self, facts, coverage).is_none()
        {
            coverage.block(
                enumeration
                    .body()
                    .map_or_else(|_| node.range().into(), |body| body.range().into()),
            );
        }
        if kind == "with_statement" {
            coverage.block(node.range().into());
        }
        if kind == "catch_clause"
            && let Some(rule) =
                self.owner_rule("catch_clause", "parameter", vvv_core::SymbolKind::Variable)
            && let Some(owner) = Catch::cast(node.clone())
        {
            if let Ok(body) = owner.body() {
                self.catches.insert(
                    node.range().into(),
                    CatchEnvironment {
                        body: body.range().into(),
                        simple: owner
                            .parameter()
                            .ok()
                            .flatten()
                            .is_some_and(|parameter| parameter.kind() == "identifier"),
                    },
                );
            }
            if self.catch_bindings(&owner, rule, facts, coverage).is_some() {
                coverage.model(node.range().into());
            } else {
                coverage.block(node.range().into());
            }
            return;
        }
        if kind == "function_signature"
            && let Some(function) = Function::cast(node.clone())
            && function.complete_fields()
            && !node.dfs().any(|node| node.is_missing() || node.is_error())
            && self.parameters(&function, facts, coverage).is_some()
        {
            coverage.model(node.range().into());
        }
        if Function::<L>::NAVIGATION_KINDS.contains(&kind.as_ref())
            && let Some(function) = Function::cast(node.clone()).filter(Function::navigation_owner)
        {
            let expression = matches!(
                kind.as_ref(),
                "arrow_function" | "function_expression" | "generator_function"
            );
            let owner_rule = if expression {
                self.owner_rule(
                    kind.as_ref(),
                    if kind == "arrow_function" {
                        "parameter"
                    } else {
                        "name"
                    },
                    if kind == "arrow_function" {
                        vvv_core::SymbolKind::Parameter
                    } else {
                        vvv_core::SymbolKind::Function
                    },
                )
            } else {
                None
            };
            if (!expression || owner_rule.is_some())
                && function.complete_header()
                && self.parameters(&function, facts, coverage).is_some()
            {
                coverage.model(node.range().into());
            } else {
                coverage.block(node.range().into());
            }
            if expression && owner_rule.is_some() {
                return;
            }
        } else if matches!(kind.as_ref(), "required_parameter" | "optional_parameter")
            && let Some(owner) = node.ancestors().find_map(Function::cast)
            && (owner.navigation_owner() || owner.syntax().kind() == "function_signature")
            && self.parameter_rule(node, owner.syntax()).is_some()
        {
            // The owner interprets the complete header once, including evaluation order.
            return;
        }
        if self.function_rule(kind.as_ref()).is_some() || kind == "function_signature" {
            let parent = node.ancestors().find(|parent| {
                !matches!(
                    parent.kind().as_ref(),
                    "export_statement" | "ambient_declaration" | "switch_case" | "switch_default"
                )
            });
            if parent
                .as_ref()
                .is_some_and(|parent| DeclarationScope::cast(parent.clone()).is_some())
            {
                return;
            }
            self.block_function_owner(node, coverage);
            return;
        }
        if matches!(
            kind.as_ref(),
            "class_declaration"
                | "abstract_class_declaration"
                | "type_alias_declaration"
                | "interface_declaration"
                | "enum_declaration"
        ) && self.local_rule().is_some()
            && node
                .ancestors()
                .find(|parent| {
                    !matches!(
                        parent.kind().as_ref(),
                        "export_statement"
                            | "ambient_declaration"
                            | "switch_case"
                            | "switch_default"
                    )
                })
                .is_some_and(|parent| DeclarationScope::cast(parent).is_some())
            && self
                .shared
                .grammar
                .bindings
                .iter()
                .filter(|rule| rule.node == kind)
                .count()
                <= 1
            && self
                .shared
                .grammar
                .bindings
                .iter()
                .filter(|rule| rule.node == kind)
                .all(|rule| {
                    rule.name.is_none()
                        && rule.scopes == ["statement_block"]
                        && rule.kind == vvv_core::SymbolKind::Variable
                        && rule.namespace == vvv_core::BindingNamespace::Value
                        && !rule.after
                })
        {
            return;
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

    struct BindingFixture {
        source: String,
        facts: vvv_core::Facts,
    }

    impl BindingFixture {
        fn new(source: &str, language: &dyn vvv_core::Language) -> Self {
            Self {
                source: source.to_owned(),
                facts: language.facts(source).unwrap(),
            }
        }

        fn span(&self, text: &str, occurrence: usize) -> vvv_core::Span {
            let start = self.source.match_indices(text).nth(occurrence).unwrap().0;
            vvv_core::Span::new(start, start + text.len())
        }

        fn eligible(&self, span: vvv_core::Span) -> bool {
            self.facts.lexical_tokens.contains(&span) || self.facts.navigation.contains(&span)
        }
    }

    #[test]
    fn file_owners_publish_lexical_initialization_and_hoisted_sites_for_ts_and_tsx() {
        let source = "import { external } from './peer'; export {}; fn(); var repeated; var repeated; let earlier = later; const later = external; function fn() {} earlier;";
        for language in [
            &crate::typescript::TypeScript::default() as &dyn vvv_core::Language,
            &crate::typescript::Tsx::default(),
        ] {
            let fixture = BindingFixture::new(source, language);
            let root = vvv_core::Span::new(0, source.len());
            for name in ["fn", "repeated", "earlier", "later"] {
                assert!(
                    fixture
                        .facts
                        .lexical
                        .iter()
                        .any(|binding| binding.symbol.name == name && binding.scope == root),
                    "{name}"
                );
            }
            let later = fixture
                .facts
                .lexical
                .iter()
                .find(|binding| binding.symbol.name == "later")
                .unwrap();
            assert!(!later.initialized(fixture.span("later", 0)));
            assert!(fixture.eligible(fixture.span("later", 0)));
            assert_eq!(
                fixture
                    .facts
                    .lexical
                    .iter()
                    .filter(|binding| binding.symbol.name == "repeated")
                    .count(),
                2
            );
        }
    }

    #[test]
    fn strict_block_functions_use_block_owners_and_scripts_keep_legacy_barriers() {
        for prefix in ["export {};", "'use strict';", "\"use strict\";"] {
            let source = format!(
                "{prefix} function outer(seed) {{ {{ helper(); function helper() {{ return seed; }} }} seed; }}"
            );
            let fixture = BindingFixture::new(&source, &crate::typescript::TypeScript::default());
            let helper = fixture
                .facts
                .lexical
                .iter()
                .find(|binding| binding.symbol.name == "helper")
                .unwrap();
            assert!(helper.scope.contains(&fixture.span("helper", 0)));
            assert!(!helper.scope.contains(&fixture.span("seed", 2)));
            assert!(fixture.eligible(fixture.span("seed", 2)));
        }
        for source in [
            "function outer(seed) { { function helper() {} } seed; }",
            "'use strict'; function outer(seed) { if (seed) function helper() {} seed; }",
            "function outer(seed) { '\\x75se strict'; { function helper() {} } seed; }",
            "function outer(seed) { seed; 'use strict'; { function helper() {} } seed; }",
        ] {
            let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
            assert!(
                !fixture.eligible(fixture.span("seed", source.matches("seed").count() - 1)),
                "{source}"
            );
        }
    }

    #[test]
    fn switch_cases_share_a_lexical_environment_but_not_the_scrutinee() {
        let source = "export {}; let value = 1; switch(value) { case value: local(); function local() {} let value = 2; break; default: value; } value;";
        let fixture = BindingFixture::new(source, &crate::typescript::Tsx::default());
        let bindings = fixture
            .facts
            .lexical
            .iter()
            .filter(|binding| binding.symbol.name == "value")
            .collect::<Vec<_>>();
        assert_eq!(bindings.len(), 2);
        let inner = bindings[1];
        assert!(!inner.scope.contains(&fixture.span("value", 1)));
        assert!(inner.scope.contains(&fixture.span("value", 2)));
        assert!(!inner.initialized(fixture.span("value", 2)));
        assert!(!inner.initialized(fixture.span("value", 4)));
        assert!(!inner.scope.contains(&fixture.span("value", 5)));
        assert!(fixture.eligible(fixture.span("local", 0)));
    }

    #[test]
    fn overload_headers_preserve_all_sites_and_own_their_generic_and_value_parameters() {
        let source = "function outer() { use(1); function use<T>(value: T): T; function use(value: number) { return value; } }";
        let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
        let functions = fixture
            .facts
            .lexical
            .iter()
            .filter(|binding| binding.symbol.name == "use")
            .collect::<Vec<_>>();
        assert_eq!(functions.len(), 2);
        assert_eq!(functions[0].scope, functions[1].scope);
        assert!(fixture.eligible(fixture.span("use", 0)));
        let parameters = fixture
            .facts
            .lexical
            .iter()
            .filter(|binding| binding.symbol.name == "value")
            .collect::<Vec<_>>();
        assert_eq!(parameters.len(), 2);
        assert_ne!(parameters[0].scope, parameters[1].scope);
        assert!(fixture.eligible(fixture.span("T", 1)));
        assert!(parameters[1].scope.contains(&fixture.span("value", 2)));
    }

    #[test]
    fn static_and_namespace_bodies_own_vars_without_leaking_into_other_owners() {
        let source = "export {}; class C { static { inner; var inner; } method() { inner; } } namespace N { item; export function helper() {} var item; } item;";
        let fixture = BindingFixture::new(source, &crate::typescript::Tsx::default());
        let inner = fixture
            .facts
            .lexical
            .iter()
            .find(|binding| binding.symbol.name == "inner")
            .unwrap();
        assert!(inner.scope.contains(&fixture.span("inner", 0)));
        assert!(!inner.scope.contains(&fixture.span("inner", 2)));
        let item = fixture
            .facts
            .lexical
            .iter()
            .find(|binding| binding.symbol.name == "item")
            .unwrap();
        assert!(item.scope.contains(&fixture.span("item", 0)));
        assert!(!item.scope.contains(&fixture.span("item", 2)));
        assert!(fixture.eligible(fixture.span("item", 0)));
    }

    #[test]
    fn local_named_types_and_class_self_names_preserve_separate_namespaces() {
        let source = "function f() { let x: I; interface I {} type A = I; class C { m(): C { return new C(); } } let c: C; }";
        let fixture = BindingFixture::new(source, &crate::typescript::Tsx::default());
        for name in ["I", "A"] {
            assert!(
                fixture
                    .facts
                    .lexical
                    .iter()
                    .filter(|binding| binding.symbol.name == name)
                    .all(|binding| binding.namespace == vvv_core::BindingNamespace::Type)
            );
        }
        let self_name = fixture
            .facts
            .lexical
            .iter()
            .find(|binding| {
                binding.symbol.name == "C"
                    && binding.scope.contains(&fixture.span("C", 1))
                    && !binding.scope.contains(&fixture.span("C", 3))
            })
            .unwrap();
        assert!(self_name.initialized(fixture.span("C", 2)));
        assert!(fixture.eligible(fixture.span("I", 0)));
        let source = "const View = class C<C> { m(value: C) { return C; } };";
        let fixture = BindingFixture::new(source, &crate::typescript::Tsx::default());
        let names = fixture
            .facts
            .lexical
            .iter()
            .filter(|binding| {
                binding.symbol.name == "C" && binding.namespace == vvv_core::BindingNamespace::Type
            })
            .collect::<Vec<_>>();
        assert!(
            names
                .iter()
                .any(|binding| binding.symbol.kind == vvv_core::SymbolKind::TypeParameter)
        );
        assert!(
            !names
                .iter()
                .any(|binding| binding.symbol.kind == vvv_core::SymbolKind::Class)
        );
    }

    #[test]
    fn assignment_patterns_reuse_structure_without_publishing_bindings_or_static_labels() {
        let source = "function f(entries, target, object) { for ({ label: target, [target]: object.field, ...object.rest } of entries) { target; } for ([object[target], target = target] of entries) {} }";
        let fixture = BindingFixture::new(source, &crate::typescript::Tsx::default());
        assert!(
            fixture
                .facts
                .lexical
                .iter()
                .filter(|binding| binding.symbol.kind == vvv_core::SymbolKind::Variable)
                .count()
                == 0
        );
        assert!(!fixture.eligible(fixture.span("label", 0)));
        for occurrence in 1..source.matches("target").count() {
            assert!(
                fixture.eligible(fixture.span("target", occurrence)),
                "{occurrence}"
            );
        }
    }

    #[test]
    fn conflicting_imports_lexical_and_nested_var_declarations_keep_owners_conservative() {
        for source in [
            "import { value } from './peer'; let value; value;",
            "export {}; function f(seed) { { let value; var value; } seed; }",
            "export {}; function f(seed) { { function value() {} var value; } seed; }",
            "export {}; function f(seed) { try {} catch ({ value }) { var value; } seed; }",
            "function f(seed) { let value; function value() {} seed; }",
        ] {
            let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
            let token = if source.contains("seed") {
                "seed"
            } else {
                "value"
            };
            assert!(
                !fixture.eligible(fixture.span(token, source.matches(token).count() - 1)),
                "{source}"
            );
        }
        for source in [
            "function f(seed) { try {} catch (value) { var value; } seed; }",
            "function f(seed) { var value; { let value; } seed; }",
        ] {
            let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
            assert!(fixture.eligible(fixture.span("seed", 1)), "{source}");
        }
    }

    #[test]
    fn enum_members_shadow_outer_values_without_forward_or_self_initialization_fallback() {
        let source = "const A = 1; enum E { A = 2, B = A, C = D, D = 4, F = E } A;";
        let fixture = BindingFixture::new(source, &crate::typescript::Tsx::default());
        let member = fixture
            .facts
            .lexical
            .iter()
            .find(|binding| {
                binding.symbol.kind == vvv_core::SymbolKind::Variant && binding.symbol.name == "A"
            })
            .unwrap();
        assert!(member.initialized(fixture.span("A", 2)));
        assert!(!member.scope.contains(&fixture.span("A", 3)));
        let later = fixture
            .facts
            .lexical
            .iter()
            .find(|binding| {
                binding.symbol.kind == vvv_core::SymbolKind::Variant && binding.symbol.name == "D"
            })
            .unwrap();
        assert!(!later.initialized(fixture.span("D", 0)));
        assert!(fixture.eligible(fixture.span("A", 2)));
        let source = "enum E { A = 1, ['dynamic'] = 2, B = A }";
        let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
        assert!(
            !fixture
                .facts
                .lexical
                .iter()
                .any(|binding| binding.symbol.kind == vvv_core::SymbolKind::Variant)
        );
        assert!(!fixture.eligible(fixture.span("A", 1)));
    }

    #[test]
    fn catch_lexical_conflicts_block_only_the_catch_body() {
        for declaration in ["let value;", "const value = 1;", "function value() {}"] {
            let source = format!(
                "export {{}}; function f(seed) {{ try {{}} catch (value) {{ {declaration} seed; }} seed; }}"
            );
            let fixture = BindingFixture::new(&source, &crate::typescript::TypeScript::default());
            assert!(!fixture.eligible(fixture.span("seed", 1)));
            assert!(fixture.eligible(fixture.span("seed", 2)));
        }
    }

    #[test]
    fn merged_ambient_and_qualified_namespace_bodies_do_not_claim_member_ownership() {
        for source in [
            "namespace N { var item; item; } namespace N { item; }",
            "declare namespace N { const item: number; item; }",
            "namespace N.Inner { var item; item; }",
        ] {
            let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
            assert!(
                !fixture.eligible(fixture.span("item", source.matches("item").count() - 1)),
                "{source}"
            );
        }
    }

    #[test]
    fn callable_redeclarations_preserve_shared_sites_and_separate_expression_environments() {
        for (header, shared) in [
            ("value", true),
            ("{ value }", true),
            ("...value", true),
            ("value = 1", false),
            ("{ value = 1 }", false),
            ("{ [key]: value }", false),
        ] {
            let source = format!("function f({header}) {{ var value; return value; }}");
            let fixture = BindingFixture::new(&source, &crate::typescript::TypeScript::default());
            let variables = fixture
                .facts
                .lexical
                .iter()
                .filter(|binding| {
                    binding.symbol.name == "value"
                        && binding.symbol.kind == vvv_core::SymbolKind::Variable
                })
                .collect::<Vec<_>>();
            assert_eq!(variables.len(), 1, "{source}");
            let peers = fixture
                .facts
                .lexical
                .iter()
                .filter(|binding| {
                    binding.symbol.name == "value" && binding.scope == variables[0].scope
                })
                .collect::<Vec<_>>();
            assert_eq!(peers.len(), if shared { 2 } else { 1 }, "{source}");
            assert!(fixture.eligible(fixture.span("value", source.matches("value").count() - 1)));
            assert!(peers.iter().all(|binding| binding.uninitialized.is_empty()));
        }
    }

    #[test]
    fn named_expression_and_arrow_parameter_environments_retain_shared_declarations() {
        for source in [
            "const f = function named(value) { var value; return value; };",
            "const f = function* named(value) { var value; yield value; };",
            "const f = value => { var value; return value; };",
            "class View { method(value) { var value; return value; } }",
        ] {
            let fixture = BindingFixture::new(source, &crate::typescript::Tsx::default());
            let body = fixture
                .facts
                .lexical
                .iter()
                .find(|binding| {
                    binding.symbol.kind == vvv_core::SymbolKind::Variable
                        && binding.symbol.name == "value"
                })
                .unwrap()
                .scope;
            assert_eq!(
                fixture
                    .facts
                    .lexical
                    .iter()
                    .filter(|binding| binding.symbol.name == "value" && binding.scope == body)
                    .count(),
                2,
                "{source}"
            );
        }
    }

    #[test]
    fn parameter_function_and_var_sites_are_preserved_without_duplicate_parameter_projections() {
        let fixture = BindingFixture::new(
            "function f(value) { var value; function value() {} var value; value; }",
            &crate::typescript::Tsx::default(),
        );
        let scope = fixture
            .facts
            .lexical
            .iter()
            .find(|binding| binding.symbol.kind == vvv_core::SymbolKind::Variable)
            .unwrap()
            .scope;
        let peers = fixture
            .facts
            .lexical
            .iter()
            .filter(|binding| binding.symbol.name == "value" && binding.scope == scope)
            .collect::<Vec<_>>();
        assert_eq!(peers.len(), 4);
        assert_eq!(
            peers
                .iter()
                .filter(|binding| binding.symbol.kind == vvv_core::SymbolKind::Parameter)
                .count(),
            1
        );
        assert!(fixture.eligible(fixture.span("value", 4)));
    }

    #[test]
    fn var_names_own_the_body_without_temporal_dead_zones_or_parameter_default_leakage() {
        let source = "function f(seed = outer) { outer; if (seed) { var outer = seed; } const capture = () => outer; return outer; } outer;";
        for language in [
            &crate::typescript::TypeScript::default() as &dyn vvv_core::Language,
            &crate::typescript::Tsx::default(),
        ] {
            let fixture = BindingFixture::new(source, language);
            let binding = fixture
                .facts
                .lexical
                .iter()
                .find(|binding| binding.symbol.name == "outer")
                .unwrap();
            assert_eq!(binding.symbol.kind, vvv_core::SymbolKind::Variable);
            assert_eq!(binding.visible_from, binding.scope.start);
            assert!(binding.explicit);
            assert!(binding.uninitialized.is_empty());
            assert!(!binding.scope.contains(&fixture.span("outer", 0)));
            assert!(!binding.scope.contains(&fixture.span("outer", 5)));
            for occurrence in [1, 3, 4] {
                let use_span = fixture.span("outer", occurrence);
                assert!(binding.visible("outer", use_span, vvv_core::BindingNamespace::Value));
                assert!(binding.initialized(use_span));
                assert!(fixture.eligible(use_span));
            }
        }
    }

    #[test]
    fn var_patterns_preserve_labels_rest_and_preexisting_binding_evidence() {
        let source = "function f(input) { first; var { label: first = first, [later]: second = later, ...rest } = input, [later, , ...tail] = input; later; }";
        let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
        let variables: Vec<_> = fixture
            .facts
            .lexical
            .iter()
            .filter(|binding| binding.symbol.kind == vvv_core::SymbolKind::Variable)
            .collect();
        assert_eq!(
            variables
                .iter()
                .map(|binding| binding.symbol.name.as_str())
                .collect::<Vec<_>>(),
            ["first", "second", "rest", "later", "tail"]
        );
        assert!(
            variables
                .iter()
                .all(|binding| binding.uninitialized.is_empty())
        );
        assert!(!fixture.eligible(fixture.span("label", 0)));
        for (text, occurrence) in [
            ("first", 0),
            ("first", 2),
            ("later", 0),
            ("later", 1),
            ("later", 3),
        ] {
            assert!(fixture.eligible(fixture.span(text, occurrence)));
        }
    }

    #[test]
    fn var_forms_share_callable_ownership_for_methods_expressions_and_loop_headers() {
        for source in [
            "function f(entries) { item; for (var item = 0; item < 2; item++) { item; } item; }",
            "function f(entries) { item; for (var { label: item = item } of entries) { item; } item; }",
            "function f(entries) { item; for (var item in entries) { item; } item; }",
            "async function f(entries) { item; for await (var item of entries) { item; } item; }",
            "const f = function named() { item; var item; return item; };",
            "const f = function* named() { item; var item; yield item; };",
            "function* f() { item; var item; yield item; }",
            "class View { method() { item; var item; return item; } }",
        ] {
            let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
            let binding = fixture
                .facts
                .lexical
                .iter()
                .find(|binding| binding.symbol.name == "item")
                .unwrap();
            assert_eq!(
                fixture
                    .facts
                    .lexical
                    .iter()
                    .filter(|binding| binding.symbol.name == "item")
                    .count(),
                1
            );
            assert!(binding.scope.contains(&fixture.span("item", 0)));
            assert!(fixture.eligible(fixture.span("item", 0)));
            assert!(fixture.eligible(fixture.span("item", source.matches("item").count() - 1)));
        }
    }

    #[test]
    fn var_collection_stops_at_nested_callables_and_preserves_duplicate_sites() {
        let source = "function f(seed) { value; var value; var value = seed; const inner = () => { value; var value = seed; }; value; }";
        let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
        let variables: Vec<_> = fixture
            .facts
            .lexical
            .iter()
            .filter(|binding| binding.symbol.name == "value")
            .collect();
        assert_eq!(variables.len(), 3);
        assert_eq!(variables[0].scope, variables[1].scope);
        assert_ne!(variables[0].symbol.name_span, variables[1].symbol.name_span);
        assert_ne!(variables[0].scope, variables[2].scope);
        assert!(!variables[2].scope.contains(&fixture.span("value", 0)));
        assert!(fixture.eligible(fixture.span("value", 3)));
        for source in [
            "function f(seed) { const View = class { static { var hidden; hidden; } }; seed; }",
            "function f(seed) { var known; const View = class { static { var hidden; hidden; } }; seed; }",
        ] {
            let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
            assert!(
                fixture
                    .facts
                    .lexical
                    .iter()
                    .any(|binding| binding.symbol.name == "hidden")
            );
            assert!(fixture.eligible(fixture.span("hidden", 1)));
            let hidden = fixture
                .facts
                .lexical
                .iter()
                .find(|binding| binding.symbol.name == "hidden")
                .unwrap();
            assert!(!hidden.scope.contains(&fixture.span("seed", 1)));
        }
    }

    #[test]
    fn var_conflicts_and_unsupported_groups_block_the_body_without_publishing_a_prefix() {
        for source in [
            "function f(seed) { var good; var local; let local; seed; }",
            "function f(seed) { var good; var [bad, ...rest, last] = seed; seed; }",
            "function f(seed) { var good; var { missing: } = seed; seed; }",
            "function f(seed) { var good; for (var item = 1 in entries) {} seed; }",
        ] {
            let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
            assert!(
                !fixture
                    .facts
                    .lexical
                    .iter()
                    .any(|binding| binding.symbol.name == "good"),
                "{source}"
            );
            let last = source
                .rfind(if source.contains("f(value)") {
                    "value"
                } else {
                    "seed"
                })
                .unwrap();
            let text = if source.contains("f(value)") {
                "value"
            } else {
                "seed"
            };
            assert!(
                !fixture.eligible(vvv_core::Span::new(last, last + text.len())),
                "{source}"
            );
        }
    }

    #[test]
    fn var_ownership_and_pattern_traversals_are_bounded_and_only_run_for_var_owners() {
        for source in [
            format!(
                "function f(seed) {{ var good; {}seed;{} seed; }}",
                "{".repeat(129),
                "}".repeat(129)
            ),
            format!(
                "function f(seed) {{ var good; {} seed; }}",
                "seed;".repeat(1_025)
            ),
            format!(
                "function f(seed) {{ var good; var {}bad{} = seed; seed; }}",
                "[".repeat(129),
                "]".repeat(129)
            ),
        ] {
            let fixture = BindingFixture::new(&source, &crate::typescript::TypeScript::default());
            assert!(
                !fixture
                    .facts
                    .lexical
                    .iter()
                    .any(|binding| binding.symbol.name == "good")
            );
            let last = source.rfind("seed").unwrap();
            assert!(!fixture.eligible(vvv_core::Span::new(last, last + 4)));
        }
        let source = format!("function f(seed) {{ {} }}", "seed;".repeat(1_025));
        let fixture = BindingFixture::new(&source, &crate::typescript::TypeScript::default());
        assert!(fixture.eligible(fixture.span("seed", 1_025)));
    }

    #[test]
    fn custom_var_rules_retain_table_scope_and_field_interpretation() {
        const CUSTOM: vvv_core::BindingRule = vvv_core::BindingRule {
            node: "variable_declaration",
            name: Some("name"),
            scopes: &["statement_block"],
            kind: vvv_core::SymbolKind::Variable,
            namespace: vvv_core::BindingNamespace::Value,
            after: true,
        };
        for rules in [&[CUSTOM][..], &[CUSTOM, CUSTOM][..]] {
            let tables = crate::syntax::AstGrepSearcher::new(
                TypeScript,
                Grammar {
                    bindings: rules,
                    ..Grammar::EMPTY
                },
            );
            let typed = tables
                .clone()
                .with_navigation_syntax(crate::syntax::NavigationSyntax::TypeScript);
            let source = "function f() { var local; local; }";
            assert_eq!(
                format!("{:?}", typed.facts(source).unwrap()),
                format!("{:?}", tables.facts(source).unwrap())
            );
        }
    }

    #[test]
    fn direct_functions_own_the_body_before_declaration_and_exclude_parameter_defaults() {
        let source = "function outer(seed = helper) { helper(); function helper(value = 1) { return helper(value); } const capture = () => helper(); } helper();";
        for language in [
            &crate::typescript::TypeScript::default() as &dyn vvv_core::Language,
            &crate::typescript::Tsx::default(),
        ] {
            let fixture = BindingFixture::new(source, language);
            let binding = fixture
                .facts
                .lexical
                .iter()
                .find(|binding| {
                    binding.symbol.name == "helper"
                        && binding.symbol.kind == vvv_core::SymbolKind::Function
                })
                .unwrap();
            assert_eq!(binding.symbol.name_span, fixture.span("helper", 2));
            assert_eq!(binding.visible_from, binding.scope.start);
            assert!(binding.explicit);
            assert!(binding.uninitialized.is_empty());
            assert!(!binding.scope.contains(&fixture.span("helper", 0)));
            for occurrence in [1, 3, 4] {
                let use_span = fixture.span("helper", occurrence);
                assert!(binding.visible("helper", use_span, vvv_core::BindingNamespace::Value));
                assert!(binding.initialized(use_span));
                assert!(fixture.eligible(use_span));
            }
            assert!(!binding.scope.contains(&fixture.span("helper", 5)));
            assert!(
                fixture
                    .facts
                    .signatures
                    .iter()
                    .any(|signature| signature.name_span == binding.symbol.name_span)
            );
        }
    }

    #[test]
    fn generators_and_all_supported_block_body_callables_hoist_direct_functions() {
        for source in [
            "function outer() { helper(); function* helper(value = 1) { yield helper(value); } }",
            "const outer = () => { helper(); function helper() {} };",
            "const outer = function named() { helper(); function helper() {} };",
            "const outer = function* named() { helper(); function helper() {} };",
            "class View { method() { helper(); function helper() {} } }",
            "const outer = () => { helper(); function helper() { return <div />; } };",
        ] {
            let fixture = BindingFixture::new(source, &crate::typescript::Tsx::default());
            let binding = fixture
                .facts
                .lexical
                .iter()
                .find(|binding| binding.symbol.name == "helper")
                .unwrap();
            assert_eq!(binding.symbol.kind, vvv_core::SymbolKind::Function);
            assert!(binding.scope.contains(&fixture.span("helper", 0)));
            assert!(fixture.eligible(fixture.span("helper", 0)));
        }
    }

    #[test]
    fn duplicate_function_names_preserve_distinct_declaration_candidates() {
        let fixture = BindingFixture::new(
            "function outer() { same(); function same() {} function same() {} }",
            &crate::typescript::TypeScript::default(),
        );
        let bindings: Vec<_> = fixture
            .facts
            .lexical
            .iter()
            .filter(|binding| binding.symbol.name == "same")
            .collect();
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].scope, bindings[1].scope);
        assert_ne!(bindings[0].symbol.name_span, bindings[1].symbol.name_span);
        assert!(fixture.eligible(fixture.span("same", 0)));
    }

    #[test]
    fn nested_conditional_overloaded_and_malformed_functions_keep_conservative_owners() {
        for source in [
            "function outer(seed) { seed; { function helper() {} } seed; }",
            "function outer(seed) { seed; if (seed) function helper() {} seed; }",
            "function outer(seed) { seed; try {} catch (_) { function helper() {} } seed; }",
            "function outer(seed) { seed; function good() {} function broken(value: ) {} seed; }",
        ] {
            let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
            assert!(!fixture.eligible(fixture.span("seed", 1)), "{source}");
            assert!(
                !fixture.eligible(fixture.span("seed", source.matches("seed").count() - 1)),
                "{source}"
            );
            if source.contains("broken") || source.contains("function helper(value: number):") {
                assert!(
                    fixture
                        .facts
                        .lexical
                        .iter()
                        .all(
                            |binding| binding.symbol.kind != vvv_core::SymbolKind::Function
                                || binding.symbol.name == "outer"
                        ),
                    "{source}"
                );
            }
        }
    }

    #[test]
    fn hoisted_function_group_limits_publish_no_prefix() {
        let source = format!(
            "function outer(seed) {{ seed; {} seed; }}",
            (0..1_025)
                .map(|index| format!("function f{index}() {{}} "))
                .collect::<String>()
        );
        let fixture = BindingFixture::new(&source, &crate::typescript::TypeScript::default());
        assert!(
            fixture
                .facts
                .lexical
                .iter()
                .all(
                    |binding| binding.symbol.kind != vvv_core::SymbolKind::Function
                        || binding.symbol.name == "outer"
                )
        );
        assert!(!fixture.eligible(fixture.span("seed", 1)));
    }

    #[test]
    fn custom_and_competing_function_rules_retain_table_interpretation() {
        const CUSTOM: vvv_core::BindingRule = vvv_core::BindingRule {
            node: "function_declaration",
            name: Some("name"),
            scopes: &["statement_block"],
            kind: vvv_core::SymbolKind::Variable,
            namespace: vvv_core::BindingNamespace::Value,
            after: true,
        };
        const STANDARD: vvv_core::BindingRule = vvv_core::BindingRule {
            kind: vvv_core::SymbolKind::Function,
            after: false,
            ..CUSTOM
        };
        for rules in [
            &[CUSTOM][..],
            &[CUSTOM, CUSTOM][..],
            &[STANDARD, STANDARD][..],
        ] {
            let grammar = Grammar {
                bindings: rules,
                ..Grammar::EMPTY
            };
            let tables = crate::syntax::AstGrepSearcher::new(TypeScript, grammar);
            let typed = tables
                .clone()
                .with_navigation_syntax(crate::syntax::NavigationSyntax::TypeScript);
            let source = "function outer() { helper; function helper() {} }";
            assert_eq!(
                format!("{:?}", typed.facts(source).unwrap()),
                format!("{:?}", tables.facts(source).unwrap())
            );
        }
    }

    #[test]
    fn parameter_patterns_publish_exact_binding_spans_for_ts_and_tsx() {
        let source = "function f({ label: renamed, nested: { inner }, short, count = seed, ... /* marker */ rest }, [first, , ... /* marker */ tail]) { renamed; inner; short; count; rest; first; tail; }";
        for language in [
            &crate::typescript::TypeScript::default() as &dyn vvv_core::Language,
            &crate::typescript::Tsx::default(),
        ] {
            let fixture = BindingFixture::new(source, language);
            let names: Vec<_> = fixture
                .facts
                .lexical
                .iter()
                .filter(|binding| binding.symbol.kind == vvv_core::SymbolKind::Parameter)
                .map(|binding| {
                    assert_eq!(binding.symbol.kind, vvv_core::SymbolKind::Parameter);
                    assert_eq!(binding.namespace, vvv_core::BindingNamespace::Value);
                    assert_eq!(binding.scope, vvv_core::Span::new(0, source.len()));
                    assert_eq!(
                        binding.symbol.name_span,
                        fixture.span(&binding.symbol.name, 0)
                    );
                    binding.symbol.name.as_str()
                })
                .collect();
            assert_eq!(
                names,
                [
                    "renamed", "inner", "short", "count", "rest", "first", "tail"
                ]
            );
            assert!(!fixture.eligible(fixture.span("label", 0)));
            assert!(!fixture.eligible(fixture.span("nested", 0)));
            for name in names {
                assert!(fixture.eligible(fixture.span(name, 1)), "{name}");
            }
        }
    }

    #[test]
    fn default_and_computed_evaluation_preserve_order_and_block_tdz_fallback() {
        let source = "function f({ first = seed, second = first, [second]: third, fourth = later }, later = third) { first; second; third; fourth; later; }";
        let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
        for (name, occurrence) in [("seed", 0), ("first", 1), ("second", 1), ("third", 1)] {
            let span = fixture.span(name, occurrence);
            assert!(fixture.eligible(span), "{name}");
            if name != "seed" {
                let binding = fixture
                    .facts
                    .lexical
                    .iter()
                    .find(|binding| binding.symbol.name == name)
                    .unwrap();
                assert!(binding.visible(name, span, vvv_core::BindingNamespace::Value));
            }
        }
        let forward = fixture.span("later", 0);
        assert!(fixture.eligible(forward));
        let later = fixture
            .facts
            .lexical
            .iter()
            .find(|binding| binding.symbol.name == "later")
            .unwrap();
        assert!(later.visible("later", forward, vvv_core::BindingNamespace::Value));
        assert!(!later.initialized(forward));

        let source = "function f({ first = first } = first, later = first) { first; }";
        let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
        assert!(fixture.eligible(fixture.span("first", 1)));
        assert!(fixture.eligible(fixture.span("first", 2)));
        assert!(fixture.eligible(fixture.span("first", 3)));
        let first = fixture
            .facts
            .lexical
            .iter()
            .find(|binding| binding.symbol.name == "first")
            .unwrap();
        assert!(!first.initialized(fixture.span("first", 1)));
        assert!(!first.initialized(fixture.span("first", 2)));
        assert!(first.visible(
            "first",
            fixture.span("first", 3),
            vvv_core::BindingNamespace::Value
        ));
    }

    #[test]
    fn unsupported_or_unfinished_headers_publish_no_parameter_prefix() {
        for source in [
            "function f(good, [first, ...rest, last]) { good; }",
            "function f(good, { first, ...rest, last }) { good; }",
            "function f(good, { ...[rest] }) { good; }",
            "function f(good, ...rest,) { good; }",
            "function f(good, [...rest,]) { good; }",
            "function f(good, { first: }) { good; }",
            "function f(good, first = @) { good; }",
        ] {
            for language in [
                &crate::typescript::TypeScript::default() as &dyn vvv_core::Language,
                &crate::typescript::Tsx::default(),
            ] {
                let fixture = BindingFixture::new(source, language);
                assert!(
                    !fixture
                        .facts
                        .lexical
                        .iter()
                        .any(|binding| binding.symbol.kind == vvv_core::SymbolKind::Parameter),
                    "{source}"
                );
                assert!(!fixture.eligible(fixture.span("good", 1)), "{source}");
            }
        }
    }

    #[test]
    fn method_patterns_keep_type_namespace_duplicates_and_existing_barriers() {
        let source = "class C { m<T>({ T: value }: T, [same, same]) { value; same; } }";
        let fixture = BindingFixture::new(source, &crate::typescript::Tsx::default());
        let names: Vec<_> = fixture
            .facts
            .lexical
            .iter()
            .filter(|binding| {
                matches!(
                    binding.symbol.kind,
                    vvv_core::SymbolKind::Parameter | vvv_core::SymbolKind::TypeParameter
                )
            })
            .map(|binding| binding.symbol.name.as_str())
            .collect();
        assert_eq!(names, ["T", "value", "same", "same"]);
        let type_use = fixture.span("T", 2);
        assert!(fixture.facts.navigation_types.contains(&type_use));
        assert!(fixture.eligible(type_use));

        for source in [
            "function f({ value }) { return () => value; }",
            "function f({ value }) { return function() { return value; }; }",
            "function* f({ value }) { value; }",
        ] {
            let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
            assert!(fixture.eligible(fixture.span("value", 1)), "{source}");
        }
    }

    #[test]
    fn callable_forms_share_exact_parameter_owners_and_captured_outer_scopes() {
        for source in [
            "function outer(captured) { const callback = value => value + captured; captured; }",
            "function outer(captured) { const callback = function(value) { value; captured; }; captured; }",
            "function outer(captured) { const callback = function*(value) { yield value; captured; }; captured; }",
            "function* outer(captured) { captured; }",
        ] {
            for language in [
                &crate::typescript::TypeScript::default() as &dyn vvv_core::Language,
                &crate::typescript::Tsx::default(),
            ] {
                let fixture = BindingFixture::new(source, language);
                assert!(fixture.eligible(fixture.span("captured", 1)), "{source}");
                if source.contains("value") {
                    let binding = fixture
                        .facts
                        .lexical
                        .iter()
                        .find(|binding| binding.symbol.name == "value")
                        .unwrap();
                    assert!(binding.scope.contains(&fixture.span("value", 1)));
                    assert!(fixture.eligible(fixture.span("value", 1)));
                    assert!(!binding.scope.contains(&fixture.span("captured", 2)));
                }
            }
        }
    }

    #[test]
    fn nested_default_bindings_override_outer_uninitialized_parameter_ownership() {
        let fixture = BindingFixture::new(
            "function f(value = (value => value)) { value; }",
            &crate::typescript::TypeScript::default(),
        );
        let bindings: Vec<_> = fixture
            .facts
            .lexical
            .iter()
            .filter(|binding| binding.symbol.name == "value")
            .collect();
        assert_eq!(bindings.len(), 2);
        let use_span = fixture.span("value", 2);
        assert!(fixture.eligible(use_span));
        assert!(!bindings[0].initialized(use_span));
        assert!(bindings[1].initialized(use_span));
        assert!(bindings[0].scope.contains(&bindings[1].scope));
        assert_ne!(bindings[0].scope, bindings[1].scope);
    }

    #[test]
    fn expression_names_enclose_parameters_but_never_escape_the_callable() {
        let fixture = BindingFixture::new(
            "const callback = function same(same) { same; }; same;",
            &crate::typescript::TypeScript::default(),
        );
        let bindings: Vec<_> = fixture
            .facts
            .lexical
            .iter()
            .filter(|binding| binding.symbol.name == "same")
            .collect();
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].symbol.kind, vvv_core::SymbolKind::Function);
        assert_eq!(bindings[1].symbol.kind, vvv_core::SymbolKind::Parameter);
        assert!(bindings[0].scope.contains(&bindings[1].scope));
        assert_ne!(bindings[0].scope, bindings[1].scope);
        assert!(!bindings[0].scope.contains(&fixture.span("same", 3)));
        assert!(
            !fixture
                .facts
                .symbols
                .iter()
                .any(|symbol| symbol.name == "same")
        );
    }

    #[test]
    fn catch_patterns_preserve_order_optional_headers_and_exact_regions() {
        let source = "function f(outer) { try {} catch ({ first = outer, second = first, ...rest }) { first; rest; } outer; }";
        let fixture = BindingFixture::new(source, &crate::typescript::Tsx::default());
        let first = fixture
            .facts
            .lexical
            .iter()
            .find(|binding| binding.symbol.name == "first")
            .unwrap();
        assert_eq!(first.symbol.kind, vvv_core::SymbolKind::Variable);
        assert!(first.initialized(fixture.span("first", 1)));
        let after = fixture.source.rfind("outer").unwrap();
        assert!(!first.scope.contains(&vvv_core::Span::new(after, after + 5)));
        let fixture = BindingFixture::new(
            "function f(outer) { try {} catch { outer; } }",
            &crate::typescript::TypeScript::default(),
        );
        assert_eq!(
            fixture
                .facts
                .lexical
                .iter()
                .filter(|binding| binding.symbol.kind == vvv_core::SymbolKind::Parameter)
                .count(),
            1
        );
        assert!(fixture.eligible(fixture.span("outer", 1)));
    }

    #[test]
    fn var_owners_stop_at_the_nearest_callable_and_invalid_headers_publish_no_prefix() {
        let fixture = BindingFixture::new(
            "function f(outer) { const callback = value => { var outer; value; }; outer; }",
            &crate::typescript::TypeScript::default(),
        );
        assert!(fixture.eligible(fixture.span("value", 1)));
        assert!(fixture.eligible(fixture.span("outer", 2)));
        for source in [
            "function f(outer) { const callback = (good, [bad, ...rest, last]) => good; outer; }",
            "function f(outer) { try {} catch ([good, ...rest, last]) { good; } outer; }",
        ] {
            let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
            assert!(
                !fixture
                    .facts
                    .lexical
                    .iter()
                    .any(|binding| binding.symbol.name == "good")
            );
            assert!(!fixture.eligible(fixture.span("good", 1)));
            let after = fixture.source.rfind("outer").unwrap();
            assert!(fixture.eligible(vvv_core::Span::new(after, after + 5)));
        }
    }

    #[test]
    fn unknown_arrow_header_rules_never_lift_the_callable_barrier() {
        const SINGLE: vvv_core::BindingRule = vvv_core::BindingRule {
            node: "arrow_function",
            name: Some("parameter"),
            scopes: &["arrow_function"],
            kind: vvv_core::SymbolKind::Parameter,
            namespace: vvv_core::BindingNamespace::Value,
            after: false,
        };
        let grammar = Grammar {
            bindings: &[SINGLE],
            identifiers: &["identifier"],
            lexical_barriers: &["arrow_function"],
            ..Grammar::EMPTY
        };
        let tables = crate::syntax::AstGrepSearcher::new(TypeScript, grammar);
        let typed = tables
            .clone()
            .with_navigation_syntax(crate::syntax::NavigationSyntax::TypeScript);
        let source = "const callback = (unmodeled) => unmodeled;";
        assert_eq!(
            format!("{:?}", typed.facts(source).unwrap()),
            format!("{:?}", tables.facts(source).unwrap())
        );
    }

    #[test]
    fn catch_pattern_limits_and_custom_rules_retain_conservative_ownership() {
        let pattern = format!("{}good{}", "[".repeat(130), "]".repeat(130));
        let source =
            format!("function f(outer) {{ try {{}} catch ({pattern}) {{ good; }} outer; }}");
        let fixture = BindingFixture::new(&source, &crate::typescript::TypeScript::default());
        assert!(
            !fixture
                .facts
                .lexical
                .iter()
                .any(|binding| binding.symbol.name == "good")
        );
        assert!(!fixture.eligible(fixture.span("good", 1)));
        const CUSTOM: vvv_core::BindingRule = vvv_core::BindingRule {
            node: "catch_clause",
            name: Some("parameter"),
            scopes: &["statement_block"],
            kind: vvv_core::SymbolKind::Variable,
            namespace: vvv_core::BindingNamespace::Value,
            after: false,
        };
        for rules in [&[CUSTOM][..], &[CUSTOM, CUSTOM][..]] {
            let grammar = Grammar {
                bindings: rules,
                ..Grammar::EMPTY
            };
            let tables = crate::syntax::AstGrepSearcher::new(TypeScript, grammar);
            let typed = tables
                .clone()
                .with_navigation_syntax(crate::syntax::NavigationSyntax::TypeScript);
            let source = "try {} catch (error) { error; }";
            assert_eq!(
                format!("{:?}", typed.facts(source).unwrap()),
                format!("{:?}", tables.facts(source).unwrap())
            );
        }
    }

    #[test]
    fn local_patterns_preserve_block_ownership_and_initialization_order() {
        let source = "function f(input) { later; let empty; const [first = input, later = first] = input; { const later = input; later; } later; }";
        for language in [
            &crate::typescript::TypeScript::default() as &dyn vvv_core::Language,
            &crate::typescript::Tsx::default(),
        ] {
            let fixture = BindingFixture::new(source, language);
            let locals: Vec<_> = fixture
                .facts
                .lexical
                .iter()
                .filter(|binding| binding.symbol.kind == vvv_core::SymbolKind::Variable)
                .collect();
            assert_eq!(
                locals
                    .iter()
                    .map(|binding| binding.symbol.name.as_str())
                    .collect::<Vec<_>>(),
                ["empty", "first", "later", "later"]
            );
            let later = locals[2];
            let before = fixture.span("later", 0);
            assert!(fixture.eligible(before));
            assert!(later.visible("later", before, vvv_core::BindingNamespace::Value));
            assert!(!later.initialized(before));
            assert!(!locals[1].initialized(fixture.span("input", 2)));
            assert!(locals[1].initialized(fixture.span("first", 1)));
            assert!(!later.initialized(fixture.span("first", 1)));
            assert!(later.initialized(fixture.span("later", 4)));
            assert!(locals.iter().all(|binding| binding.explicit));
            assert!(locals[2].scope.contains(&locals[3].scope));
            assert_ne!(locals[2].scope, locals[3].scope);
        }
    }

    #[test]
    fn unsupported_local_blocks_publish_no_supported_prefix() {
        for source in [
            "function f(input) { const good = input; const missing; good; }",
            "function f(input) { const good = input; let [missing]; good; }",
            "function f(input) { const good = input; let [a, ...rest, last] = input; good; }",
        ] {
            let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
            assert!(
                fixture
                    .facts
                    .lexical
                    .iter()
                    .all(|binding| binding.symbol.kind != vvv_core::SymbolKind::Variable),
                "{source}"
            );
            assert!(!fixture.eligible(fixture.span("good", 1)), "{source}");
        }
    }

    #[test]
    fn loop_views_preserve_header_forms_without_flattening_bodies() {
        let tree = TypeScript.ast_grep("for (let index = 0; index < 2; index++) {} for await (const [head] of entries) {} for (existing in entries) existing;");
        let root = tree.root();
        let mut loops = root.dfs().filter_map(Loop::cast);
        let Loop::For(classic) = loops.next().unwrap() else {
            panic!("classic loop")
        };
        assert_eq!(classic.initializer().unwrap().kind(), "lexical_declaration");
        assert_eq!(classic.body().unwrap().kind(), "statement_block");
        let Loop::Iteration(iteration) = loops.next().unwrap() else {
            panic!("iteration loop")
        };
        assert_eq!(iteration.kind().unwrap().unwrap().text(), "const");
        assert_eq!(iteration.operator().unwrap().text(), "of");
        assert_eq!(iteration.pattern().unwrap().kind(), "array_pattern");
        assert_eq!(iteration.iterable().unwrap().text(), "entries");
        let Loop::Iteration(assignment) = loops.next().unwrap() else {
            panic!("assignment loop")
        };
        assert!(assignment.kind().unwrap().is_none());
        assert_eq!(assignment.operator().unwrap().text(), "in");
        assert_eq!(assignment.body().unwrap().kind(), "expression_statement");
    }

    #[test]
    fn loop_bindings_own_headers_and_bodies_without_leaking_afterward() {
        for source in [
            "function f(input) { for (let input = input; input; input++) { input; } input; }",
            "function f(input) { for (const input of input) { input; } input; }",
        ] {
            for language in [
                &crate::typescript::TypeScript::default() as &dyn vvv_core::Language,
                &crate::typescript::Tsx::default(),
            ] {
                let fixture = BindingFixture::new(source, language);
                let binding = fixture
                    .facts
                    .lexical
                    .iter()
                    .find(|binding| binding.symbol.kind == vvv_core::SymbolKind::Variable)
                    .unwrap();
                let header_use = fixture.span("input", 2);
                assert!(binding.visible("input", header_use, vvv_core::BindingNamespace::Value));
                assert!(!binding.initialized(header_use));
                assert!(fixture.eligible(header_use));
                let after = fixture.source.rfind("input").unwrap();
                assert!(
                    !binding
                        .scope
                        .contains(&vvv_core::Span::new(after, after + 5))
                );
                assert!(fixture.eligible(vvv_core::Span::new(after, after + 5)));
            }
        }
    }

    #[test]
    fn iteration_patterns_share_ordered_defaults_and_atomic_limits() {
        let source = "function f(input) { for (const { first = input, second = first, [second]: third, ...rest } of input) { third; rest; } }";
        let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
        let bindings: Vec<_> = fixture
            .facts
            .lexical
            .iter()
            .filter(|binding| binding.symbol.kind == vvv_core::SymbolKind::Variable)
            .collect();
        assert_eq!(
            bindings
                .iter()
                .map(|binding| binding.symbol.name.as_str())
                .collect::<Vec<_>>(),
            ["first", "second", "third", "rest"]
        );
        assert!(bindings[0].initialized(fixture.span("first", 1)));
        assert!(!bindings[1].initialized(fixture.span("first", 1)));
        assert!(bindings[1].initialized(fixture.span("second", 1)));
        assert!(!bindings[2].initialized(fixture.span("second", 1)));
        for pattern in [
            "[good, ...rest, last]".to_owned(),
            format!("{}good{}", "[".repeat(130), "]".repeat(130)),
            format!(
                "[{}]",
                (0..1025)
                    .map(|index| format!("v{index}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ] {
            let source = format!(
                "function f(input) {{ for (const {pattern} of input) {{ input; }} input; }}"
            );
            let fixture = BindingFixture::new(&source, &crate::typescript::TypeScript::default());
            assert!(
                fixture
                    .facts
                    .lexical
                    .iter()
                    .all(|binding| binding.symbol.kind != vvv_core::SymbolKind::Variable)
            );
            let after = fixture.source.rfind("input").unwrap();
            assert!(fixture.eligible(vvv_core::Span::new(after, after + 5)));
        }
    }

    #[test]
    fn var_iteration_owns_the_function_body_and_existing_assignments_add_no_bindings() {
        let fixture = BindingFixture::new(
            "function f(input) { for (var item of input) { item; } input; }",
            &crate::typescript::TypeScript::default(),
        );
        assert!(fixture.eligible(fixture.span("input", 2)));
        let fixture = BindingFixture::new(
            "function f(input) { for (input of entries) { input; } input; }",
            &crate::typescript::TypeScript::default(),
        );
        assert!(fixture.eligible(fixture.span("input", 2)));
        assert_eq!(
            fixture
                .facts
                .lexical
                .iter()
                .filter(|binding| binding.symbol.kind == vvv_core::SymbolKind::Parameter)
                .count(),
            1
        );
    }

    #[test]
    fn malformed_classic_headers_publish_no_bindings() {
        let source =
            "function f(input) { for (let good = input; input + ; good++) { good; } input; }";
        let fixture = BindingFixture::new(source, &crate::typescript::TypeScript::default());
        assert!(
            fixture
                .facts
                .lexical
                .iter()
                .all(|binding| binding.symbol.kind != vvv_core::SymbolKind::Variable)
        );
        assert!(!fixture.eligible(fixture.span("good", 2)));
        assert!(fixture.eligible(fixture.span("input", 3)));
    }

    #[test]
    fn custom_iteration_rules_retain_table_scope_and_capture_contracts() {
        const CUSTOM: vvv_core::BindingRule = vvv_core::BindingRule {
            node: "for_in_statement",
            name: None,
            scopes: &["statement_block"],
            kind: vvv_core::SymbolKind::Variable,
            namespace: vvv_core::BindingNamespace::Value,
            after: false,
        };
        for rules in [&[CUSTOM][..], &[CUSTOM, CUSTOM][..]] {
            let grammar = Grammar {
                bindings: rules,
                ..Grammar::EMPTY
            };
            let tables = crate::syntax::AstGrepSearcher::new(TypeScript, grammar);
            let typed = tables
                .clone()
                .with_navigation_syntax(crate::syntax::NavigationSyntax::TypeScript);
            let source = "function f(input) { for (let item of input) { item; } input; }";
            assert_eq!(
                format!("{:?}", typed.facts(source).unwrap()),
                format!("{:?}", tables.facts(source).unwrap())
            );
        }
        let fixture = BindingFixture::new(
            "function f(input) { for (var item = input in entries) { item; } input; }",
            &crate::typescript::TypeScript::default(),
        );
        assert!(!fixture.eligible(fixture.span("input", 2)));
    }

    #[test]
    fn local_pattern_limits_reject_the_complete_block() {
        for pattern in [
            format!("{}value{}", "[".repeat(130), "]".repeat(130)),
            format!(
                "[{}]",
                (0..1025)
                    .map(|index| format!("value{index}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ] {
            let source =
                format!("function f(input) {{ const good = input; let {pattern} = input; good; }}");
            let fixture = BindingFixture::new(&source, &crate::typescript::TypeScript::default());
            assert!(
                fixture
                    .facts
                    .lexical
                    .iter()
                    .all(|binding| binding.symbol.kind != vvv_core::SymbolKind::Variable)
            );
            assert!(!fixture.eligible(fixture.span("good", 1)));
        }
    }

    #[test]
    fn competing_local_rules_retain_table_interpretation() {
        const LOCAL: vvv_core::BindingRule = vvv_core::BindingRule {
            node: "lexical_declaration",
            name: None,
            scopes: &["statement_block"],
            kind: vvv_core::SymbolKind::Variable,
            namespace: vvv_core::BindingNamespace::Value,
            after: false,
        };
        let grammar = Grammar {
            bindings: &[LOCAL, LOCAL],
            ..Grammar::EMPTY
        };
        let tables = crate::syntax::AstGrepSearcher::new(TypeScript, grammar);
        let typed = tables
            .clone()
            .with_navigation_syntax(crate::syntax::NavigationSyntax::TypeScript);
        let source = "function f(input) { let local = input; local; }";
        assert_eq!(
            format!("{:?}", typed.facts(source).unwrap()),
            format!("{:?}", tables.facts(source).unwrap())
        );
    }

    #[test]
    fn pattern_traversal_limits_reject_the_complete_header() {
        for source in [
            format!(
                "function f(good, {}value{}) {{ good; }}",
                "[".repeat(130),
                "]".repeat(130)
            ),
            format!(
                "function f(good, [{}]) {{ good; }}",
                (0..1025)
                    .map(|index| format!("value{index}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ] {
            let fixture = BindingFixture::new(&source, &crate::typescript::TypeScript::default());
            assert!(
                !fixture
                    .facts
                    .lexical
                    .iter()
                    .any(|binding| binding.symbol.kind == vvv_core::SymbolKind::Parameter)
            );
            assert!(!fixture.eligible(fixture.span("good", 1)));
        }
    }

    #[test]
    fn custom_parameter_fields_and_competing_rules_retain_table_interpretation() {
        const PATTERN: vvv_core::BindingRule = vvv_core::BindingRule {
            node: "required_parameter",
            name: Some("pattern"),
            scopes: &["function_declaration"],
            kind: vvv_core::SymbolKind::Parameter,
            namespace: vvv_core::BindingNamespace::Value,
            after: false,
        };
        const VALUE: vvv_core::BindingRule = vvv_core::BindingRule {
            name: Some("value"),
            ..PATTERN
        };
        for rules in [&[VALUE][..], &[PATTERN, VALUE][..], &[VALUE, PATTERN][..]] {
            // Grammar captures deliberately differ from built-in parameter interpretation.
            let grammar = Grammar {
                bindings: rules,
                ..Grammar::EMPTY
            };
            let tables = crate::syntax::AstGrepSearcher::new(TypeScript, grammar);
            let typed = tables
                .clone()
                .with_navigation_syntax(crate::syntax::NavigationSyntax::TypeScript);
            let source = "function f(real = chosen) { real; chosen; }";
            assert_eq!(
                format!("{:?}", typed.facts(source).unwrap()),
                format!("{:?}", tables.facts(source).unwrap())
            );
        }
    }

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
                let actual = typed.facts(source).unwrap();
                let expected = tables.facts(source).unwrap();
                assert_eq!(actual.symbols, expected.symbols);
                assert_eq!(actual.imports, expected.imports);
                assert_eq!(actual.declaration_pieces, expected.declaration_pieces);
                assert_eq!(actual.calls, expected.calls);
                assert_eq!(actual.signatures, expected.signatures);
            }
        }
    }
    #[test]
    fn declaration_and_call_contracts_are_independent_of_scope_coverage() {
        let cases = BindingCases {
            sources: &[
                "function f<T>(required: T, optional?: T) { required; optional; } class C<U> { m(value: U) { value; } }",
                "class C { constructor(public value: number) { value; } m(value: number) { value; } }",
                "function f(value: number) { class Local {} interface I {} type Alias = number; enum E { A } value; }",
            ],
        };
        cases.verify(crate::typescript::TypeScript::default().searcher());
        cases.verify(crate::typescript::Tsx::default().searcher());
    }
}
