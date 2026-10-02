use super::{navigation::UnsupportedSyntax, pattern::PatternNavigation};
use crate::syntax::navigation::{BindingSite, NavigationCoverage, NavigationFacts};
use crate::syntax::views::syntax_view;
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{BindingRule, Facts, Span};

pub(crate) struct Parameter<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl Parameter {
        kinds: ["parameter"],
        required: {
            pattern: "pattern"
        },
        optional: {}
    }
}

pub(crate) struct TypeParameter<'tree, L: LanguageExt> {
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

enum HeaderParameter<'tree, L: LanguageExt> {
    Value(Parameter<'tree, L>),
    Type(TypeParameter<'tree, L>),
}

impl<'tree, L: LanguageExt> HeaderParameter<'tree, L> {
    fn cast(node: Node<'tree, StrDoc<L>>) -> Option<Self> {
        match node.kind().as_ref() {
            "parameter" => Parameter::cast(node).map(Self::Value),
            "type_parameter" => TypeParameter::cast(node).map(Self::Type),
            _ => None,
        }
    }

    fn syntax(&self) -> &Node<'tree, StrDoc<L>> {
        match self {
            Self::Value(view) => view.syntax(),
            Self::Type(view) => view.syntax(),
        }
    }

    fn field(&self, field: &str) -> Option<Node<'tree, StrDoc<L>>> {
        let captured = match (self, field) {
            (Self::Value(view), "pattern") => Some(view.pattern()),
            (Self::Type(view), "name") => Some(view.name()),
            _ => None,
        };
        captured
            .and_then(Result::ok)
            .or_else(|| self.syntax().field(field))
    }
}

pub(crate) struct HeaderBindings<'tree, 'a, 'g, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
    excluded: Option<Vec<Span>>,
    shared: &'a NavigationFacts<'g>,
}

impl<'tree, 'a, 'g, L: LanguageExt> HeaderBindings<'tree, 'a, 'g, L> {
    pub fn new(node: Node<'tree, StrDoc<L>>, shared: &'a NavigationFacts<'g>) -> Self {
        Self {
            excluded: None,
            node,
            shared,
        }
    }

    pub fn span(&self) -> Span {
        self.node.range().into()
    }

    pub fn declarations(
        &mut self,
        container: Node<'tree, StrDoc<L>>,
        facts: &mut Facts,
        coverage: &mut NavigationCoverage,
    ) {
        let shared = self.shared;
        for declaration in crate::syntax::views::DirectChildren::new(container, &[]) {
            if let Some(rule) = shared.grammar.bindings.iter().find(|rule| {
                rule.node == declaration.kind() && rule.scopes.contains(&self.node.kind().as_ref())
            }) && matches!(declaration.kind().as_ref(), "parameter" | "type_parameter")
            {
                self.extract(&declaration, rule, facts, coverage);
            }
        }
    }

    pub fn extract(
        &mut self,
        declaration: &Node<'tree, StrDoc<L>>,
        rule: &BindingRule,
        facts: &mut Facts,
        coverage: &mut NavigationCoverage,
    ) {
        let names = UnsupportedSyntax::validate(declaration).and_then(|()| {
            let pattern = rule
                .name
                .and_then(|field| match HeaderParameter::cast(declaration.clone()) {
                    Some(parameter) => parameter.field(field),
                    None => declaration.field(field),
                })
                .ok_or_else(|| UnsupportedSyntax::at(declaration))?;
            PatternNavigation::new(pattern, declaration.clone(), self.shared.grammar).bindings()
        });
        match names {
            Ok(names) => names.emit(
                BindingSite {
                    declaration: declaration.range().into(),
                    scope: self.node.range().into(),
                    excluded: self
                        .excluded
                        .get_or_insert_with(|| self.shared.exclusions(&self.node, coverage))
                        .clone(),
                    visible_from: self.node.range().start,
                    kind: rule.kind,
                    namespace: rule.namespace,
                },
                facts,
            ),
            Err(_) => coverage.block(self.node.range().into()),
        }
    }
}

pub(crate) struct Function<'tree, L: ast_grep_core::tree_sitter::LanguageExt> {
    node: ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>,
}

syntax_view! { impl Function {
    kinds: ["function_item", "function_signature_item"],
    required: {name: "name", parameters: "parameters"},
    optional: {generics: "type_parameters", body: "body"},
} }

impl<'tree, L: ast_grep_core::tree_sitter::LanguageExt> crate::syntax::views::CallableView<'tree, L>
    for Function<'tree, L>
{
    fn syntax(&self) -> &ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>> {
        self.syntax()
    }

    fn anonymous(&self) -> bool {
        false
    }
}

pub(crate) struct Closure<'tree, L: ast_grep_core::tree_sitter::LanguageExt> {
    node: ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>,
}

syntax_view! { impl Closure {
    kinds: ["closure_expression"],
    required: {parameters: "parameters", body: "body"},
    optional: {generics: "type_parameters"},
} }
#[cfg(test)]
mod tests {
    use crate::rust::Rust;
    use vvv_core::{Language, SymbolKind};

