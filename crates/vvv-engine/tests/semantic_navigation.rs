mod common;
use common::Fake;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use vvv_engine::{
    ContentId, Engine, EngineError, Languages, MemoryVfs, NavigationCancellation,
    NavigationOutcome, NavigationProvider, NavigationQuery, Position, ProviderVersion, Selection,
    SemanticFailure, SemanticReply, SemanticRequest, SemanticTarget, SourceVersion, SymbolRef, Vfs,
    Workspace,
};

struct Fixture {
    engine: Engine,
    vfs: Arc<MemoryVfs>,
    target: SymbolRef,
}
impl Fixture {
    fn new() -> Self {
        let vfs = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/package", "ws")
                .with_file("/ws/use.p", "é méthode")
                .with_file("/ws/target.p", "def Target")
                .with_file("/ws/other.p", "def Target")
                .with_file("/ws/config.json", "{}"),
        );
        let engine = Engine::new(
            Workspace::new("/ws", vfs.clone()),
            Languages::new().with(Fake::default()),
        );
        let NavigationOutcome::Resolved { target, .. } =
            NavigationQuery::at("target.p", Position::new(0, 4))
                .execute(&engine)
                .unwrap()
                .outcome
        else {
            panic!()
        };
        Self {
            engine,
            vfs,
            target,
        }
    }
    fn provider(&self, behavior: Behavior) -> Provider {
        Provider {
            behavior,
            target: self.target.clone(),
            version: AtomicUsize::new(1),
            calls: AtomicUsize::new(0),
            vfs: self.vfs.clone(),
        }
    }
    fn navigate(
        &self,
        provider: &Provider,
        selection: Selection,
        cancel: &NavigationCancellation,
    ) -> Result<vvv_engine::NavigationReply, EngineError> {
        NavigationQuery::at("use.p", Position::new(0, 2))
            .select(selection)
            .execute_with(&self.engine, provider, cancel)
    }
}
#[derive(Clone, Copy)]
enum Behavior {
    Normal,
    Ambiguous,
    BadOrigin,
    BadTarget,
    StaleVersion,
    EditedOrigin,
    EditedDependency,
    External,
    Cancel,
    Incomplete,
    Unavailable,
}
struct Provider {
    behavior: Behavior,
    target: SymbolRef,
    version: AtomicUsize,
    calls: AtomicUsize,
    vfs: Arc<MemoryVfs>,
}
impl NavigationProvider for Provider {
    fn version(&self) -> ProviderVersion {
        ProviderVersion {
            provider: "test".into(),
            revision: self.version.load(Ordering::SeqCst).to_string(),
        }
    }
    fn navigate(
        &self,
        request: &SemanticRequest,
        cancel: &NavigationCancellation,
    ) -> Result<SemanticReply, SemanticFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        // The API uses byte spans, not protocol UTF-16 offsets or display columns.
        assert_eq!(request.origin.span.start, 3);
        assert_eq!(
            &request.source[request.origin.span.start..request.origin.span.end],
            "méthode"
        );
        assert_eq!(request.origin.content, ContentId::of(&request.source));
        let mut reply = SemanticReply {
            version: request.version.clone(),
            origin: request.origin.clone(),
            dependencies: vec![SourceVersion {
                path: "config.json".into(),
                content: ContentId::of("{}"),
            }],
            targets: vec![SemanticTarget::Workspace {
                symbol: self.target.clone(),
            }],
        };
        match self.behavior {
            Behavior::Normal => {}
            Behavior::Ambiguous => {
                let mut other = self.target.clone();
                other.declaration.path = "other.p".into();
                reply
                    .targets
                    .push(SemanticTarget::Workspace { symbol: other });
            }
            Behavior::BadOrigin => reply.origin.span.start += 1,
            Behavior::BadTarget => {
                let SemanticTarget::Workspace { symbol } = &mut reply.targets[0] else {
                    panic!()
                };
                symbol.name_span.start += 1;
            }
            Behavior::StaleVersion => {
                self.version.fetch_add(1, Ordering::SeqCst);
            }
            Behavior::EditedOrigin => {
                self.vfs.write(Path::new("/ws/use.p"), "é changed").unwrap();
            }
            Behavior::EditedDependency => {
                self.vfs.write(Path::new("/ws/config.json"), "{ }").unwrap();
            }
            Behavior::External => reply.targets.push(SemanticTarget::External {
                uri: "provider://library/Target".into(),
                content: ContentId::of("Target"),
                span: vvv_engine::Span::new(0, 6),
            }),
            Behavior::Cancel => cancel.cancel(),
            Behavior::Incomplete => return Err(SemanticFailure::Incomplete),
            Behavior::Unavailable => return Err(SemanticFailure::Unavailable),
        }
        Ok(reply)
    }
}

