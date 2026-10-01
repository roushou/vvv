# Construct-owned navigation extraction

Navigation extraction uses language-specific views of parsed constructs under
`vvv-lang/src/syntax/`. The boundaries in [architecture.md](architecture.md) and
[AGENTS.md](../AGENTS.md) apply: ast-grep provides the parsed tree, language plugins
return plain facts, and the engine owns resolution and candidate selection.

Structural view generation and interpretation boundaries are specified in
[typed-syntax-design.md](typed-syntax-design.md).

## Ownership and placement

```text
crates/
  vvv-core/src/
    navigation.rs                  # Shared binding and uncertainty facts
    lang/mod.rs                    # Grammar and Language contracts
  vvv-lang/src/
    rust/
      mod.rs                       # Selects Rust navigation syntax
      grammar.rs                   # Declarative grammar/semantics data
    typescript/
      mod.rs                       # Selects TypeScript/TSX views
      grammar.rs
    syntax/
      language.rs                  # AstGrepLanguage composition
      searcher.rs                  # One parse and Facts composition
      navigation.rs                # Shared lowering, coverage, and selection
      views.rs                     # Borrowed fields, direct children, callable contract
      typescript.rs                # TypeScript/TSX function, block, call, member views
      rust/
        mod.rs
        navigation.rs              # RustNavigation and unsupported syntax evidence
        block.rs                   # Block structure and direct statements
        header.rs                  # Function/closure/item views and HeaderBindings
        pattern.rs                 # Complete pattern inspection
        let_declaration.rs         # LetDeclaration ownership and visibility
        conditional.rs             # Conditional operand and branch visibility
        condition.rs               # Ordered if/while/guard condition operands
        match_expression.rs        # Match structure and independent arm coverage
        match_arm.rs               # Arm patterns, guards, and visibility
        loop_expression.rs         # Infinite, while, and for variants
        macro_invocation.rs        # Expression position and binding uncertainty
        import.rs                  # Block import coverage
        module.rs                  # Inline navigation owners
        call.rs                    # Callee and member-access structure
  vvv-engine/src/graph/
    navigation.rs                  # Candidate resolution and pattern evidence
    navigation_scope.rs            # Visibility, imports, and uncertainty checks
```

`Block`, `Function`, `Pattern`, `LetDeclaration`, and `Conditional` represent
particular parsed constructs. Each owns the behavior that requires understanding
that construct, with its methods colocated. Their parser nodes live only for one
extraction; they are not a second retained AST and do not enter core or the wire.
An optional alternative represents both `if` and `if/else`.

Small grammar configuration remains data. Existing `BindingRule` describes a table
entry; no corresponding `LetRule` or `ConditionalRule` is needed. Language-specific
semantics are shared only when their interpretation is actually common, rather
than because node names resemble one another. Closures have their own parameter
owner and retain capture-aware exclusions. Functions, closures, and item views share
binding lowering through `HeaderBindings`, rather than treating a closure as a function.

## Adapter selection

`NavigationSyntax` lives in the syntax adapter, not the core plugin contract:

```rust
#[derive(Debug, Clone, Copy, Default)]
pub enum NavigationSyntax {
    #[default]
    Tables,
    #[cfg(feature = "rust")]
    Rust,
    #[cfg(feature = "typescript")]
    TypeScript,
}
```

`AstGrepSearcher::new` defaults to `Tables`. Rust explicitly selects its construct
adapter through `AstGrepLanguage::with_navigation_syntax(NavigationSyntax::Rust)`;
TypeScript and TSX select typed callable views while retaining table binding
extraction. Generic grammar-backed users retain the complete table path.
Selection does not inspect language IDs, suffixes, or guessed grammar nodes.
Language modules select the adapter as configuration and still contain no parser
traversal or I/O. No new `Language` implementation or extraction trait is required.

`RustNavigation` retains the shared grammar context and dispatches recognized
constructs during the existing AST walk. Each binding occurrence has one owner.
Handled let declarations and function parameter/generic occurrences do not also
run their table extraction. Rust headers lower their direct parameter and generic declarations once per
function, closure, impl, trait, struct, enum, or type-alias owner. Other declaration
forms retain the generic table path. `Import` owns Rust block coverage; the shared
import extractor still owns path, alias, group, and visibility facts. `Module`
provides inline owners to shared module lowering. Language-specific `Call` and member views classify callees; shared `Calls`
consumes the callable contract to establish declaration ownership. File-root import and mutation facts retain their contracts.

## Shared lowering and parse-local coverage

`BindingName` contains a name, byte span, and explicit-binding evidence.
`BindingSite` contains the declaration span, lexical owner, visibility start,
namespace, kind, and capture exclusions. It lowers a complete set of names to
`LexicalBinding`s, reusing existing symbol identities where available.

