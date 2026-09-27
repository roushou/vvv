//! The picker's compact view: one tight row per hit, where the terminal's
//! view spends a file header and an ordinal on each.

use vvv_engine::Match;
use vvv_engine::Role as MatchRole;
use vvv_engine::protocol::display::{Line, Role};
use vvv_engine::protocol::vocabulary::Mark;
use vvv_engine::report::{Block, Detailed, Options, Row, View};
use vvv_engine::{Notice, NoticeKind, Occurrence, Respelling};

/// Hits as one row each: glyph, `short:line`, kind and name.
#[derive(Debug, Default)]
pub struct Compact;

impl View for Compact {
    fn rows(&self, block: &Block, options: Options, width: usize) -> Vec<Row> {
        match block {
            Block::Matches(matches) => matches.iter().map(|m| Self::hit(m, width)).collect(),
            other => Detailed.rows(other, options, width),
        }
    }

    fn occurrence(
        &self,
        occurrence: &Occurrence,
        _ordinal: usize,
        ticked: bool,
        width: usize,
    ) -> Row {
        Self::occurrence_line(occurrence, Some(ticked), width)
    }

    fn relation(&self, occurrence: &Occurrence, _ordinal: usize, width: usize) -> Row {
        Self::occurrence_line(occurrence, None, width)
    }

    fn respelling(&self, respelling: &Respelling, width: usize) -> Row {
        let site_width = 22;
        let mut line = Line::mark(Mark::Import).and(Role::Plain, " ");
        let site = format!("{}:{}", respelling.path.short(), respelling.start.line + 1);
        line = line
            .and(Role::Path, format!("{site:<site_width$}"))
            .and(Role::Plain, " ");
        let name = respelling
            .to
            .rsplit("::")
            .next()
            .unwrap_or(&respelling.to)
            .to_owned();
        let budget = width.saturating_sub(site_width + 3);
        line = line.and_line(Line::of(Role::Plain, name).fit(budget));
        Row::at(line, respelling.path.clone(), respelling.start.line)
    }

    fn notice(&self, notice: &Notice, width: usize) -> Row {
        let site_width = 22;
        let what = match &notice.kind {
            NoticeKind::UnrewritableImport { import, .. } => {
                format!("{import}  grouped import, by hand")
            }
            NoticeKind::RedundantImport { import } => {
                format!("{import}  now declared here, remove by hand")
            }
            NoticeKind::Unreachable { item, from, .. } => {
                format!("{item}  used from {from}; `pub` is your call")
            }
        };
        let mut line = Line::mark(Mark::ByHand).and(Role::Plain, " ");
        let site = format!("{}:{}", notice.path.short(), notice.start.line + 1);
        line = line
            .and(Role::Path, format!("{site:<site_width$}"))
            .and(Role::Plain, " ");
        line = line.and_line(Line::of(Role::Plain, what).fit(width.saturating_sub(site_width + 3)));
        Row::at(line, notice.path.clone(), notice.start.line)
    }

    fn rewrite(&self, m: &Match, _ordinal: usize, ticked: bool, width: usize) -> Row {
        let site_width = 22;
        let mut line = Line::mark(Mark::ticked(ticked)).and(Role::Plain, " ");
        let site = format!("{}:{}", m.path.short(), m.start.line + 1);
        line = line
            .and(Role::Path, format!("{site:<site_width$}"))
            .and(Role::Plain, " ");
        let budget = width.saturating_sub(2 + site_width + 1);
        line = line.and_line(Line::hit(m, Role::Plain).fit(budget));
        Row::at(line, m.path.clone(), m.start.line)
    }
}

impl Compact {
    /// One occurrence row. `ticked` is `Some` for a plan verdict (with its
    /// checkbox) and `None` for a relation view, which reads rather than
    /// commits.
    fn occurrence_line(occurrence: &Occurrence, ticked: Option<bool>, width: usize) -> Row {
        let site_width = 22;
        let mut line = match ticked {
            Some(ticked) => Line::mark(Mark::ticked(ticked)).and(Role::Plain, " "),
            None => Line::new(),
        }
        .and_line(Line::mark(Mark::from(occurrence.reason)))
        .and(Role::Plain, " ");
        let site = format!(
            "{}:{}",
            occurrence.m.path.short(),
            occurrence.m.start.line + 1
        );
        line = line
            .and(Role::Path, format!("{site:<site_width$}"))
            .and(Role::Plain, " ");
        let used = site_width + if ticked.is_some() { 4 } else { 2 } + 1;
        let budget = width.saturating_sub(used);
        line = line.and_line(Line::hit(&occurrence.m, Role::Plain).fit(budget));
        Row::at(line, occurrence.m.path.clone(), occurrence.m.start.line)
    }

    /// `● short:line   kind name` for a declaration; the glyph, the site
    /// and the source hit for anything else.
    fn hit(m: &Match, width: usize) -> Row {
        let site_width = 22;
        let mut line = match m.role {
            MatchRole::Declaration => Line::mark(Mark::Declaration).and(Role::Plain, " "),
            MatchRole::Import => Line::mark(Mark::Import).and(Role::Plain, " "),
            MatchRole::Use => Line::of(Role::Plain, "  "),
        };
        let site = format!("{}:{}", m.path.short(), m.start.line + 1);
        line = line
            .and(Role::Path, format!("{site:<site_width$}"))
            .and(Role::Plain, " ");
        let budget = width.saturating_sub(2 + site_width + 1);
        if m.role == MatchRole::Declaration
            && let Some(symbol) = &m.symbol
        {
            line = line.and(
                Role::Declaration,
                format!("{} {}", symbol.kind, symbol.name),
            );
        } else {
            let rest = if m.role == MatchRole::Import {
                Role::Import
            } else {
                Role::Plain
            };
            line = line.and_line(Line::hit(m, rest).fit(budget));
        }
        Row::at(line, m.path.clone(), m.start.line)
    }
}
