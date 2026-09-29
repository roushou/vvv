//! Terminal output for people: the human renderer.
//!
//! The pipeline is `Answer → Document → View → Presentation → Styled → text`.
//! Results go to `out` (stdout) so they can be piped; summaries, hints and
//! warnings go to `err` (stderr). Both are generic writers so tests can
//! render into buffers.

mod advice;

use std::io::{self, IsTerminal, Stderr, Stdout, Write};

use clap::ColorChoice;
use vvv_engine::protocol::Answer;

use super::Diagnose;
use crate::output::Reporter;
use crate::output::render::{Palette, Renderer, Styled};
use vvv_engine::report::{Block, Detailed, Document, Note, Options, Presentation, View};

pub struct HumanReporter<O: Write = Stdout, E: Write = Stderr> {
    out: O,
    err: E,
    styles: Palette,
    /// Expand what is collapsed by default (`-v`).
    verbose: bool,
    /// Print every file's full patch, not only structural edits (`--diff`).
    diff: bool,
}

impl HumanReporter {
    /// Bound to the process's stdout/stderr, coloured per `--color` and
    /// whether each stream is a terminal.
    pub fn stdio(color: ColorChoice) -> Self {
        let stdout = io::stdout();
        let stderr = io::stderr();
        let styles = Palette::for_stream(color, stdout.is_terminal());
        // stderr follows stdout's decision unless it is not a terminal itself.
        let styles = if color == ColorChoice::Auto && !stderr.is_terminal() {
            Palette::plain()
        } else {
            styles
        };
        Self::new(stdout, stderr, styles)
    }
}

impl<O: Write, E: Write> HumanReporter<O, E> {
    pub fn new(out: O, err: E, styles: Palette) -> Self {
        Self {
            out,
            err,
            styles,
            verbose: false,
            diff: false,
        }
    }

    pub fn verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    pub fn diff(mut self, diff: bool) -> Self {
        self.diff = diff;
        self
    }

    /// The writers, for tests that render into buffers.
    #[cfg(test)]
    pub fn into_parts(self) -> (O, E) {
        (self.out, self.err)
    }

    fn options(&self) -> Options {
        Options {
            verbose: self.verbose,
            diff: self.diff,
        }
    }
}

impl<O: Write, E: Write> HumanReporter<O, E> {
    fn render_presentation(&mut self, presentation: &Presentation) -> io::Result<()> {
        for row in &presentation.body {
            writeln!(self.out, "{}", Styled(&self.styles, &row.line))?;
        }
        for row in &presentation.notes {
            writeln!(self.err, "{}", Styled(&self.styles, &row.line))?;
        }
        Ok(())
    }
}

impl<O: Write, E: Write> Renderer for HumanReporter<O, E> {
    /// Render a document, stream for stream: one styled line each.
    fn render(&mut self, report: &Document) -> io::Result<()> {
        let presentation = Detailed.present(report, self.options(), usize::MAX);
        self.render_presentation(&presentation)
    }
}

impl<O: Write, E: Write> Reporter for HumanReporter<O, E> {
    fn report(&mut self, answer: &Answer) -> anyhow::Result<()> {
        let report = Document::of(answer);
        let presentation =
            advice::TerminalView { answer }.present(&report, self.options(), usize::MAX);
        self.render_presentation(&presentation)?;
        Ok(())
    }

    fn error(&mut self, error: &anyhow::Error) {
        let failure = error.failure();
        let mut report = Document::error(&failure);
        for line in failure.hint.iter().flat_map(|hint| hint.lines()) {
            report.block_note(Block::Note(Note::Hint(line.to_owned())));
        }
        let _ = self.render(&report);
    }
}

#[cfg(test)]
mod tests {
    //! Exact human output, plain palette, rendered into buffers.

    use super::*;
    use crate::output::fixtures as fx;
    use vvv_engine::protocol::{Answer, History, Search};