`NavigationCoverage` contains blocked regions, modeled construct spans, and cached
exclusions per owner. Both the lexical identifier pass and the later type/import
identifier pass consult this evidence. A modeled conditional bypasses only its own
ancestor barrier. Supported loops and match arms establish their own modeled
coverage; macro arguments and unsupported patterns retain barriers. Unsupported ordinary declarations retain the enclosing
scope barrier. Unsupported conditional patterns block that conditional, including
its alternative, because their potential names cannot escape it.

Owner exclusions are computed once per scope and reused across declarations.
Function/generic exclusions retain noncapturing item behavior, including impl and
trait generics visible to their direct methods but unavailable to nested items.
No parser node cache or coverage ledger is retained in `Facts`.

## Complete pattern inspection

`Pattern` retains its node, declaration, and grammar configuration. Its iterative
walk collects names privately and returns them only after inspecting the complete
supported pattern. An unsupported nested form discards the entire result; emitting
only the supported prefix could lose a shadowing declaration.

Tuple, slice, reference, mutable, and tuple-struct forms retain their existing
extraction rules. Tuple-struct constructor fields and their path/generic segments
are excluded from bound names. Typed closure parameters retain their separate
binding occurrence. Missing fields or parser recovery affecting the recognized
binding structure produce adapter-owned unsupported syntax evidence, not an empty
successful pattern or a new request error.

