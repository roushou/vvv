//! Immutable review data and bounded delivery, independent of workspace freshness.
use super::{PlanId, PlanReview};
use crate::{Engine, EngineError, Intent, Move, MutationAnswer, PageBudget, Rename, Rewrite};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum PlanPreview {
    Rename(Arc<Rename>),
    Rewrite(Arc<Rewrite>),
    Move(Arc<Move>),
    MoveSymbol(Arc<crate::MoveSymbol>),
}
impl PlanPreview {
    pub(crate) fn from_mutation(value: &MutationAnswer) -> Result<Self, EngineError> {
        match value {
            MutationAnswer::Rename(r) => Ok(Self::Rename(Arc::new(r.clone()))),
            MutationAnswer::Rewrite(r) => Ok(Self::Rewrite(Arc::new(r.clone()))),
            MutationAnswer::Move(r) => Ok(Self::Move(Arc::new(r.clone()))),
            MutationAnswer::MoveSymbol(r) => Ok(Self::MoveSymbol(Arc::new(r.clone()))),
            _ => Err(EngineError::InvalidPlan),
        }
    }
    pub fn files(&self) -> &[crate::FileChange] {
        match self {
            Self::Rename(r) => &r.files,
            Self::Rewrite(r) => &r.files,
            Self::Move(r) => &r.files,
            Self::MoveSymbol(r) => &r.files,
        }
    }
    fn intent(&self) -> Intent {
        match self {
            Self::Rename(r) => Intent::Rename(r.intent.clone()),
            Self::Rewrite(r) => Intent::Rewrite(r.intent.clone()),
            Self::Move(r) => Intent::Move(r.intent.clone()),
            Self::MoveSymbol(r) => Intent::MoveSymbol(r.intent.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum PlanReviewReply {
    Complete(PlanReview),
    Page(PlanReviewPage),
}
impl PlanReviewReply {
    pub fn into_complete(self) -> Result<PlanReview, EngineError> {
        match self {
            Self::Complete(r) => Ok(r),
            Self::Page(_) => Err(EngineError::InvalidPlan),
        }
    }
    pub(crate) fn budget(
        max_bytes: usize,
        page: Option<&PageBudget>,
    ) -> Result<PageBudget, EngineError> {
        let mut budget = PageBudget {
            max_bytes,
            max_items: 1,
        };
        budget.validate()?;
        if let Some(page) = page {
            page.validate()?;
            budget.max_items = page.max_items;
            budget.max_bytes = budget.max_bytes.min(page.max_bytes);
        }
        Ok(budget)
    }
}

/// A plan-store cursor. It cannot be used with source-query continue or expand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct PlanReviewCursor(String);
impl PlanReviewCursor {
    fn new(id: &PlanId, record: usize, offset: usize) -> Self {
        Self(format!("r1.{}.{record:016x}.{offset:016x}", id.0))
    }
    pub(crate) fn position(&self) -> Result<(PlanId, usize, usize), EngineError> {
        let parts: Vec<_> = self.0.split('.').collect();
        if parts.len() != 6
            || parts[0] != "r1"
            || parts[1] != "p1"
            || !parts[2..]
                .iter()
                .all(|p| p.len() == 16 && p.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(EngineError::InvalidCursor);
        }
        let record = usize::from_str_radix(parts[4], 16).map_err(|_| EngineError::InvalidCursor)?;
        let offset = usize::from_str_radix(parts[5], 16).map_err(|_| EngineError::InvalidCursor)?;
        Ok((PlanId(parts[1..4].join(".")), record, offset))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ReviewSection {
    Move,
    Piece,
    Notice,
    Respelling,
    Declaration,
    Occurrence,
    File,
    Edit,
    Diff,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReviewTotals {
    pub declarations: usize,
    pub occurrences: usize,
    pub files: usize,
    pub edits: usize,
    #[serde(default, skip_serializing_if = "ReviewTotals::zero")]
    pub notices: usize,
    #[serde(default, skip_serializing_if = "ReviewTotals::zero")]
    pub respellings: usize,
    #[serde(default, skip_serializing_if = "ReviewTotals::zero")]
    pub pieces: usize,
}
impl ReviewTotals {
    fn zero(value: &usize) -> bool {
        *value == 0
    }
}
/// Metadata uses the corresponding ordinary preview record shape. Text fields
/// replaced by empty strings are reconstructed by following JSON-pointer chunks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanReviewItem {
    Metadata {
        section: ReviewSection,
        index: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_index: Option<usize>,
        value: Value,
    },
    Text {
        section: ReviewSection,
        index: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_index: Option<usize>,
        field: String,
        offset: usize,
        total_bytes: usize,
        text: String,
        complete: bool,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PlanReviewPage {
    pub kind: PlanReviewKind,
    pub plan_id: PlanId,
    pub lifetime_seconds: u64,
    pub review_id: crate::ContentId,
    pub mutation: ReviewMutation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<Intent>,
    pub totals: ReviewTotals,
    pub items: Vec<PlanReviewItem>,
    pub next_cursor: Option<PlanReviewCursor>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PlanReviewKind {
    PlanReview,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ReviewMutation {
    Rename,
    Rewrite,
    Move,
    MoveSymbol,
}

pub(crate) struct CapturedReview {
    preview: PlanPreview,
    identity: crate::ContentId,
    totals: ReviewTotals,
    records: Vec<PlanReviewItem>,
}
impl CapturedReview {
    pub(crate) fn new(preview: PlanPreview) -> Self {
        let identity =
            crate::ContentId::of(&serde_json::to_string(&preview).expect("preview serializes"));
        let mut review = Self {
            preview,
            identity,
            totals: ReviewTotals {
                declarations: 0,
                occurrences: 0,
                files: 0,
                edits: 0,
                notices: 0,
                respellings: 0,
                pieces: 0,
            },
            records: vec![],
        };
        let rename = match &review.preview {
            PlanPreview::Rename(r) => Some(r.clone()),
            PlanPreview::Rewrite(_) | PlanPreview::Move(_) | PlanPreview::MoveSymbol(_) => None,
        };
        if let Some(r) = rename {
            review.totals.declarations = r.declarations.len();
            review.totals.occurrences = r.occurrences.len();
            for (index, declaration) in r.declarations.iter().enumerate() {
                review.record(
                    ReviewSection::Declaration,
                    index,
                    None,
                    serde_json::to_value(declaration).expect("match serializes"),
                );
            }
            for (index, occurrence) in r.occurrences.iter().enumerate() {
                review.record(
                    ReviewSection::Occurrence,
                    index,
                    None,
                    serde_json::to_value(occurrence).expect("occurrence serializes"),
                );
            }
        }
        if let PlanPreview::Move(moved) = &review.preview {
            let moved = moved.clone();
            let mut metadata = serde_json::json!({"from": moved.from, "to": moved.to});
            if let Some(address) = &moved.from_address {
                metadata["from_address"] =
                    serde_json::to_value(address).expect("address serializes");
            }
            if let Some(address) = &moved.to_address {
                metadata["to_address"] = serde_json::to_value(address).expect("address serializes");
            }
            review.record(ReviewSection::Move, 0, None, metadata);
            review.totals.notices = moved.notices.len();
            review.totals.respellings = moved.respellings.len();
            for (index, notice) in moved.notices.iter().enumerate() {
                review.record(
                    ReviewSection::Notice,
                    index,
                    None,
                    serde_json::to_value(notice).expect("notice serializes"),
                );
            }
            for (index, respelling) in moved.respellings.iter().enumerate() {
                review.record(
                    ReviewSection::Respelling,
                    index,
                    None,
                    serde_json::to_value(respelling).expect("respelling serializes"),
                );
            }
        }
        if let PlanPreview::MoveSymbol(moved) = &review.preview {
            let moved = moved.clone();
            review.record(
                ReviewSection::Move,
                0,
                None,
                serde_json::json!({"from": moved.from, "to": moved.to}),
            );
            review.totals.declarations = 1;
            review.totals.pieces = moved.pieces.len();
            review.record(
                ReviewSection::Declaration,
                0,
                None,
                serde_json::to_value(&moved.declaration).expect("declaration serializes"),
            );
            for (index, piece) in moved.pieces.iter().enumerate() {
                review.record(
                    ReviewSection::Piece,
                    index,
                    None,
                    serde_json::to_value(piece).expect("piece serializes"),
                );
            }
            review.totals.notices = moved.notices.len();
            review.totals.respellings = moved.respellings.len();
            for (index, notice) in moved.notices.iter().enumerate() {
                review.record(
                    ReviewSection::Notice,
                    index,
                    None,
                    serde_json::to_value(notice).expect("notice serializes"),
                );
            }
            for (index, respelling) in moved.respellings.iter().enumerate() {
                review.record(
                    ReviewSection::Respelling,
                    index,
                    None,
                    serde_json::to_value(respelling).expect("respelling serializes"),
                );
            }
        }
        review.totals.files = review.preview.files().len();
        // Build one file at a time rather than cloning the full preview.
        for index in 0..review.totals.files {
            let file = &review.preview.files()[index];
            let mut metadata = serde_json::json!({"path": file.path});
            if let Some(destination) = &file.moved_to {
                metadata["moved_to"] = serde_json::to_value(destination).expect("path serializes");
            }
            let edits = file
                .edits
                .iter()
                .map(|r| serde_json::to_value(r).expect("edit serializes"))
                .collect::<Vec<_>>();
            let diff = file.diff.to_string();
            review.record(ReviewSection::File, index, None, metadata);
            review.totals.edits += edits.len();
            for (edit_index, edit) in edits.into_iter().enumerate() {
                review.record(ReviewSection::Edit, edit_index, Some(index), edit);
            }
            review.records.push(PlanReviewItem::Text {
                section: ReviewSection::Diff,
                index,
                file_index: None,
                field: "/diff".into(),
                offset: 0,
                total_bytes: diff.len(),
                complete: true,
                text: diff,
            });
        }
        review
    }
    fn record(
        &mut self,
        section: ReviewSection,
        index: usize,
        file_index: Option<usize>,
        mut value: Value,
    ) {
        let mut text = vec![];
        Self::text_fields(&mut value, "", &mut text);
        self.records.push(PlanReviewItem::Metadata {
            section,
            index,
            file_index,
            value,
        });
        for (field, text) in text {
            self.records.push(PlanReviewItem::Text {
                section,
                index,
                file_index,
                field,
                offset: 0,
                total_bytes: text.len(),
                complete: true,
                text,
            });
        }
    }
    fn text_fields(value: &mut Value, pointer: &str, fields: &mut Vec<(String, String)>) {
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    let pointer =
                        format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1"));
                    if matches!(
                        key.as_str(),
                        "text" | "line" | "replacement" | "import" | "item" | "from" | "to"
                    ) && value.is_string()
                    {
                        let text = value.as_str().expect("string").to_owned();
                        if !text.is_empty() {
                            fields.push((pointer, text));
                            *value = Value::String(String::new());
                        }
                    } else {
                        Self::text_fields(value, &pointer, fields);
                    }
                }
            }
            Value::Array(array) => {
                for (index, value) in array.iter_mut().enumerate() {
                    Self::text_fields(value, &format!("{pointer}/{index}"), fields);
                }
            }
            _ => {}
        }
    }
    pub(crate) fn weight(&self) -> usize {
        crate::query_store::QueryStore::weight(&self.records)
            .saturating_add(crate::query_store::QueryStore::weight(&self.preview))
    }
    pub(crate) fn page(
        &self,
        engine: &Engine,
        id: &PlanId,
        mut record: usize,
        mut offset: usize,
        budget: PageBudget,
    ) -> Result<PlanReviewPage, EngineError> {
        engine.check_read()?;
        if record > self.records.len() || (record == self.records.len() && offset != 0) {
            return Err(EngineError::InvalidCursor);
        }
        let mut page = PlanReviewPage {
            kind: PlanReviewKind::PlanReview,
            plan_id: id.clone(),
            lifetime_seconds: crate::PlanLimits::default().lifetime_seconds,
            review_id: self.identity.clone(),
            mutation: match self.preview {
                PlanPreview::Rename(_) => ReviewMutation::Rename,
                PlanPreview::Rewrite(_) => ReviewMutation::Rewrite,
                PlanPreview::Move(_) => ReviewMutation::Move,
                PlanPreview::MoveSymbol(_) => ReviewMutation::MoveSymbol,
            },
            intent: (record == 0 && offset == 0).then(|| self.preview.intent()),
            totals: self.totals.clone(),
            items: vec![],
            next_cursor: None,
        };
        while record < self.records.len() && page.items.len() < budget.max_items {
            engine.check_read()?;
            let original = &self.records[record];
            let length = match original {
                PlanReviewItem::Text { text, .. } => {
                    if offset >= text.len() && !text.is_empty() || !text.is_char_boundary(offset) {
                        return Err(EngineError::InvalidCursor);
                    }
                    text.len()
                }
                _ => {
                    if offset != 0 {
                        return Err(EngineError::InvalidCursor);
                    }
                    0
                }
            };
            let minimum = match original {
                PlanReviewItem::Text { text, .. } => text[offset..]
                    .chars()
                    .next()
                    .map_or(offset, |c| offset + c.len_utf8()),
                _ => 0,
            };
            let mut best = None;
            // The final chunk removes its cursor: test it separately because this
            // envelope change makes byte fitting non-monotonic at the endpoint.
            if length.saturating_sub(offset) <= budget.max_bytes {
                page.items.push(original.chunk(offset, length));
                page.next_cursor = (record + 1 < self.records.len())
                    .then(|| PlanReviewCursor::new(id, record + 1, 0));
                if budget.check(&page).is_ok() {
                    best = Some((length, (record + 1, 0)));
                }
                page.items.pop();
            }
            if best.is_none() && minimum < length {
                let mut low = minimum;
                let mut high = (length - 1).min(offset.saturating_add(budget.max_bytes));
                while low <= high {
                    engine.check_read()?;
                    let middle = low + (high - low) / 2;
                    let mut end = middle;
                    if let PlanReviewItem::Text { text, .. } = original {
                        while !text.is_char_boundary(end) {
                            end -= 1;
                        }
                    }
                    page.items.push(original.chunk(offset, end));
                    page.next_cursor = Some(PlanReviewCursor::new(id, record, end));
                    let fits = budget.check(&page).is_ok();
                    page.items.pop();
                    if fits {
                        best = Some((end, (record, end)));
                        low = middle + 1;
                    } else {
                        if middle == 0 {
                            break;
                        }
                        high = middle - 1;
                    }
                }
            }
            if let Some((end, successor)) = best {
                page.items.push(original.chunk(offset, end));
                (record, offset) = successor;
            } else if page.items.is_empty() {
                page.items.push(original.chunk(offset, minimum));
                let successor = if minimum == length {
                    (record + 1, 0)
                } else {
                    (record, minimum)
                };
                page.next_cursor = (successor.0 < self.records.len())
                    .then(|| PlanReviewCursor::new(id, successor.0, successor.1));
                budget.check(&page)?;
                unreachable!("minimum record did not fit")
            } else {
                break;
            }
        }
        page.next_cursor =
            (record < self.records.len()).then(|| PlanReviewCursor::new(id, record, offset));
        budget.check(&page)?;
        engine.check_read()?;
        Ok(page)
    }
}
impl PlanReviewItem {
    fn chunk(&self, start: usize, end: usize) -> Self {
        match self {
            Self::Metadata { .. } => self.clone(),
            Self::Text {
                section,
                index,
                file_index,
                field,
                total_bytes,
                text,
                ..
            } => Self::Text {
                section: *section,
                index: *index,
                file_index: *file_index,
                field: field.clone(),
                offset: start,
                total_bytes: *total_bytes,
                text: text[start..end].to_owned(),
                complete: end == *total_bytes,
            },
        }
    }
}
impl crate::report::Document {
    pub(crate) fn plan_reply(reply: &PlanReviewReply) -> Self {
        match reply {
            PlanReviewReply::Complete(r) => Self::plan_review(r),
            PlanReviewReply::Page(p) => Self::plan_page(p),
        }
    }
    pub(crate) fn plan_page(page: &PlanReviewPage) -> Self {
        use crate::protocol::display::{Line, Role};
        let mut doc = Self::new();
        doc.body([Line::of(Role::Plain, "Plan: ").and(Role::Plain, &page.plan_id.0)]);
        for item in &page.items {
            doc.body([Line::of(
                Role::Plain,
                serde_json::to_string(item).expect("review item serializes"),
            )]);
        }
        if let Some(cursor) = &page.next_cursor {
            doc.notes([Line::of(Role::Plain, "Review cursor: ").and(Role::Plain, &cursor.0)]);
        }
        doc
    }
}
