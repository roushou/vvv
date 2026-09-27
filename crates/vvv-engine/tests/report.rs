//! Reports retain source facts independently of presentation choices.
mod common;

use common::FaultFixture;
use vvv_engine::report::{Block, Detailed, Document, Options, Presentation, Source, View};
use vvv_engine::{
    Answer, Apply, DeadQuery, DepsQuery, ExplainQuery, ImportsQuery, MoveIntent, OutlineQuery,
    Position, ReferencesQuery, RenameIntent, SurfaceQuery, WhereQuery,
};

#[test]
fn declaration_blocks_and_rows_keep_their_owning_file_and_line() {
    let fixture = FaultFixture::new(&[("a.p", "pub def foo\ndef lone\nfoo")]);
    let source_sites = |presentation: Presentation| {
        presentation
            .body
            .into_iter()
            .filter_map(|row| row.source)
            .collect::<Vec<_>>()
    };
    let site = |line| Source {
        path: "a.p".into(),
        line,
    };
    let references = ReferencesQuery::new("foo")
        .execute(&fixture.engine)
        .unwrap();
    let document = Document::of(&Answer::References(references));
    assert!(
        matches!(&document.parts().0[0], Block::Declarations(declarations) if declarations[0].path == std::path::Path::new("a.p"))
    );
    assert_eq!(
        source_sites(Detailed.present(&document, Options::default(), usize::MAX)),
        [site(0)]
    );

    let where_ = WhereQuery {
        name: "foo".into(),
        from: Some("a.p".into()),
    }
    .execute(&fixture.engine)
    .unwrap();
    assert_eq!(
        source_sites(Detailed.present(
            &Document::of(&Answer::Where(where_)),
            Options::default(),
            usize::MAX
        )),
        [site(0)]
    );
    let outline = OutlineQuery { path: "a.p".into() }
        .execute(&fixture.engine)
        .unwrap();
    let document = Document::of(&Answer::Outline(outline));
    assert!(document.parts().0.iter().any(
        |block| matches!(block, Block::Outline { path, .. } if path == std::path::Path::new("a.p"))
    ));
    assert_eq!(
        source_sites(Detailed.present(&document, Options::default(), usize::MAX)),
        [site(0), site(1)]
    );

    let dead = DeadQuery::default().execute(&fixture.engine).unwrap();
    assert_eq!(
        source_sites(Detailed.present(
            &Document::of(&Answer::Dead(dead)),
            Options::default(),
            usize::MAX
        )),
        [site(1)]
    );
    let surface = SurfaceQuery::default().execute(&fixture.engine).unwrap();
    assert_eq!(
        source_sites(Detailed.present(
            &Document::of(&Answer::Surface(surface)),
            Options::default(),
            usize::MAX
        )),
        [site(0)]
    );
}

#[test]
fn import_and_explanation_rows_preserve_source_coordinates() {
    let fixture = FaultFixture::new(&[
        ("a.p", "pub def foo"),
        ("b.p", "use a.p/foo\nuse a.p/foo\nuse ext/missing\nfoo"),
    ]);
    let source_sites = |presentation: Presentation| {
        presentation
            .body
            .into_iter()
            .filter_map(|row| row.source)
            .collect::<Vec<_>>()
    };
    let site = |line| Source {
        path: "b.p".into(),
        line,
    };
    let deps = DepsQuery { path: "b.p".into() }
        .execute(&fixture.engine)
        .unwrap();
    let document = Document::of(&Answer::Deps(deps));
    assert!(document.parts().0.iter().any(|block| matches!(block, Block::DepGroups { path, .. } if path == std::path::Path::new("b.p"))));
    assert_eq!(
        source_sites(Detailed.present(&document, Options::default(), usize::MAX)),
        [site(0), site(1), site(2)]
    );
    let incoming = DepsQuery { path: "a.p".into() }
        .execute(&fixture.engine)
        .unwrap();
    assert_eq!(
        source_sites(Detailed.present(
            &Document::of(&Answer::Deps(incoming)),
            Options::default(),
            usize::MAX
        )),
        [site(0), site(1)]
    );
    let imports = ImportsQuery::default().execute(&fixture.engine).unwrap();
    assert_eq!(
        source_sites(Detailed.present(
            &Document::of(&Answer::Imports(imports)),
            Options::default(),
            usize::MAX
        )),
        [site(2), site(1)]
    );
    let explanation = ExplainQuery {
        path: "b.p".into(),
        position: Position::new(0, 5),
    }
    .execute(&fixture.engine)
    .unwrap();
    assert_eq!(
        source_sites(Detailed.present(
            &Document::of(&Answer::Explain(explanation)),
            Options::default(),
            usize::MAX
        )),
        [site(0), site(0)]
    );
}

