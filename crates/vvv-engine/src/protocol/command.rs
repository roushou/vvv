//! Stable command identity and descriptors shared by discovery and clients.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Command {
    DiscardPlan,
    ApplyPlan,
    ValidatePlan,
    InspectPlan,
    PrepareRename,
    PrepareRewrite,
    PrepareMove,
    PrepareMoveSymbol,
    SymbolMoveCandidates,
    ReviewPlan,
    #[cfg(feature = "schema")]
    Schema,
    SearchPage,
    ContextPage,
    Continue,
    Expand,
    Discover,
    Context,
    Navigate,
    Relationships,
    Resolve,
    Search,
    Outline,
    References,
    Where,
    Deps,
    Explain,
    Surface,
    Impact,
    Dead,
    Imports,
    WorkspaceFiles,
    File,
    Rewrite,
    Rename,
    Move,
    MoveSymbol,
    Batch,
    History,
    Undo,
}

impl Command {
    pub const ALL: &[Self] = &[
        Self::DiscardPlan,
        Self::ApplyPlan,
        Self::ValidatePlan,
        Self::InspectPlan,
        Self::PrepareRename,
        Self::PrepareRewrite,
        Self::PrepareMove,
        Self::PrepareMoveSymbol,
        Self::SymbolMoveCandidates,
        Self::ReviewPlan,
        #[cfg(feature = "schema")]
        Self::Schema,
        Self::SearchPage,
        Self::ContextPage,
        Self::Continue,
        Self::Expand,
        Self::Discover,
        Self::Context,
        Self::Navigate,
        Self::Relationships,
        Self::Resolve,
        Self::Search,
        Self::Outline,
        Self::References,
        Self::Where,
        Self::Deps,
        Self::Explain,
        Self::Surface,
        Self::Impact,
        Self::Dead,
        Self::Imports,
        Self::WorkspaceFiles,
        Self::File,
        Self::Rewrite,
        Self::Rename,
        Self::Move,
        Self::MoveSymbol,
        Self::Batch,
        Self::History,
        Self::Undo,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::DiscardPlan => "discard_plan",
            Self::ApplyPlan => "apply_plan",
            Self::ValidatePlan => "validate_plan",
            Self::InspectPlan => "inspect_plan",
            Self::PrepareRename => "prepare_rename",
            Self::PrepareRewrite => "prepare_rewrite",
            Self::PrepareMove => "prepare_move",
            Self::PrepareMoveSymbol => "prepare_move_symbol",
            Self::SymbolMoveCandidates => "symbol_move_candidates",
            Self::ReviewPlan => "review_plan",
            #[cfg(feature = "schema")]
            Self::Schema => "schema",
            Self::SearchPage => "search_page",
            Self::ContextPage => "context_page",
            Self::Continue => "continue",
            Self::Expand => "expand",
            Self::Discover => "discover",
            Self::Context => "context",
            Self::Navigate => "navigate",
            Self::Relationships => "relationships",
            Self::Resolve => "resolve",
            Self::Search => "search",
            Self::Outline => "outline",
            Self::References => "references",
            Self::Where => "where",
            Self::Deps => "deps",
            Self::Explain => "explain",
            Self::Surface => "surface",
            Self::Impact => "impact",
            Self::Dead => "dead",
            Self::Imports => "imports",
            Self::WorkspaceFiles => "workspace_files",
            Self::File => "file",
            Self::Rewrite => "rewrite",
            Self::Rename => "rename",
            Self::Move => "move",
            Self::MoveSymbol => "move_symbol",
            Self::Batch => "batch",
            Self::History => "history",
            Self::Undo => "undo",
        }
    }

    pub fn parameters(self) -> &'static [&'static str] {
        match self {
            Self::DiscardPlan => &["plan_id"],
            Self::ApplyPlan => &["plan_id"],
            Self::ValidatePlan => &["plan_id", "checks", "extra_inputs", "budget"],
            Self::InspectPlan => &["plan_id", "max_bytes", "page"],
            Self::PrepareRename
            | Self::PrepareRewrite
            | Self::PrepareMove
            | Self::PrepareMoveSymbol => &["intent", "max_bytes", "page"],
            Self::ReviewPlan => &["cursor", "page"],
            Self::SymbolMoveCandidates => &["name", "from"],
            #[cfg(feature = "schema")]
            Self::Schema => &["for_command", "contract"],
            Self::SearchPage => &["query", "scope", "page"],
            Self::ContextPage => &[
                "origin",
                "selection",
                "references",
                "include_enclosing",
                "detail",
                "page",
                "work",
            ],
            Self::Continue => &["cursor", "page", "work"],
            Self::Expand => &["cursor", "max_bytes"],
            Self::Discover => &[],
            Self::Context => &[
                "origin",
                "selection",
                "budget",
                "references",
                "include_enclosing",
                "detail",
            ],
            Self::Relationships => &["origin", "selection", "kind", "scope", "budget"],
            Self::Navigate | Self::Resolve => &["origin", "selection"],
            Self::Search => &["pattern", "kind", "symbol", "name", "language", "scope"],
            Self::Outline => &["path"],
            Self::References => &["name", "symbol", "language", "declared_in"],
            Self::Where => &["name", "from"],
            Self::Deps => &["path"],
            Self::Explain => &["path", "position"],
            Self::Surface => &["package"],
            Self::Impact => &["name", "declared_in"],
            Self::Dead => &["language"],
            Self::Imports => &["path"],
            Self::WorkspaceFiles => &[],
            Self::File => &["path"],
            Self::Rewrite => &["query", "template", "selection", "apply"],
            Self::Rename => &[
                "name",
                "to",
                "symbol",
                "language",
                "declared_in",
                "selection",
                "apply",
            ],
            Self::Move => &["from", "to", "apply"],
            Self::MoveSymbol => &[
                "name",
                "from",
                "to",
                "selection",
                "expected_content",
                "apply",
            ],
            Self::Batch => &["intents", "apply"],
            Self::History => &[],
            Self::Undo => &[],
        }
    }

    pub fn is_read_only(self) -> bool {
        match self {
            Self::DiscardPlan => true,
            Self::ApplyPlan | Self::ValidatePlan => false,
            Self::InspectPlan => true,
            Self::PrepareRename
            | Self::PrepareRewrite
            | Self::PrepareMove
            | Self::PrepareMoveSymbol
            | Self::SymbolMoveCandidates
            | Self::ReviewPlan => true,
            #[cfg(feature = "schema")]
            Self::Schema => true,
            Self::SearchPage => true,
            Self::ContextPage => true,
            Self::Continue => true,
            Self::Expand => true,
            Self::Discover => true,
            Self::Context => true,
            Self::Relationships => true,
            Self::Navigate | Self::Resolve => true,
            Self::Search => true,
            Self::Outline => true,
            Self::References => true,
            Self::Where => true,
            Self::Deps => true,
            Self::Explain => true,
            Self::Surface => true,
            Self::Impact => true,
            Self::Dead => true,
            Self::Imports => true,
            Self::WorkspaceFiles => true,
            Self::File => true,
            Self::Rewrite => false,
            Self::Rename => false,
            Self::Move => false,
            Self::MoveSymbol => false,
            Self::Batch => false,
            Self::History => true,
            Self::Undo => false,
        }
    }
}

impl std::str::FromStr for Command {
    type Err = serde_json::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        serde_json::from_value(serde_json::Value::String(value.into()))
    }
}
