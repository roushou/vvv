# Typed syntax views

vvv interprets ast-grep trees through language-specific typed views. The views
borrow the existing tree; they do not materialize a second AST or escape
`vvv-lang/src/syntax/`. Language interpretation lowers them into the serializable
`Facts` contract. Workspace resolution remains in the engine.

## Structure and interpretation

A generated syntax view owns one inexpensive node handle. A declarative shape describes its
accepted node kinds, named fields, and direct child collections. Macro expansion
provides kind-checked casts, node access, and field accessors. View structs remain
ordinary Rust declarations; only inherent accessors are generated, so structural
symbol tools can still locate their definitions. Required fields
return structured syntax errors when absent or malformed; optional fields distinguish
absence from malformed presence. Casting recognizes a construct without requiring
an entire, potentially unfinished declaration to be complete.

Structural APIs answer questions such as a function's parameters, its body, a
block's direct statements, or a member access's receiver. They contain no grammar
semantics tables, coverage cache, accumulated facts, or workspace state. Accessors
are lazy; unused fields and descendants incur no traversal or allocation.

Interpretation uses those APIs to establish scope, explicit binding evidence, and
coverage. Rust header binding extraction runs once for each owner rather than
reconstructing an owner for every parameter. The common header component retains
scope/exclusion data while lowering the owner's direct header declarations. Pattern
views classify structural forms; pattern interpretation retains its grammar,
declaration owner, and bounded alternative traversal separately.

Declarative shapes describe structure, not visibility algorithms. Ordered condition
bindings, macro uncertainty, constructor/constant classification, and alternative
binding-set validation remain explicit algorithms with their existing owners.
Unsupported syntax remains represented or rejected conservatively.

## Definitions and placement

```text
vvv-lang/src/syntax/
  views.rs            # Shape macro, fields/errors, lazy children, CallableView
  calls.rs            # Shared call-site and callable-owner interpretation
  bindings.rs         # PatternView, TablePattern, shared PatternNames policy
  declarations.rs     # Rule-aware names, fields, modifiers, generic shadowing
  typescript.rs       # TypeScript/TSX callables, declarations, bindings, imports/exports
  rust/
    header.rs         # Function, Closure, Impl, Trait, Struct, Enum, Type;
                      # Union, Declaration, Parameter, TypeParameter, HeaderBindings
    block.rs          # Block and direct statements
    conditional.rs    # If, Else, ElseBranch; Conditional interpretation
    condition.rs      # LetCondition, LetChain; ordered Condition bindings
    loop_expression.rs # Loop variants; LoopBindings interpretation
    match_expression.rs # Match, MatchBody; MatchNavigation
    match_arm.rs      # MatchArm, MatchPattern; ArmNavigation
    let_declaration.rs # LetDeclaration; LetBinding
    macro_invocation.rs # MacroInvocation and placement inspection
    pattern.rs        # Pattern/PatternForm, PatternNavigation, pattern facts
    call.rs           # Call and MemberAccess
    import.rs         # Import, UseAlias, UseGroup, UseList, UseGlob
    module.rs         # Module, ModuleBody, Visibility; ModuleOwner
```

A declaration describes structural fields; the generated methods inspect them on
request:

```rust
pub(crate) struct Function<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}
syntax_view! { impl Function {
    kinds: ["function_item", "function_signature_item"],
    required: {parameters: "parameters"},
    optional: {generics: "type_parameters", body: "body"},
} }

syntax_view! { impl Block {
    kinds: ["block"], required: {}, optional: {},
    children: {statements: ["line_comment", "block_comment"]},
} }
```

`Function::cast(node)` recognizes a function. `function.parameters()` returns the
required parameter-list handle or a `SyntaxError`; `function.generics()` returns
`Ok(None)` for an absent generic list. `block.statements()` walks direct named
children and excludes comments, preserving expression-statement wrappers. Field
kinds beyond construct casts remain language-specific interpretation checks.
Signature extraction consumes the function body accessor while preserving grammar
rule overrides and captured malformed-body boundaries. `HeaderBindings` consumes direct header children and lowers their patterns; the
structural function never retains that component or the resulting facts.

## Rust pattern structure

Pattern views and their interpretation are colocated in `syntax/rust/pattern.rs`.
`PatternForm` carries the concrete compound view, so dispatch does not reconstruct
it from a second kind check. Views retain node handles; collections remain lazy.

| View                 | Structural questions                                                |
| -------------------- | ------------------------------------------------------------------- |
| `StructPattern`      | Constructor and ordered fields, with rest as a distinct entry       |
| `FieldPattern`       | Label, explicit value or shorthand binding, written ref/mut markers |
| `TuplePattern`       | Direct elements and rest multiplicity                               |
| `TupleStructPattern` | Constructor separated from direct elements                          |
| `SlicePattern`       | Direct elements, including captured rest                            |
| `OrPattern`          | Direct alternatives, preserving nested alternatives and wildcards   |
| `CapturedPattern`    | Binding name and captured subpattern as separate handles            |
| `RestPattern`        | Valid sequence ownership; captured rest requires a slice            |
| `RefPattern`         | Inner pattern of a written `ref` pattern                            |