    #[test]
    fn typed_and_untyped_parameters_have_one_binding_each() {
        let source = "fn f(outer: usize) { let closure = |left, right: usize| { outer; left; right; fn inner() { outer; left; } }; }";
        let facts = Rust::default().facts(source).unwrap();
        for name in ["left", "right"] {
            let bindings: Vec<_> = facts
                .lexical
                .iter()
                .filter(|binding| binding.symbol.name == name)
                .collect();
            assert_eq!(bindings.len(), 1);
            assert_eq!(bindings[0].symbol.kind, SymbolKind::Parameter);
            let nested = source.find("fn inner").unwrap();
            assert!(bindings[0].excluded.iter().any(|span| span.start == nested));
        }
    }
}

impl<'tree, L: ast_grep_core::tree_sitter::LanguageExt> crate::syntax::views::CallableView<'tree, L>
    for Closure<'tree, L>
{
    fn syntax(&self) -> &ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>> {
        self.syntax()
    }

    fn anonymous(&self) -> bool {
        true
    }
}

pub(crate) struct Impl<'tree, L: ast_grep_core::tree_sitter::LanguageExt> {
    node: ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>,
}

syntax_view! { impl Impl {
    kinds: ["impl_item"],
    required: {target: "type", body: "body"},
    optional: {generics: "type_parameters"},
} }

pub(crate) struct Trait<'tree, L: ast_grep_core::tree_sitter::LanguageExt> {
    node: ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>,
}

syntax_view! { impl Trait {
    kinds: ["trait_item"],
    required: {name: "name", body: "body"},
    optional: {generics: "type_parameters"},
} }

pub(crate) struct Struct<'tree, L: ast_grep_core::tree_sitter::LanguageExt> {
    node: ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>,
}

syntax_view! { impl Struct {
    kinds: ["struct_item"],
    required: {name: "name"},
    optional: {generics: "type_parameters", body: "body"},
} }

pub(crate) struct Enum<'tree, L: ast_grep_core::tree_sitter::LanguageExt> {
    node: ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>,
}

syntax_view! { impl Enum {
    kinds: ["enum_item"],
    required: {name: "name", body: "body"},
    optional: {generics: "type_parameters"},
} }

pub(crate) struct Type<'tree, L: ast_grep_core::tree_sitter::LanguageExt> {
    node: ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>,
}

syntax_view! { impl Type {
    kinds: ["type_item"],
    required: {name: "name", value: "type"},
    optional: {generics: "type_parameters"},
} }

/// Union fields have a required body, unlike unit and tuple structs.
pub(crate) struct Union<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! { impl Union {
    kinds: ["union_item"], required: {name: "name", body: "body"}, optional: {},
} }

pub(crate) struct NamedItem<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! { impl NamedItem {
    kinds: ["const_item", "static_item", "field_declaration", "macro_definition"],
    required: {name: "name"}, optional: {},
} }

pub(crate) struct EnumVariant<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl EnumVariant {
        kinds: ["enum_variant"],
        required: {
            name: "name"
        },
        optional: {
            body: "body"
        }
    }
}

pub(crate) enum Constructor<'tree, L: LanguageExt> {
    Struct(Struct<'tree, L>),
    Variant(EnumVariant<'tree, L>),
}

impl<'tree, L: LanguageExt> Constructor<'tree, L> {
    pub fn cast(node: Node<'tree, StrDoc<L>>) -> Option<Self> {
        match node.kind().as_ref() {
            "struct_item" => Struct::cast(node).map(Self::Struct),
            "enum_variant" => EnumVariant::cast(node).map(Self::Variant),
            _ => None,
        }
    }

    fn syntax(&self) -> &Node<'tree, StrDoc<L>> {
        match self {
            Self::Struct(view) => view.syntax(),
            Self::Variant(view) => view.syntax(),
        }
    }

    pub fn fact(&self) -> Option<vvv_core::PatternConstructor> {
        if self
            .syntax()
            .dfs()
            .any(|part| part.is_error() || part.is_missing())
        {
            return None;
        }
        let (name, body) = match self {
            Self::Struct(view) => (view.name().ok()?, view.body().ok()?),
            Self::Variant(view) => (view.name().ok()?, view.body().ok()?),
        };
        let shape = match body
            .as_ref()
            .map(|body| body.kind().into_owned())
            .as_deref()
        {
            None => vvv_core::ConstructorShape::Unit,
            Some("ordered_field_declaration_list") => vvv_core::ConstructorShape::Tuple,
            Some("field_declaration_list") => vvv_core::ConstructorShape::Record,
            _ => return None,
        };
        Some(vvv_core::PatternConstructor {
            name_span: name.range().into(),
            shape,
            owner: match self {
                Self::Struct(_) => None,
                Self::Variant(_) => self
                    .syntax()
                    .ancestors()
                    .find_map(Enum::cast)
                    .and_then(|owner| owner.name().ok().or_else(|| owner.syntax().field("name")))
                    .map(|name| name.range().into()),
            },
        })
    }
}

