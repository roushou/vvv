//! CLI flag advice added after the shared report has composed an answer.

use vvv_engine::protocol::{Answer, Confidence};
use vvv_engine::report::{Block, Document, Note, Options};

pub(super) struct Advice<'a> {
    pub(super) answer: &'a Answer,
    pub(super) options: Options,
}

impl Advice<'_> {
    pub(super) fn append_to(&self, report: &mut Document) {
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
            if !result.applied && result.files.iter().any(|file| !file.edits.is_empty()) =>
        {
            Some("--apply to write".to_owned())
        }
        Answer::Rename(result) if !result.applied => {
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
            if !self.options.verbose
                && result
                    .occurrences
                    .iter()
                    .any(|occurrence| occurrence.confidence == Confidence::Resolved)
            {
                flags.push("-v to list the ✓ rows".to_owned());
            }
            Some(flags.join(" · "))
        }
        Answer::Move(result) if !result.applied => Some(self.move_hint()),
        Answer::MoveSymbol(result) if !result.applied => Some(self.move_hint()),
        Answer::Batch(result) if !result.applied => Some(self.move_hint()),
        Answer::History(result) if result.entries.is_empty() => Some(
            "--apply writes a plan and records it here; `vvv undo` reverses the newest".to_owned(),
        ),
        _ => None,
    };
        if let Some(hint) = hint {
            report.block_note(Block::Note(Note::Hint(hint)));
        }
    }

    fn move_hint(&self) -> String {
        let mut flags = vec!["--apply to write"];
        if !self.options.diff {
            flags.push("--diff for the full patch");
        }
        flags.join(" · ")
    }
}
