//! Terminal output for people: the human renderer.
//!
//! The pipeline is `Answer → Document → View → Presentation → Styled → text`.
//! Results go to `out` (stdout) so they can be piped; summaries, hints and
//! warnings go to `err` (stderr). Both are generic writers so tests can
//! render into buffers.

use std::io::{self, IsTerminal, Stderr, Stdout, Write};

use clap::ColorChoice;
use vvv_engine::protocol::Answer;

use super::Diagnose;
use crate::output::Reporter;
use crate::output::render::{Palette, Renderer, Styled};
use vvv_engine::report::{Detailed, Document, Options, View};

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

impl<O: Write, E: Write> Renderer for HumanReporter<O, E> {
    /// Render a document, stream for stream: one styled line each.
    fn render(&mut self, report: &Document) -> io::Result<()> {
        let presentation = Detailed.present(report, self.options(), usize::MAX);
        for row in &presentation.body {
            writeln!(self.out, "{}", Styled(&self.styles, &row.line))?;
        }
        for row in &presentation.notes {
            writeln!(self.err, "{}", Styled(&self.styles, &row.line))?;
        }
        Ok(())
    }
}

impl<O: Write, E: Write> Reporter for HumanReporter<O, E> {
    fn report(&mut self, answer: &Answer) -> anyhow::Result<()> {
        let options = self.options();
        let report = Document::of(answer, options);
        self.render(&report)?;
        Ok(())
    }

    fn error(&mut self, error: &anyhow::Error) {
        let _ = self.render(&Document::error(&error.failure()));
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

    #[test]
    fn search() {
        insta::assert_snapshot!(render(|r| r.report(&Answer::Search(fx::search())).unwrap()));
    }

    #[test]
    fn search_empty() {
        let empty = Search {
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