    fn render(f: impl FnOnce(&mut HumanReporter<Vec<u8>, Vec<u8>>)) -> String {
        let mut r = HumanReporter::new(Vec::new(), Vec::new(), Palette::plain());
        f(&mut r);
        let (out, err) = r.into_parts();
        format!(
            "{}--- stderr ---\n{}",
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap()
        )
    }

    #[cfg(feature = "schemas")]
    #[test]
    fn schema_document() {
        let schema = vvv_engine::SchemaQuery {
            for_command: Some(vvv_engine::Command::History),
            contract: vvv_engine::SchemaContract::Arguments,
        }
        .execute()
        .unwrap();
        insta::assert_snapshot!(render(|r| r.report(&Answer::Schema(schema)).unwrap()));
    }

    #[test]
    fn navigation_outcomes() {
        use vvv_engine::{
            ContentId, DefinitionPreview, NavigationOutcome, NavigationReply, ResolutionEvidence,
            SourceAnchor, SymbolKind, SymbolRef, UnavailableReason,
        };
        let text = "pub struct Engine;";
        let mut declaration = fx::decl("src/state.rs", 0, SymbolKind::Struct, "Engine", text);
        declaration.symbol.as_mut().unwrap().span = vvv_engine::Span::new(0, text.len());
        declaration.symbol.as_mut().unwrap().extent = vvv_engine::Span::new(0, text.len());
        let symbol = declaration.symbol.clone().unwrap();
        let target = SymbolRef {
            language: "rust".into(),
            declaration: SourceAnchor {
                path: declaration.path.clone(),
                content: ContentId::of(text),
                span: symbol.extent,
            },
            name_span: symbol.name_span,
            kind: symbol.kind,
        };
        let resolved = NavigationOutcome::Resolved {
            target: target.clone(),
            evidence: ResolutionEvidence {
                semantic: None,
                addresses: vec![],
            },
            preview: Box::new(DefinitionPreview {
                container: target,
                declaration,
                source: vvv_engine::File {
                    identifiers: vec![],
                    path: "src/state.rs".into(),
                    text: text.into(),
                    highlights: vec![],
                    symbols: vec![symbol.clone()],
                },
                selection: symbol.name_span,
                identifiers: vec![],
            }),
        };
        insta::assert_snapshot!(render(|reporter| {
            for outcome in std::iter::once(resolved).chain(
                [
                    UnavailableReason::NoIdentifier,
                    UnavailableReason::Unresolved,
                    UnavailableReason::UnsupportedContext,
                    UnavailableReason::ExternalSourceUnavailable,
                    UnavailableReason::CyclicImports,
                ]
                .into_iter()
                .map(|reason| NavigationOutcome::Unavailable { reason }),
            ) {
                reporter
                    .report(&Answer::Navigate(NavigationReply {
                        snapshot: ContentId::of(text).into(),
                        outcome,
                    }))
                    .unwrap();
            }
        }));
    }

    #[test]
    fn bounded_context_excerpts() {
        use vvv_engine::{
            ContentId, ContextItem, ContextOmissions, ContextOutcome, ContextRelation,
            ContextReply, Position, SourceAnchor, Span, SymbolKind, SymbolRef,
        };
        let text = "fn engine() { work(); }";
        let anchor = SourceAnchor {
            path: "src/example.rs".into(),
            content: ContentId::of(text),
            span: Span::new(0, text.len()),
        };
        let target = SymbolRef {
            language: "rust".into(),
            declaration: anchor.clone(),
            name_span: Span::new(3, 9),
            kind: SymbolKind::Function,
        };
        let reply = ContextReply {
            enclosing: None,
            snapshot: ContentId::of(text).into(),
            outcome: ContextOutcome::Resolved,
            items: vec![ContextItem {
                signature: None,
                target,
                relation: ContextRelation::Definition,
                via: None,
                excerpt: SourceAnchor {
                    span: Span::new(0, 11),
                    ..anchor
                },
                start: Position::new(10, 0),
                text: text[..11].into(),
                complete: false,
            }],
            omissions: ContextOmissions {
                byte_limit: 1,
                unavailable: 2,
                ..ContextOmissions::default()
            },
            references_by_name: false,
        };
        insta::assert_snapshot!(render(|reporter| reporter
            .report(&Answer::Context(reply))
            .unwrap()));
    }

