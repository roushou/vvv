//! The CLI view: detailed rows with flag advice added at presentation time.

use vvv_engine::protocol::{Answer, Confidence};
use vvv_engine::report::{Block, Detailed, Document, Note, Options, Presentation, Row, View};

pub(super) struct TerminalView<'a> {
    pub(super) answer: &'a Answer,
}

impl View for TerminalView<'_> {
    fn rows(&self, block: &Block, options: Options, width: usize) -> Vec<Row> {
        Detailed.rows(block, options, width)
    }

    fn present(&self, report: &Document, options: Options, width: usize) -> Presentation {
        let mut presentation = Detailed.present(report, options, width);
        let move_hint = || {
            let mut flags = vec!["--apply to write"];
            if !options.diff {
                flags.push("--diff for the full patch");
            }
            flags.join(" · ")
        };
        let hint = match self.answer {
        Answer::Search(result) if !result.skipped.is_empty() => Some(
            "a pattern is written in one language; pass --lang to search that one only (the matches above are complete for the others)"
                .to_owned(),
        ),
        Answer::Where(result)
            if result.sites.iter().all(|site| site.import.is_none())
                && result.sites.iter().any(|site| site.address.is_some()) =>
        {
            Some("--from <file> for the import to write there".to_owned())
        }
        Answer::Rewrite(result)
            if !result.state.is_applied() && result.files.iter().any(|file| !file.edits.is_empty()) =>
        {
            Some("--apply to write".to_owned())
        }
        Answer::Rename(result) if !result.state.is_applied() => {
            let numbers: Vec<usize> = result
                .occurrences
                .iter()
                .enumerate()
                .filter(|(_, occurrence)| occurrence.confidence == Confidence::Unresolved)
                .map(|(index, _)| index + 1)
                .collect();
            let unsure = match (numbers.first(), numbers.last()) {
                (Some(first), Some(last))
                    if last - first + 1 == numbers.len() && first != last =>
                {
                    format!("{first}-{last}")
                }
                _ => numbers
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            };
            let mut flags = vec!["--apply to write".to_owned()];
            if !unsure.is_empty() {
                flags.push(format!("--select {unsure} for the ? rows alone"));
            }
            if !options.verbose
                && result
                    .occurrences
                    .iter()
                    .any(|occurrence| occurrence.confidence == Confidence::Resolved)
            {
                flags.push("-v to list the ✓ rows".to_owned());
            }
            Some(flags.join(" · "))
        }
        Answer::Move(result) if !result.state.is_applied() => Some(move_hint()),
        Answer::MoveSymbol(result) if !result.state.is_applied() => Some(move_hint()),
        Answer::Batch(result) if !result.state.is_applied() => Some(move_hint()),
        Answer::History(result) if result.entries.is_empty() => Some(
            "--apply writes a plan and records it here; `vvv undo` reverses the newest".to_owned(),
        ),
        _ => None,
    };
        if let Some(hint) = hint {
            presentation.notes.extend(Detailed.rows(
                &Block::Note(Note::Hint(hint)),
                options,
                width,
            ));
        }
        presentation
    }

    fn occurrence(
        &self,
        occurrence: &vvv_engine::Occurrence,
        ordinal: usize,
        ticked: bool,
        width: usize,
    ) -> Row {
        Detailed.occurrence(occurrence, ordinal, ticked, width)
    }

    fn respelling(&self, respelling: &vvv_engine::Respelling, width: usize) -> Row {
        Detailed.respelling(respelling, width)
    }

    fn notice(&self, notice: &vvv_engine::Notice, width: usize) -> Row {
        Detailed.notice(notice, width)
    }

    fn rewrite(
        &self,
        matched: &vvv_engine::Match,
        ordinal: usize,
        ticked: bool,
        width: usize,
    ) -> Row {
        Detailed.rewrite(matched, ordinal, ticked, width)
    }
}