/// A declaration's concrete shape is retained while shared consumers inspect its header.
pub(crate) enum Declaration<'tree, L: LanguageExt> {
    Function(Function<'tree, L>),
    Impl(Impl<'tree, L>),
    Trait(Trait<'tree, L>),
    Struct(Struct<'tree, L>),
    Union(Union<'tree, L>),
    Enum(Enum<'tree, L>),
    Type(Type<'tree, L>),
    Module(super::module::Module<'tree, L>),
    Variant(EnumVariant<'tree, L>),
    Item(NamedItem<'tree, L>),
}

impl<'tree, L: LanguageExt> Declaration<'tree, L> {
    pub fn cast(node: Node<'tree, StrDoc<L>>) -> Option<Self> {
        Some(match node.kind().as_ref() {
            "function_item" | "function_signature_item" => Self::Function(Function::cast(node)?),
            "impl_item" => Self::Impl(Impl::cast(node)?),
            "trait_item" => Self::Trait(Trait::cast(node)?),
            "struct_item" => Self::Struct(Struct::cast(node)?),
            "union_item" => Self::Union(Union::cast(node)?),
            "enum_item" => Self::Enum(Enum::cast(node)?),
            "type_item" => Self::Type(Type::cast(node)?),
            "enum_variant" => Self::Variant(EnumVariant::cast(node)?),
            "mod_item" => Self::Module(super::module::Module::cast(node)?),
            _ => Self::Item(NamedItem::cast(node)?),
        })
    }

    pub fn field(
        &self,
        field: &str,
    ) -> Option<Result<Option<Node<'tree, StrDoc<L>>>, crate::syntax::views::SyntaxError>> {
        Some(match (self, field) {
            (Self::Function(view), "name") => view.name().map(Some),
            (Self::Impl(view), "type") => view.target().map(Some),
            (Self::Trait(view), "name") => view.name().map(Some),
            (Self::Struct(view), "name") => view.name().map(Some),
            (Self::Union(view), "name") => view.name().map(Some),
            (Self::Enum(view), "name") => view.name().map(Some),
            (Self::Type(view), "name") => view.name().map(Some),
            (Self::Module(view), "name") => view.name().map(Some),
            (Self::Item(view), "name") => view.name().map(Some),
            (Self::Variant(view), "name") => view.name().map(Some),
            (Self::Function(view), "body") => view.body(),
            (Self::Impl(view), "body") => view.body().map(Some),
            (Self::Trait(view), "body") => view.body().map(Some),
            (Self::Struct(view), "body") => view.body(),
            (Self::Union(view), "body") => view.body().map(Some),
            (Self::Enum(view), "body") => view.body().map(Some),
            (Self::Module(view), "body") => view.body(),
            (Self::Impl(view), "type_parameters") => view.generics(),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod constructor_tests {
    use super::*;
    use ast_grep_language::Rust;
    use vvv_core::ConstructorShape;

    #[test]
    fn constructors_preserve_unit_tuple_record_shapes_and_enum_ownership() {
        let source = "struct Unit; struct Tuple(u8); struct Record { x: u8 } enum Choice { A, B(u8), C { x: u8 } }";
        let tree = Rust.ast_grep(source);
        let facts: Vec<_> = tree
            .root()
            .dfs()
            .filter_map(Constructor::cast)
            .filter_map(|view| view.fact())
            .collect();
        assert_eq!(
            facts.iter().map(|fact| fact.shape).collect::<Vec<_>>(),
            [
                ConstructorShape::Unit,
                ConstructorShape::Tuple,
                ConstructorShape::Record,
                ConstructorShape::Unit,
                ConstructorShape::Tuple,
                ConstructorShape::Record
            ]
        );
        assert!(facts[..3].iter().all(|fact| fact.owner.is_none()));
        assert!(
            facts[3..].iter().all(|fact| &source
                [fact.owner.unwrap().start..fact.owner.unwrap().end]
                == "Choice")
        );
        let unfinished = Rust.ast_grep("struct Incomplete { x: }");
        let view = unfinished.root().dfs().find_map(Constructor::cast).unwrap();
        assert!(view.fact().is_none());
    }
}

#[cfg(test)]
mod parameter_view_tests {
    use super::*;
    use ast_grep_language::Rust;

    #[test]
    fn header_parameters_keep_value_patterns_separate_from_type_names() {
        let tree = Rust.ast_grep("fn f<T>((left, right): (T, T)) {}");
        let root = tree.root();
        let value = root.dfs().find_map(Parameter::cast).unwrap();
        assert_eq!(value.pattern().unwrap().text(), "(left, right)");
        let generic = root.dfs().find_map(TypeParameter::cast).unwrap();
        assert_eq!(generic.name().unwrap().text(), "T");
        let parameter = HeaderParameter::cast(value.syntax().clone()).unwrap();
        assert_eq!(parameter.field("type").unwrap().text(), "(T, T)");
        assert!(parameter.field("custom").is_none());
    }
}