`SequenceElements` reports duplicate rest elements as structural errors.
`StructFields` reports unsupported fields and any field after rest. Both iterators
retain only a parent handle, position, and validation state. Captures check their
cardinality and binding-name shape without allocating a child vector.

`PatternNavigation` consumes the views, assigns constructor/reference roles,
combines written binding modes, and assembles bounded alternative constraints.
The grammar remains authoritative for supported container forms; constructor
classification and alternative binding-set compatibility remain engine questions.
No names are emitted until the complete pattern has been interpreted successfully.
Unknown nested syntax therefore cannot publish a supported prefix or inherit an
outer binding.

## Language boundaries

Rust views live in `syntax/rust/`; TypeScript and TSX views share
`syntax/typescript.rs`. Both use the same descriptor/accessor infrastructure. Small
views and their tests are colocated rather than each requiring a separate file.

Rust and TypeScript functions expose language-specific parameter and body APIs.
Rust trait declarations may omit bodies; TypeScript arrows may have a single
unparenthesized parameter and an expression body. These distinctions remain
observable rather than being flattened into one universal function record.

The shared callable question is narrower: whether a construct establishes an
anonymous boundary, and which existing declaration owns a call. Rust functions,
Rust closures, and TypeScript callable forms implement that contract. Call
interpretation consumes it to preserve closure boundaries. Member-access views
centralize the distinct receiver/member field names for Rust and TypeScript;
recognizing a receiver does not establish its type or enable method resolution.

## Rust control-flow structure

Control-flow views use the same checked-field descriptors as declaration views.
`If` retains its condition, consequence, and optional alternative; `ElseBranch`
distinguishes another `If` from a `Block`. The `Loop` enum retains separate infinite,
while, and for views, each with its own required fields. `LetCondition` separates
its pattern and initializer, and `LetChain` iterates direct operands lazily without
flattening boolean expressions or allocating an operand vector.

`Match` separates its scrutinee and body. `MatchBody` lazily yields direct typed
`MatchArm` handles without flattening nested matches. `MatchPattern` distinguishes
the binding pattern from an optional guard and retains unnamed wildcard syntax.
`LetDeclaration` distinguishes an absent initializer from an initialized declaration
and an optional let-else block. `MacroInvocation` answers parent-placement and
field-occupancy questions; it does not parse or expand its token arguments.

Interpretation components retain the views and already checked header handles.
They own binding order, visibility boundaries, exclusions, all-or-nothing pattern
publication, and coverage decisions. Structural casts recognize unfinished constructs;
interpretation validates the same owner, body, condition, and arm boundaries before
publishing facts. An unsupported arm still blocks only that arm, and failed let
patterns retain the enclosing block's conservative coverage.

`Constructor` retains a `Struct` or `EnumVariant` view and supplies unit, tuple,
or record evidence with the variant's written enum owner. Complete subtree checks
and unsupported body shapes retain their existing conservative outcomes.
Explicit field labels are identified by `FieldPattern` separately from their
binding values, including preserved unfinished captures for coverage exclusion.

## TypeScript binding structure

TypeScript and TSX share `Parameter`, `TypeParameter`, `VariableDeclaration`, and
`VariableDeclarator` views. Parameter fields retain the parser's distinction between
patterns and names, including optional and constructor parameters. Declarator names
are consumed by symbol extraction through the same view used by the binding adapter.
Variable declaration owners remain distinct from their individual declarators.

`ObjectPattern`, `ArrayPattern`, `PairPattern`, `AssignmentPattern`, and `RestPattern`
retain direct syntax handles. Pair keys remain separate from binding values, defaults
retain both sides, and named-child iterators retain nested forms without manufacturing
bindings for array holes. Ordinary and object assignment patterns share their field
questions while retaining their written node kinds.

`TypeScriptNavigation` owns binding rule dispatch and the existing callable
completeness check. It selects matching rules once, uses typed fields for recognized
binding declarations, and delegates unknown declaration forms to generic table
extraction. Shared scope, exclusion, marker, and visibility-site interpretation retain
the grammar's existing contracts. Custom fields and unfinished captures retain raw
field fallback.

Structural recognition does not establish lexical support. Simple identifier and
type-parameter bindings retain their existing scope rules. Destructuring, local
variable declarations, anonymous/signature callable boundaries, and hoisting/TDZ
coverage retain their current conservative outcomes. Grammar pattern-container and
constructor rules remain authoritative; a pattern's recognized shape alone cannot
publish binding names or bypass an unsupported owner.