    #[test]
    fn applied_plan_validation() {
        let report = serde_json::from_value::<vvv_engine::ValidationReport>(serde_json::json!({
            "plan_id":"p1.example", "history_id":1,"run":1,"sources":[],
            "before":"source-version", "after":"source-version","input_files":2,"extra_inputs":[],"source_state":"unchanged","passed":false,
            "checks":[{"command":{"name":"compile","program":"cargo","args":["check"]},"outcome":"failed","exit_code":101,"duration_ms":12,"stdout":{"text":"","bytes_seen":0,"truncated":false,"complete":true},"stderr":{"text":"error: expected Engine, found Runtime\n","bytes_seen":38,"truncated":false,"complete":true},"failure":null}]
        })).unwrap();
        insta::assert_snapshot!(
            "validation_failure",
            render(|r| r.report(&Answer::ValidatePlan(report.clone())).unwrap())
        );
        let review = vvv_engine::PlanReview {
            plan_id: report.plan_id.clone(),
            lifetime_seconds: 600,
            status: vvv_engine::PlanStatus::Applied {
                receipt: vvv_engine::PlanReceipt {
                    plan_id: report.plan_id.clone(),
                    history_id: 1,
                    files: vec![],
                },
            },
            validation: Some(report),
        };
        insta::assert_snapshot!(
            "inspect_validation",
            render(|r| r.report(&Answer::InspectPlan(review)).unwrap())
        );
    }

    #[test]
    fn search() {
        insta::assert_snapshot!(render(|r| r.report(&Answer::Search(fx::search())).unwrap()));
    }

    #[test]
    fn search_empty() {
        let empty = Search {
            scope: Default::default(),
            query: vvv_engine::Query::pattern("nope"),
            matches: vec![],
            skipped: vec![],
        };
        insta::assert_snapshot!(render(|r| r.report(&Answer::Search(empty)).unwrap()));
    }

    #[test]
    fn search_with_skipped_language() {
        insta::assert_snapshot!(render(|r| {
            r.report(&Answer::Search(fx::search_with_skipped()))
                .unwrap();
        }));
    }

    #[test]
    fn outline_rows() {
        insta::assert_snapshot!(render(|r| r
            .report(&Answer::Outline(fx::outline()))
            .unwrap()));
    }

    #[test]
    fn references_are_marked() {
        insta::assert_snapshot!(render(|r| {
            r.report(&Answer::References(fx::references())).unwrap();
        }));
    }

    #[test]
    fn where_with_and_without_from() {
        insta::assert_snapshot!(render(|r| {
            r.report(&Answer::Where(fx::locations(true))).unwrap();
            r.report(&Answer::Where(fx::locations(false))).unwrap();
        }));
    }

    #[test]
    fn deps_both_ways() {
        insta::assert_snapshot!(render(|r| r.report(&Answer::Deps(fx::deps())).unwrap()));
    }

    #[test]
    fn explain_a_position() {
        insta::assert_snapshot!(render(|r| {
            r.report(&Answer::Explain(fx::explanation())).unwrap();
        }));
    }

    #[test]
    fn explain_an_import() {
        insta::assert_snapshot!(render(|r| r
            .report(&Answer::Explain(fx::explanation_of_an_import()))
            .unwrap()));
    }

    #[test]
    fn surface_of_a_package() {
        insta::assert_snapshot!(render(|r| r
            .report(&Answer::Surface(fx::surface()))
            .unwrap()));
    }

    #[test]
    fn impact_by_depth() {
        insta::assert_snapshot!(render(|r| r.report(&Answer::Impact(fx::impact())).unwrap()));
    }

    #[test]
    fn dead_with_unsure_counts() {
        insta::assert_snapshot!(render(|r| r.report(&Answer::Dead(fx::dead())).unwrap()));
    }

    #[test]
    fn imports_worth_a_look() {
        insta::assert_snapshot!(render(|r| r
            .report(&Answer::Imports(fx::imports()))
            .unwrap()));
    }

