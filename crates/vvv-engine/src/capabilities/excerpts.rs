//! Exact, UTF-8 aligned continuation of a declaration's captured extent.
use super::pagination::PageSession;
use crate::query_store::Checkpoint;
use crate::{Cursor, Engine, EngineError, Position, SnapshotId, SourceAnchor, Span, SymbolRef};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ExpandQuery {
    pub cursor: Cursor,
    #[cfg_attr(feature = "schema", schemars(range(min = 1024, max = 1048576)))]
    pub max_bytes: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Expansion {
    pub snapshot: SnapshotId,
    pub target: SymbolRef,
    /// Full extent being expanded; the excerpt is the returned subrange.
    pub requested: SourceAnchor,
    pub excerpt: SourceAnchor,
    pub start: Position,
    pub text: String,
    pub done: bool,
    pub next_cursor: Option<Cursor>,
}
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Excerpt {
    pub target: SymbolRef,
    pub next: usize,
}
impl ExpandQuery {
    pub fn execute(self, engine: &Engine) -> Result<Expansion, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine)
    }
    pub(crate) fn execute_in(self, engine: &Engine) -> Result<Expansion, EngineError> {
        let budget = crate::PageBudget {
            max_bytes: self.max_bytes,
            max_items: 1,
        };
        budget.validate()?;
        let (mut session, state, graph) = PageSession::resume(engine, &self.cursor, true)?;
        let Checkpoint::Excerpt(excerpt) = state else {
            return Err(EngineError::InvalidCursor);
        };
        let result = excerpt.expand(engine, &graph, &mut session, budget);
        session.finish(engine, result)
    }
}
impl Excerpt {
    fn expand(
        self,
        engine: &Engine,
        graph: &crate::graph::Graph,
        session: &mut PageSession,
        budget: crate::PageBudget,
    ) -> Result<Expansion, EngineError> {
        let file = graph.file(&self.target.declaration.path)?;
        let text = &file.text()[self.next..self.target.declaration.span.end];
        let minimum = text.chars().next().map_or(0, char::len_utf8);
        let mut length = text.len().min(budget.max_bytes);
        while !text.is_char_boundary(length) {
            length -= 1;
        }
        loop {
            let after = Self {
                target: self.target.clone(),
                next: self.next + length,
            };
            let done = after.next == self.target.declaration.span.end;
            let next = Checkpoint::Excerpt(after);
            let reply = Expansion {
                snapshot: session.root.identity.clone(),
                target: self.target.clone(),
                requested: self.target.declaration.clone(),
                excerpt: SourceAnchor {
                    span: Span::new(self.next, self.next + length),
                    ..self.target.declaration.clone()
                },
                start: file.file().source().position(self.next),
                text: text[..length].to_owned(),
                done,
                next_cursor: (!done).then(|| session.token(engine, &next)),
            };
            match budget.check(&reply) {
                Ok(()) => {
                    if !done {
                        session.retain(next);
                    }
                    return Ok(reply);
                }
                Err(EngineError::OutputLimit {
                    max_bytes,
                    required_bytes,
                }) => {
                    if length == minimum {
                        return Err(EngineError::PageOutputLimit {
                            max_bytes,
                            required_bytes,
                            anchor: Some(self.target.declaration.clone()),
                        });
                    }
                    length = length
                        .saturating_sub(required_bytes - max_bytes)
                        .max(minimum);
                    while !text.is_char_boundary(length) {
                        length -= 1;
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }
}
impl crate::report::Document {
    pub(crate) fn expansion(reply: &Expansion) -> Self {
        use crate::protocol::display::{Line, Role};
        let mut doc = Self::new();
        doc.body([Line::of(Role::Path, reply.excerpt.path.to_string())]);
        doc.body(reply.text.lines().map(|line| Line::of(Role::Plain, line)));
        doc
    }
}