#[test]
fn one_document_supports_collapsed_expanded_and_patch_views() {
    let fixture = FaultFixture::new(&[("a.p", "pub def foo\nfoo")]);
    let answer = Answer::Rename(
        RenameIntent::new("foo", "bar")
            .plan(&fixture.engine)
            .unwrap()
            .into_inner(),
    );
    let document = Document::of(&answer);
    assert!(document.parts().0.iter().any(
        |block| matches!(block, Block::Verdicts { plan: Some(plan), .. } if !plan.files.is_empty())
    ));
    let text = |presentation: &Presentation| {
        presentation
            .body
            .iter()
            .flat_map(|row| row.line.pieces().iter())
            .map(|piece| piece.text.as_str())
            .collect::<String>()
    };
    let regular = Detailed.present(&document, Options::default(), usize::MAX);
    let expanded = Detailed.present(
        &document,
        Options {
            verbose: true,
            diff: false,
        },
        usize::MAX,
    );
    let patched = Detailed.present(
        &document,
        Options {
            verbose: false,
            diff: true,
        },
        usize::MAX,
    );
    assert!(!text(&regular).contains("@@"));
    assert!(!text(&expanded).contains("@@"));
    assert!(text(&patched).contains("@@"));
    assert_eq!(
        regular
            .body
            .iter()
            .filter(|row| row.source.is_some())
            .count(),
        1
    );
    assert_eq!(
        expanded
            .body
            .iter()
            .filter(|row| row.source.is_some())
            .count(),
        3
    );
    assert_eq!(
        text(&Detailed.present(&document, Options::default(), usize::MAX)),
        text(&regular)
    );

    let outline = OutlineQuery { path: "a.p".into() }
        .execute(&fixture.engine)
        .unwrap();
    let document = Document::of(&Answer::Outline(outline));
    assert!(
        !text(&Detailed.present(&document, Options::default(), usize::MAX)).contains("everyone")
    );
    assert!(
        text(&Detailed.present(
            &document,
            Options {
                verbose: true,
                diff: false
            },
            usize::MAX
        ))
        .contains("everyone")
    );
}

#[test]
fn move_rows_retain_respelling_and_notice_sites() {
    let fixture = FaultFixture::new(&[
        ("manifest.p", ""),
        ("a/x.p", "use b/y.p\n"),
        ("b/y.p", "use a/x.p\nuse {a/x.p}\n"),
    ]);
    let result = MoveIntent::new("a/x.p", "d/x.p")
        .plan(&fixture.engine)
        .unwrap();
    let presentation = Detailed.present(
        &Document::of(&Answer::Move(result.into_inner())),
        Options::default(),
        usize::MAX,
    );
    let sites: Vec<_> = presentation
        .body
        .into_iter()
        .filter_map(|row| row.source)
        .collect();
    assert_eq!(
        sites,
        [
            Source {
                path: "b/y.p".into(),
                line: 0
            },
            Source {
                path: "b/y.p".into(),
                line: 1
            }
        ]
    );
}

#[test]
fn diff_rows_reference_only_the_available_source_side() {
    let fixture = FaultFixture::new(&[("a.p", "one\ntwo\n")]);
    let plan = vvv_engine::RewriteIntent::new(vvv_engine::Query::pattern("one"), "new\nextra")
        .plan(&fixture.engine)
        .unwrap();
    let preview = Document::of(&Answer::Rewrite((*plan).clone()));
    let rows = Detailed
        .present(&preview, Options::default(), usize::MAX)
        .body;
    let text = |row: &vvv_engine::report::Row| {
        row.line
            .pieces()
            .iter()
            .map(|piece| piece.text.as_str())
            .collect::<String>()
    };
    assert!(rows.iter().any(|row| text(row) == "-one"
        && row.source.as_ref()
            == Some(&Source {
                path: "a.p".into(),
                line: 0
            })));
    assert!(
        rows.iter()
            .any(|row| text(row) == "+new" && row.source.is_none())
    );
    assert!(rows.iter().any(|row| text(row) == " two"
        && row.source.as_ref()
            == Some(&Source {
                path: "a.p".into(),
                line: 1
            })));
    let applied = Apply(plan).apply(&fixture.engine).unwrap();
    let rows = Detailed
        .present(
            &Document::of(&Answer::Rewrite(applied.into_inner())),
            Options::default(),
            usize::MAX,
        )
        .body;
    assert!(
        rows.iter()
            .any(|row| text(row) == "-one" && row.source.is_none())
    );
    assert!(rows.iter().any(|row| text(row) == "+new"
        && row.source.as_ref()
            == Some(&Source {
                path: "a.p".into(),
                line: 0
            })));
    assert!(rows.iter().any(|row| text(row) == "+extra"
        && row.source.as_ref()
            == Some(&Source {
                path: "a.p".into(),
                line: 1
            })));
    assert!(rows.iter().any(|row| text(row) == " two"
        && row.source.as_ref()
            == Some(&Source {
                path: "a.p".into(),
                line: 2
            })));
}

#[test]
fn moved_diff_rows_keep_the_path_of_the_available_source_side() {
    use vvv_engine::{FileChange, MutationState};

    let files = vec![FileChange {
        path: "old.p".into(),
        moved_to: Some("new.p".into()),
        edits: Vec::new(),
        diff: vvv_engine::protocol::Diff::between(
            std::path::Path::new("old.p"),
            std::path::Path::new("new.p"),
            "before\n",
            "after\n",
        ),
    }];
    for (state, expected) in [
        (
            MutationState::Preview,
            Source {
                path: "old.p".into(),
                line: 0,
            },
        ),
        (
            MutationState::Applied { history_id: 1 },
            Source {
                path: "new.p".into(),
                line: 0,
            },
        ),
    ] {
        let mut document = Document::new();
        document.block_body(Block::Changes {
            state,
            files: files.clone(),
        });
        let sites: Vec<_> = Detailed
            .present(&document, Options::default(), usize::MAX)
            .body
            .into_iter()
            .filter_map(|row| row.source)
            .collect();
        assert_eq!(sites, [expected]);
    }
}
