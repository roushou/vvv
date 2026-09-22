//! The picker's compact view: one tight row per hit, where the terminal's
//! view spends a file header and an ordinal on each.

use vvv_engine::Match;
use vvv_engine::Role as MatchRole;
use vvv_engine::protocol::display::{Line, Role};
use vvv_engine::protocol::vocabulary::Mark;
use vvv_engine::report::{Block, Detailed, Options, Row, View};
use vvv_engine::{Notice, NoticeKind, Occurrence, Respelling};

/// Hits as one row each: glyph, `short:line`, kind and name, address.
#[derive(Debug, Default)]
pub struct Compact;

impl View for Compact {
    fn rows(&self, block: &Block, options: Options, width: usize) -> Vec<Row> {
        match block {
            Block::Matches(matches) => matches.iter().map(|m| hit(m, width)).collect(),
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
        let site_width = 22;
        let mut line = Line::mark(Mark::ticked(ticked))
            .and(Role::Plain, " ")
            .and_line(Line::mark(Mark::from(occurrence.reason)))
            .and(Role::Plain, " ");
        let site = format!(
            "{}:{}",
            crate::model::short(&occurrence.m.path),
            occurrence.m.start.line + 1
        );
        line = line
            .and(Role::Path, format!("{site:<site_width$}"))
            .and(Role::Plain, " ");
        let budget = width.saturating_sub(4 + site_width + 1);
        line = line.and_line(fit(Line::hit(&occurrence.m, Role::Plain), budget));
        Row::at(line, occurrence.m.path.clone(), occurrence.m.start.line)
    }

    fn respelling(&self, respelling: &Respelling, width: usize) -> Row {
        let site_width = 22;
        let mut line = Line::mark(Mark::Import).and(Role::Plain, " ");
        let site = format!(
            "{}:{}",
            crate::model::short(&respelling.path),
            respelling.start.line + 1
        );
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
        line = line.and_line(fit(Line::of(Role::Plain, name), budget));
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
        let site = format!(
            "{}:{}",
            crate::model::short(&notice.path),
            notice.start.line + 1
        );
        line = line
            .and(Role::Path, format!("{site:<site_width$}"))
            .and(Role::Plain, " ");
        line = line.and_line(fit(
            Line::of(Role::Plain, what),
            width.saturating_sub(site_width + 3),
        ));
        Row::at(line, notice.path.clone(), notice.start.line)
    }

    fn rewrite(
        &self,
        m: &Match,
        _after: Option<&str>,
        _ordinal: usize,
        ticked: bool,
        width: usize,
    ) -> Row {
        let site_width = 22;
        let mut line = Line::mark(Mark::ticked(ticked)).and(Role::Plain, " ");
        let site = format!("{}:{}", crate::model::short(&m.path), m.start.line + 1);
        line = line
            .and(Role::Path, format!("{site:<site_width$}"))
            .and(Role::Plain, " ");
        let budget = width.saturating_sub(2 + site_width + 1);
        line = line.and_line(fit(Line::hit(m, Role::Plain), budget));
        Row::at(line, m.path.clone(), m.start.line)
    }
}

/// `● short:line   kind name   ◆ address` for a declaration; the glyph, the
/// site and the source hit for anything else.
fn hit(m: &Match, width: usize) -> Row {
    let site_width = 22;
    let mut line = match m.role {
        MatchRole::Declaration => Line::mark(Mark::Declaration).and(Role::Plain, " "),
        MatchRole::Import => Line::mark(Mark::Import).and(Role::Plain, " "),
        MatchRole::Use => Line::of(Role::Plain, "  "),
    };
    let site = format!("{}:{}", crate::model::short(&m.path), m.start.line + 1);
    line = line
        .and(Role::Path, format!("{site:<site_width$}"))
        .and(Role::Plain, " ");
    let budget = width.saturating_sub(2 + site_width + 1);
    match (&m.symbol, &m.address) {
        (Some(symbol), address) if m.role == MatchRole::Declaration => {
            let name = format!("{} {}", symbol.kind, symbol.name);
            line = line.and(Role::Declaration, name.clone());
            if let Some(address) = address {
                let rest = budget.saturating_sub(name.chars().count() + 3);
                let text = format!("◆ {address}");
                if text.chars().count() <= rest {
                    line = line.and(Role::Plain, "   ").and(Role::Address, text);
                }
            }
        }
        _ => {
            let rest = if m.role == MatchRole::Import {
                Role::Import
            } else {
                Role::Plain
            };
            line = line.and_line(fit(Line::hit(m, rest), budget));
        }
    }
    Row::at(line, m.path.clone(), m.start.line)
}

/// Cut `line` to `budget` columns, `…` on the cut.
fn fit(line: Line, budget: usize) -> Line {
    let total: usize = line.pieces().iter().map(|p| p.text.chars().count()).sum();
    if total <= budget {
        return line;
    }
    let last = line.pieces().len().saturating_sub(1);
    let mut remaining = budget.saturating_sub(1);
    let mut out = Line::new();
    for (i, piece) in line.pieces().iter().enumerate() {
        let take = piece.text.chars().count().min(remaining);
        let mut text: String = piece.text.chars().take(take).collect();
        remaining = remaining.saturating_sub(take);
        if i == last {
            text.push('…');
        }
        out = out.and(piece.role, text);
    }
    out
}
