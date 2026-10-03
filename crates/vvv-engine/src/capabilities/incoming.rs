//! Per-file discovery of written names that can refer to an exact target.
use crate::graph::Graph;
use crate::{
    Candidate, EngineError, NavigationOutcome, NavigationQuery, SourceAnchor, SourceVersion,
    SymbolRef,
};
use serde::Serialize;
use std::collections::BTreeSet;
use vvv_core::Span;

/// Plain checkpoint data; no source documents or parser handles are retained.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct IncomingReferences {
    names: BTreeSet<String>,
    imports: Vec<(String, Span)>,
    import: usize,
    pub sites: Option<Vec<Span>>,
    pub site: usize,
}

pub(crate) struct DiscoveryWork {
    pub lookups: usize,
    pub max_lookups: usize,
    pub unresolved_imports: usize,
}

impl IncomingReferences {
    pub(crate) fn new(file: &Candidate, name: &str, calls_only: bool) -> Result<Self, EngineError> {
        let facts = file.facts()?;
        let mut imports: BTreeSet<(String, Span)> = facts
            .named_imports
            .iter()
            .filter(|import| import.imported != "*")
            .map(|import| (import.local.clone(), import.name_span))
            .collect();
        if !facts.named_modules {
            for import in facts
                .imports
                .iter()
                .filter(|import| import.declares && !import.glob)
            {
                if let Some(local) = import.binding()
                    && let Some((_, _, token)) = facts
                        .tokens()
                        .filter(|(_, _, token)| import.span.contains(token))
                        .max_by_key(|(_, _, token)| token.end)
                {
                    // The captured path span can exclude an alias token. Probe that path,
                    // but enumerate the written binding supplied by the plugin.
                    imports.insert((local.as_str().to_owned(), token));
                }
            }
        }

        if calls_only {
            imports.retain(|(local, _)| {
                facts.calls.iter().any(|call| {
                    file.text().get(call.callee.start..call.callee.end) == Some(local.as_str())
                })
            });
        }
        Ok(Self {
            names: BTreeSet::from([name.to_owned()]),
            imports: imports.into_iter().collect(),
            import: 0,
            sites: None,
            site: 0,
        })
    }

    /// Stops before an uncharged probe, preserving discovery progress for retry.
    pub(crate) fn prepare(
        &mut self,
        graph: &mut Graph,
        file: &Candidate,
        target: &SymbolRef,
        work: &mut DiscoveryWork,
        observed: &mut Vec<SourceVersion>,
    ) -> Result<bool, EngineError> {
        while let Some((local, span)) = self.imports.get(self.import) {
            graph.check_read()?;
            if self.names.contains(local) {
                self.import += 1;
                continue;
            }
            if work.lookups == work.max_lookups {
                return Ok(false);
            }
            work.lookups += 1;
            let anchor = SourceAnchor {
                path: file.path().into(),
                content: file.file().content_id(),
                span: *span,
            };
            let reply = match graph.navigate_observed(NavigationQuery::occurrence(anchor), observed)
            {
                Ok(reply) => reply,
                Err(EngineError::NavigationLimit) => {
                    work.unresolved_imports += 1;
                    self.import += 1;
                    continue;
                }
                Err(error) => return Err(error),
            };
            match reply.outcome {
                NavigationOutcome::Resolved { target: found, .. } if &found == target => {
                    self.names.insert(local.clone());
                }
                NavigationOutcome::Ambiguous { candidates }
                    if candidates
                        .iter()
                        .any(|candidate| &candidate.target == target) =>
                {
                    self.names.insert(local.clone());
                }
                NavigationOutcome::Unavailable { .. } => work.unresolved_imports += 1,
                _ => {}
            }
            self.import += 1;
        }
        Ok(true)
    }

    pub(crate) fn names(&self) -> &BTreeSet<String> {
        &self.names
    }
}