`StructPattern` inspects named fields without traversing the constructor or
explicit field labels as bindings. Shorthand fields publish their own token;
renamed fields inspect only their nested pattern. Rest fields (`..`) publish no
names. Reference and mutable markers remain per-name evidence. Renamed label spans
are blocked in parse-local coverage so neither identifier pass can resolve them as
bare outer locals. A malformed or unsupported nested field discards all names from
the complete pattern. The extraction follows the Rust Reference's
[struct patterns](https://doc.rust-lang.org/reference/patterns.html#struct-patterns).

Explicit-binding markers belong to individual names, not all siblings in a
composite pattern. An immutable identifier pattern may match a constant instead of
introducing a variable. Pattern extraction therefore returns possible declaration
evidence; it does not establish a constructor namespace, infer a type, or expand a
macro. The engine checks visible named imports and module constant/constructor
bindings before confirming such a name as a variable. Missing evidence remains
conservative when a competing import or constructor is identified.

## Visibility without a new fact schema

The existing binding contract represents let declarations and condition chains:

```rust
LexicalBinding {
    symbol,
    scope: owner_span,
    excluded: noncapturing_items,
    visible_from,
    namespace: BindingNamespace::Value,
    explicit,
}
```

`scope` owns precedence, `visible_from` establishes the binding's start, and
`excluded` removes regions that cannot capture it. The engine prefers the smallest
containing owner and then the latest start; equal-rank targets remain selectable
candidates. Namespace checks, import precedence, and macro uncertainty remain
engine behavior. Navigation declarations with shared owner spans use their exact
name tokens as match spans, keeping candidate IDs distinct and selection exact;
symbol spans and display containers retain their complete owners.

Ordinary and `let … else` declarations belong to the enclosing block. Their
bindings start after the complete declaration, including its failure branch. The
initializer and failure branch retain the previous binding.

A conditional uses a synthetic binding owner from the start of the `if` expression
through the end of its consequence, excluding its alternative. Each condition
binding starts after its own `let_condition` operand. This represents later
operands and the successful branch without a multi-region visibility type.

```rust
let value = fallback;
if let Some(value) = input
    && ready(value)
    && let Some(value) = derive(value)
{
    consume(value);
} else {
    consume_outer(value);
}
consume_outer(value);
```

The first condition binding reaches later operands. The second initializer still
sees the first binding; afterward, the second binding takes precedence. Neither
reaches the alternative or following statement. Consequence block locals/imports
have a smaller owner and retain their precedence. Closures can capture these
bindings; nested function items cannot.

The parser represents an `if` alternative as an `else_clause`, while a let-else
alternative is a block. An else-if is a separate conditional with independent
bindings. Only the direct ordered operands of a `let_chain` are interpreted;
arbitrary nested boolean descendants are not flattened into a chain.

These visibility rules follow the Rust Reference's
[condition chains](https://doc.rust-lang.org/reference/expressions/if-expr.html#expr.if.chains)
and [let statements](https://doc.rust-lang.org/reference/statements.html#statement.let).
Navigation does not prove type correctness, exhaustiveness, reachability, or else
branch divergence. A future construct that needs disjoint visibility regions must
justify a separate plugin contract rather than hide language rules in the engine.

## Match, loop, and closure owners

`Match` validates its scrutinee/body structure and lowers each `MatchArm`
independently. A supported arm owns its pattern bindings from the end of the
pattern through its guard and body. Unsupported patterns or guard bindings block
that arm without blocking sibling arms, the scrutinee, or surrounding code. Guards
reuse ordered condition lowering; nested function items remain excluded. All owners
share the validated pattern extraction contract described below.

`Loop` has infinite, while, and for variants. A for binding begins after its
iterator expression and ends with its loop. A while-let binding begins after its
own operand and reaches later operands and the body. Neither reaches the following
statement. Ordinary while/infinite loops model their body without inventing
bindings. Unsupported loop patterns block that loop without publishing a prefix.

`Closure` owns typed and untyped parameters exactly once. Outer locals remain
capturable; nested item declarations remain excluded. Capture mode and inferred
parameter types are not resolved. These scopes reuse the existing `LexicalBinding`
contract and engine precedence rules.

The visibility boundaries follow the Rust Reference's
[match expressions](https://doc.rust-lang.org/reference/expressions/match-expr.html),
[loop expressions](https://doc.rust-lang.org/reference/expressions/loop-expr.html), and
[closure expressions](https://doc.rust-lang.org/reference/expressions/closure-expr.html).

`MacroInvocation` retains position-based uncertainty without expanding macros or
whitelisting names. `Impl`, `Trait`, `Struct`, `Enum`, and `Type` own required
header fields and generic scopes. `Type` currently represents type-alias ownership;
reference tokens retain shared grammar extraction. No receiver or associated-type
inference is implied by these views.

## Conservative boundaries

Statement-position macros retain uncertainty about block-wide items and later
locals. A smaller conditional scope does not prove an immutable pattern against a
possible generated constant. A macro in a consequence can introduce competing
block items even if the condition binding was introduced earlier. Known explicit
inner bindings and rooted paths retain their existing evidence-based behavior.

Required-expression classification through wrappers such as `!matches!(…)`,
receiver-head navigation, generic/const-block patterns, and broader
constructor evidence remain unresolved coverage in [backlog.md](backlog.md).
They are separate from conditional ownership. No macro-name whitelist, LSP, or
compiler inference is used. Navigation-only declarations do not expand mutation
symbols, file-root import facts, rename scope, or move support.

## Verification contracts

Language tests compare complete table and construct facts for existing binding
forms, including order and mutation facts. They cover conditional scopes, ordered
shadowing, marker ownership, unsupported nested patterns, malformed syntax,
noncapturing items, and nested barriers. Compiler-backed examples establish branch
and chain visibility.

Engine tests use the shared Fake language with synthetic spans. They cover
initializers, success-only visibility, consequence precedence, noncapturing items,
macro uncertainty, module/imported constants, unknown constructors, and selection.
The conditional corpus exercises compact navigation, context, relationships, and
unavailable outcomes. Mutation preview/apply/undo properties remain independent of
navigation-only ownership.

MCP tests cover navigation, body context, callers, ambiguity, and explicit selection.
Dogfooding uses scoped structural search and its content-bound anchors. A running
MCP server does not load a replacement binary; use a reconnected native server or
a fresh scripted subprocess, and distinguish their coverage. Bounded pages and
relationship scans are not evidence of complete reference coverage.

Run the warning-denied checks in AGENTS.md and review corpus changes as behavior
changes. Binary-replacing feature builds must run separately from subprocess MCP
tests. Construct extraction adds no core fact fields, request fields, CLI flags,
TUI state, or MCP tools.

Pattern extraction retains tentative names, pattern-reference roles, and direct
alternative constraints as serializable plugin facts. Captures force their own
binding, while literals and range bounds introduce none. Rest patterns are limited
to one per tuple or slice (slice captures permitted), or the final struct field.
Malformed syntax publishes neither names nor pattern evidence. Nested alternatives
are bounded at 128 levels without distributing nested patterns into a cross product.

The engine classifies each alternative site before comparing its binding names
and explicit modes. Constants and unit constructors contribute no bindings;
written binding sites remain separately selectable. Range endpoints use constant-only
lookup. Constructor declarations provide unit/tuple/record shape and enum ownership;
variant paths reuse scoped module resolution and the enum's visibility. Imports,
aliases, globs, re-exports, and all consulted source versions retain the existing
resolver contracts. Explicit paths are canonicalized from syntax tokens, including
comments/whitespace between segments; absolute scoped paths retain their root and
remain unresolved where the layout cannot interpret it.

Missing evidence and unknown expansions never become an invented binding or a
same-named local fallback. Classification and alternative validation share the
navigation work budget, including empty alternatives. Older plugin facts default
to no pattern evidence and retain their previous navigation behavior. Types,
implicit match ergonomics, generic constructors, const blocks, and constructor type
aliases remain outside this syntactic contract. Mutation symbols and addresses are
unchanged.
