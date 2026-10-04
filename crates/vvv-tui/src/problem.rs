//! A failed request and the recovery its typed outcome permits.
use crate::action::Effect;
use crate::render::{Fit, Painter, Pane};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::Widget,
};
use vvv_engine::{ErrorCode, Failure};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub scroll: usize,
    at_end: bool,
    pub failure: Failure,
    pub retry: Option<Effect>,
}
impl Problem {
    pub fn new(failure: Failure, retry: Option<Effect>) -> Self {
        Self {
            scroll: 0,
            at_end: false,
            failure,
            retry,
        }
    }
    pub fn message(&self) -> &str {
        &self.failure.message
    }
    pub fn navigate(&mut self, action: crate::action::Action) {
        use crate::action::Action;
        match action {
            Action::Top => {
                self.scroll = 0;
                self.at_end = false;
            }
            Action::Bottom => {
                self.scroll = 0;
                self.at_end = true;
            }
            Action::Scroll(n) | Action::Page(n) => {
                let step = if matches!(action, Action::Page(_)) {
                    i64::from(n) * 10
                } else {
                    i64::from(n)
                };
                self.scroll =
                    self.scroll
                        .saturating_add_signed(if self.at_end { -step } else { step } as isize);
            }
            _ => {}
        }
    }
    pub fn can_retry(&self) -> bool {
        self.retry.is_some()
            && self.failure.recovery.is_none()
            && matches!(
                self.failure.code,
                ErrorCode::Stale
                    | ErrorCode::Io
                    | ErrorCode::Cancelled
                    | ErrorCode::Incomplete
                    | ErrorCode::NotFound
                    | ErrorCode::PlanExpired
                    | ErrorCode::CursorExpired
            )
    }
    pub fn title(&self) -> &'static str {
        match self.failure.code {
            ErrorCode::Stale => "Preview out of date",
            ErrorCode::NoLanguage => "No language support",
            ErrorCode::NoLayout | ErrorCode::Unmovable => "Unavailable capability",
            ErrorCode::BadPattern | ErrorCode::BadQuery => "Invalid query",
            ErrorCode::BadTemplate => "Invalid template",
            ErrorCode::Conflict => "Conflicting changes",
            ErrorCode::Exists => "Destination exists",
            ErrorCode::RecoveryFailed => "Recovery incomplete",
            _ => "Request failed",
        }
    }
    pub fn site(&self) -> Option<vvv_engine::RelPath> {
        let recovery = self.failure.recovery.as_ref()?;
        recovery
            .remaining
            .first()
            .map(|r| r.path.clone())
            .or_else(|| recovery.unverified.first().map(|r| r.path.clone()))
            .or_else(|| recovery.failures.first().map(|r| r.path.clone()))
    }
    pub fn pane(&self, painter: Painter, focused: bool, area: Rect, buf: &mut Buffer) {
        let mut text = vec![self.failure.message.clone()];
        if let Some(hint) = &self.failure.hint {
            text.push(hint.clone());
        }
        if self.failure.code == ErrorCode::Stale {
            text.push("Refresh the source and review the new preview before applying.".into());
        }
        if matches!(
            self.failure.code,
            ErrorCode::NoLanguage | ErrorCode::NoLayout | ErrorCode::Unmovable
        ) {
            text.push(
                "Inspect the source or open it in your editor. This operation is unavailable here."
                    .into(),
            );
        }
        if let Some(recovery) = &self.failure.recovery {
            text.push(format!("Original failure: {}", recovery.cause.message));
            for issue in &recovery.failures {
                text.push(format!("{}: {}", issue.path, issue.message));
            }
            for remaining in &recovery.remaining {
                text.push(format!(
                    "{}: an effect remains; inspect this file before further changes.",
                    remaining.path
                ));
            }
            for unverified in &recovery.unverified {
                text.push(format!(
                    "{}: recovery could not be verified. {}",
                    unverified.path, unverified.message
                ));
            }
        }
        let width = area.width.saturating_sub(4) as usize;
        let mut rows = Vec::new();
        for (index, paragraph) in text.iter().enumerate() {
            let mut line = String::new();
            for word in paragraph.split_whitespace() {
                let combined = if line.is_empty() {
                    word.to_owned()
                } else {
                    format!("{line} {word}")
                };
                if Line::from(combined.as_str()).width() <= width {
                    line = combined;
                } else {
                    if !line.is_empty() {
                        rows.push(Line::from(std::mem::take(&mut line)));
                    }
                    let mut parts = Fit(word, width).wrapped();
                    line = parts.pop().unwrap_or_default();
                    rows.extend(parts.into_iter().map(Line::from));
                }
            }
            rows.push(Line::from(Span::styled(
                line,
                if index == 0 {
                    painter.error
                } else {
                    Default::default()
                },
            )));
        }
        let end = rows
            .len()
            .saturating_sub(area.height.saturating_sub(2) as usize);
        let scroll = if self.at_end {
            end.saturating_sub(self.scroll)
        } else {
            self.scroll.min(end)
        };
        Pane::new(
            painter,
            Line::from(Span::styled(self.title(), painter.warning)),
            focused,
        )
        .rows(rows)
        .scroll(scroll)
        .render(area, buf);
    }
}