## Import and module structure

Rust import views distinguish an alias's path from its local name, grouped prefixes
from direct list entries, and a wildcard from its optional prefix. Nested groups
retain their original source spans and ordered outer prefixes. Group ownership is
computed from parent handles without recursively re-extracting prefixes.
`Module` distinguishes file-backed declarations from inline bodies; `ModuleBody`
iterates direct declarations. `Visibility` supplies the written restriction to both
import and module interpretation.

TypeScript and TSX retain separate import and export views. `SourceStatement`
exposes their shared source question while preserving local exports with no source.
Named specifiers retain imported names, aliases, and type-only markers; clauses
retain default and namespace names. No universal import trait flattens these forms.

Grammar rules still determine captured paths and supported bindings. Typed accessors
supply structure to existing lowering, with table fallback for other grammars and
raw captured fields for unfinished syntax. This preserves malformed-tree spans
without treating them as complete syntax. Nested module ownership and scoped imports
remain navigation facts; file-root import bindings and mutation addresses retain
their existing scope. Function-local modules and unplaced files gain no new support
from these views.

## Declaration consumers

Rust and TypeScript declaration enums retain concrete header views. Rust includes
function, impl, trait, struct, union, enum, type, module, and named-item shapes.
TypeScript includes callable, class, interface, enum, and named-declaration shapes.
Struct bodies remain optional, including tuple bodies; union, enum, trait, and
container bodies retain their required structural fields. Signature rules decide
which body kind ends an excerpt, so a tuple field list still belongs to its signature.

The shared `Declaration` component selects a structural shape once per instance
and bridges grammar-named fields to its checked accessors. Symbol extraction,
signatures, and companion-target inspection use this component. Unknown grammar
fields and unfinished captures retain raw-node fallback. A successful absent optional
field stays absent; it is not replaced with a guessed header or body.

Symbol rule ordering, scope wrappers, leading sibling runs, and export extent
composition remain explicit interpretation algorithms. Impl target names still
follow the rule's inner type fields, and generic shadowing retains syntactic
name evidence. Same-scope ownership, competing declarations, qualified targets,
and cross-scope targets keep their existing conservative verdicts. Neither the
new views nor their shared consumer expand symbol movement or signature coverage.

## Shared binding policy and consumer boundaries

`PatternNames` owns the grammar policy for simple name extraction: identifier leaves,
ignored markers, independently lowered parameter declarations, allowed containers,
and constructor-field exclusion. `PatternView` supplies structural access through two
concrete implementations: the generic `TablePattern` and TypeScript's typed pattern
forms. Generic table and TypeScript binding extraction consume the same policy;
an unsupported descendant rejects the complete name set. Rust's constructor roles,
written modes, and alternative binding constraints retain their richer language-specific
interpretation rather than being reduced to this simple policy.

Rust `Parameter` and `TypeParameter` views supply checked header captures. The header
component retains grammar-selected scopes and field overrides, including raw captured
fields when syntax is unfinished. Signature extraction directly consumes declaration
fields; it does not maintain another body-selection layer.

Raw tree access remains at concrete structural accessors, generic rule/table fallback,
source-order leading-trivia traversal, and language-owned interpretation algorithms.
These boundaries preserve extensible grammar fields and unfinished-tree spans.
Workspace resolution, ambiguity, source availability, mutation planning, and protocol
data remain independent of the parser views. Structural descriptors do not establish
new binding coverage, macro expansion, hoisting rules, or type inference.

## Data and cost

Views retain node handles and static descriptors only. Child iterators retain a
parent handle and traversal position, with no child vectors or copied source.
Only lowering into existing facts allocates persistent navigation data. The engine
continues to operate on facts and fake languages; syntax views introduce no new
parser dependency across crate boundaries.

There is no runtime schema interpreter, build-time generator, new parser, or
normalized owned tree. Declarative definitions expand during Rust compilation.
Context-sensitive logic remains reviewable Rust alongside its construct.

## Verification

`facts_bench` can capture complete debug representations of facts for every Rust,
TypeScript, and TSX corpus source. Before/after captures check the plugin contract
in addition to the command corpus. Existing corpus outputs must remain unchanged
for this structural refactor; feature-specific tests exercise malformed fields,
direct child boundaries, anonymous callables, and language-specific function forms.

```console
cargo build --release -p vvv-lang --all-features --example facts_bench
./target/release/examples/facts_bench /tmp/vvv-facts
./target/release/examples/facts_bench
```

The timing mode measures complete parse-and-facts extraction after warmup, reporting
nine samples for fixed Rust repository inputs and TypeScript corpus/synthetic inputs.
Compare identical sources and compiler profiles. These measurements assess the
whole adapter, not a claim that each accessor is free. Native and freshly rebuilt
MCP runs additionally check structural search, navigation, context, and relationship
coverage.