#[test]
fn semantic_targets_are_versioned_validated_and_separate_from_the_legacy_oracle() {
    let f = Fixture::new();
    let provider = f.provider(Behavior::Normal);
    let reply = f
        .navigate(
            &provider,
            Selection::All,
            &NavigationCancellation::default(),
        )
        .unwrap();
    let NavigationOutcome::Resolved {
        target,
        evidence,
        preview,
    } = &reply.outcome
    else {
        panic!()
    };
    assert_eq!(target, &f.target);
    assert_eq!(evidence.semantic.as_ref().unwrap(), &provider.version());
    assert_eq!(
        target.declaration.content,
        ContentId::of(&preview.source.text)
    );
    provider.version.fetch_add(1, Ordering::SeqCst);
    let newer = f
        .navigate(
            &provider,
            Selection::All,
            &NavigationCancellation::default(),
        )
        .unwrap();
    assert_ne!(reply.snapshot, newer.snapshot);
    // An already confirmed syntactic declaration never asks the provider.
    NavigationQuery::at("target.p", Position::new(0, 4))
        .execute_with(&f.engine, &provider, &NavigationCancellation::default())
        .unwrap();
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn all_semantic_candidates_are_kept_and_selection_is_explicit() {
    let f = Fixture::new();
    let provider = f.provider(Behavior::Ambiguous);
    let reply = f
        .navigate(
            &provider,
            Selection::All,
            &NavigationCancellation::default(),
        )
        .unwrap();
    let NavigationOutcome::Ambiguous { candidates } = reply.outcome else {
        panic!()
    };
    assert_eq!(candidates.len(), 2);
    assert!(candidates.iter().all(|c| c.evidence.semantic.is_some()));
    let chosen = &candidates[1];
    let reply = f
        .navigate(
            &provider,
            Selection::ids([chosen.declaration.id.clone()]),
            &NavigationCancellation::default(),
        )
        .unwrap();
    assert!(
        matches!(reply.outcome,NavigationOutcome::Resolved { target, evidence, .. } if target==chosen.target && evidence.semantic.is_some())
    );
}

#[test]
fn stale_sources_provider_versions_and_forged_ranges_are_rejected() {
    for behavior in [
        Behavior::BadOrigin,
        Behavior::BadTarget,
        Behavior::StaleVersion,
        Behavior::EditedOrigin,
        Behavior::EditedDependency,
    ] {
        let f = Fixture::new();
        let provider = f.provider(behavior);
        let error = f
            .navigate(
                &provider,
                Selection::All,
                &NavigationCancellation::default(),
            )
            .unwrap_err();
        match behavior {
            Behavior::BadOrigin | Behavior::BadTarget => {
                assert!(matches!(error, EngineError::InvalidSemantic))
            }
            Behavior::StaleVersion => assert!(matches!(error, EngineError::StaleSemantic)),
            _ => assert!(matches!(error, EngineError::StaleSource { .. })),
        }
    }
}

#[test]
fn cancellation_incomplete_and_external_results_cannot_confirm_a_partial_answer() {
    let f = Fixture::new();
    let cancel = NavigationCancellation::default();
    cancel.cancel();
    let provider = f.provider(Behavior::Normal);
    assert!(matches!(
        f.navigate(&provider, Selection::All, &cancel),
        Err(EngineError::NavigationCancelled)
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        f.navigate(
            &f.provider(Behavior::Cancel),
            Selection::All,
            &NavigationCancellation::default()
        ),
        Err(EngineError::NavigationCancelled)
    ));
    assert!(matches!(
        f.navigate(
            &f.provider(Behavior::Incomplete),
            Selection::All,
            &NavigationCancellation::default()
        ),
        Err(EngineError::NavigationLimit)
    ));
    let external = f
        .navigate(
            &f.provider(Behavior::External),
            Selection::All,
            &NavigationCancellation::default(),
        )
        .unwrap();
    assert!(matches!(
        external.outcome,
        NavigationOutcome::Unavailable {
            reason: vvv_engine::UnavailableReason::ExternalSourceUnavailable
        }
    ));
    let missing = f
        .navigate(
            &f.provider(Behavior::Unavailable),
            Selection::All,
            &NavigationCancellation::default(),
        )
        .unwrap();
    assert!(matches!(
        missing.outcome,
        NavigationOutcome::Unavailable { .. }
    ));
}