    #[test]
    fn rewrite_preview() {
        insta::assert_snapshot!(render(|r| {
            r.report(&Answer::Rewrite(fx::rewrite(false))).unwrap();
        }));
    }

    #[test]
    fn rewrite_applied() {
        insta::assert_snapshot!(render(|r| {
            r.report(&Answer::Rewrite(fx::rewrite(true))).unwrap();
        }));
    }

    #[test]
    fn rename_preview() {
        insta::assert_snapshot!(render(|r| r
            .report(&Answer::Rename(fx::rename(1)))
            .unwrap()));
    }

    #[test]
    fn rename_diff_prints_the_patch() {
        let mut r = HumanReporter::new(Vec::new(), Vec::new(), Palette::plain()).diff(true);
        r.report(&Answer::Rename(fx::rename(1))).unwrap();
        let (out, err) = r.into_parts();
        insta::assert_snapshot!(format!(
            "{}--- stderr ---\n{}",
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap()
        ));
    }

    #[test]
    fn rename_ambiguous() {
        insta::assert_snapshot!(render(|r| r
            .report(&Answer::Rename(fx::rename(2)))
            .unwrap()));
    }

    #[test]
    fn rename_verbose_lists_every_row() {
        let mut r = HumanReporter::new(Vec::new(), Vec::new(), Palette::plain()).verbose(true);
        r.report(&Answer::Rename(fx::rename(1))).unwrap();
        let (out, err) = r.into_parts();
        insta::assert_snapshot!(format!(
            "{}--- stderr ---\n{}",
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap()
        ));
    }

    #[test]
    fn move_symbol_preview() {
        insta::assert_snapshot!(render(|r| {
            r.report(&Answer::MoveSymbol(fx::move_symbol())).unwrap();
        }));
    }

    #[test]
    fn move_preview_with_notice() {
        insta::assert_snapshot!(render(|r| {
            r.report(&Answer::Move(fx::move_file())).unwrap();
        }));
    }

    #[test]
    fn undo() {
        insta::assert_snapshot!(render(|r| r.report(&Answer::Undo(fx::undo())).unwrap()));
    }

    #[test]
    fn history_lists_oldest_first() {
        // `Ago` depends on the clock; pin the entries to "now" so the column is stable.
        let now = vvv_engine::protocol::vocabulary::Ago::now();
        let mut h = fx::history();
        for e in &mut h.entries {
            e.at = now;
        }
        insta::assert_snapshot!(render(|r| r.report(&Answer::History(h)).unwrap()));
    }

    #[test]
    fn history_empty() {
        let empty = History { entries: vec![] };
        insta::assert_snapshot!(render(|r| r.report(&Answer::History(empty)).unwrap()));
    }

    #[test]
    fn errors_carry_hints() {
        let query = anyhow::Error::from(vvv_engine::QueryError::Empty);
        let symbol = anyhow::Error::from(vvv_engine::EngineError::NoSuchSymbol {
            name: "Nope".into(),
            kind: None,
        });
        insta::assert_snapshot!(render(|r| {
            r.error(&query);
            r.error(&symbol);
        }));
    }

    #[test]
    fn ambiguous_symbol_error_keeps_cli_flag_advice() {
        let error = anyhow::Error::from(vvv_engine::EngineError::AmbiguousSymbol {
            name: "Config".to_owned(),
            declarations: vec![],
        });
        let output = render(|reporter| reporter.error(&error));
        assert!(output.contains("pick one with --in <file>"));
    }

    #[test]
    fn colored_output_wraps_hits_and_paths() {
        let mut r = HumanReporter::new(Vec::new(), Vec::new(), Palette::colored());
        r.report(&Answer::Search(fx::search())).unwrap();
        let (out, _) = r.into_parts();
        let out = String::from_utf8(out).unwrap();
        assert!(
            out.contains("\x1b[1m\x1b[35msrc/lib.rs\x1b[0m"),
            "path styled"
        );
        assert!(out.contains("\x1b[1m\x1b[31mLanguage\x1b[0m"), "hit styled");
    }
}
