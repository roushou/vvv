mod common;
use common::Fake;
use std::sync::Arc;
use vvv_core::{DeclarationSignature, Language};
use vvv_engine::{
    ContextBudget, ContextDetail, ContextPageQuery, ContextQuery, ContextSignature, ContinueQuery,
    Engine, EngineError, ExpandQuery, Languages, MemoryVfs, NavigationQuery, PageBudget, PageReply,
    Position, Selection, Span, Symbol, SymbolKind, Vfs, WorkBudget, Workspace,
};

struct Fixture {
    engine: Engine,
    vfs: Arc<MemoryVfs>,
    source: String,
    signature: Span,
}
impl Fixture {
    fn new(long: bool, supported: bool) -> Self {
        let docs = if long {
            "é🙂\"\\\r\n".repeat(1500)
        } else {
            String::new()
        };
        let prefix = "def Related\ndef Hidden\n";
        let head = format!("{docs}def Root Related");
        let source = format!("{prefix}{head} Hidden {}", "body ".repeat(1500));
        let name = source.find("Root").unwrap();
        let signature = Span::new(prefix.len(), prefix.len() + head.len());
        let symbols = vec![
            Symbol::plain(
                SymbolKind::Function,
                "Related",
                Span::new(4, 11),
                Span::new(0, 11),
            ),
            Symbol::plain(
                SymbolKind::Function,
                "Hidden",
                Span::new(16, 22),
                Span::new(12, 22),
            ),
            Symbol::plain(
                SymbolKind::Function,
                "Root",
                Span::new(name, name + 4),
                Span::new(prefix.len(), source.len()),
            ),
        ];
        let fake = Fake::default().with_symbols(symbols);
        let mut facts = fake.facts(&source).unwrap();
        if supported {
            facts.signatures = facts
                .symbols
                .iter()
                .map(|symbol| DeclarationSignature {
                    name_span: symbol.name_span,
                    span: if symbol.name == "Root" {
                        signature
                    } else {
                        symbol.extent
                    },
                })
                .collect();
        }
        let vfs = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/package", "ws")
                .with_file("/ws/a.p", &source),
        );
        let engine = Engine::new(
            Workspace::new("/ws", vfs.clone()),
            Languages::new().with(fake.with_navigation_facts(facts)),
        );
        Self {
            engine,
            vfs,
            source,
            signature,
        }
    }
    fn query(&self) -> ContextQuery {
        let start = self.source.find("Root").unwrap();
        let position = vvv_core::text::LineIndex::new(&self.source).position(&self.source, start);
        let mut query = ContextQuery::new(NavigationQuery::at("a.p", position).origin);
        query.detail = ContextDetail::Signature;
        query.budget = ContextBudget::MAXIMUM;
        query
    }
    fn page(&self, bytes: usize) -> ContextPageQuery {
        ContextPageQuery {
            origin: self.query().origin,
            detail: ContextDetail::Signature,
            selection: Selection::All,
            references: false,
            include_enclosing: false,
            page: PageBudget {
                max_bytes: bytes,
                max_items: 1,
            },
            work: WorkBudget::default(),
        }
    }
    fn expand(&self, cursor: vvv_engine::Cursor, mut text: String, expected: Span) -> String {
        let mut next = Some(cursor);
        let mut offset = expected.start + text.len();
        while let Some(cursor) = next {
            let query = ExpandQuery {
                cursor,
                max_bytes: 2048,
            };
            let reply = query.clone().execute(&self.engine).unwrap();
            let replay = query.execute(&self.engine).unwrap();
            assert_eq!(
                serde_json::to_value(&reply).unwrap(),
                serde_json::to_value(replay).unwrap()
            );
            assert!(serde_json::to_vec(&reply).unwrap().len() <= 2048);
            assert_eq!(reply.requested.span, expected);
            assert_eq!(reply.excerpt.span.start, offset);
            assert_eq!(
                reply.text,
                self.source[reply.excerpt.span.start..reply.excerpt.span.end]
            );
            offset = reply.excerpt.span.end;
            text.push_str(&reply.text);
            next = reply.next_cursor;
            assert_eq!(reply.done, next.is_none());
        }
        assert_eq!(offset, expected.end);
        text
    }
}

#[test]
fn signature_context_excludes_body_dependencies_and_survives_continuation() {
    let f = Fixture::new(false, true);
    let expected = f.query().execute(&f.engine).unwrap();
    assert_eq!(expected.items.len(), 2);
    assert_eq!(expected.items[0].text, "def Root Related");
    assert_eq!(expected.items[1].text, "def Related");
    assert!(expected.items.iter().all(|item| item.complete));
    let first = f.page(4096).execute(&f.engine).unwrap();
    let mut items = first
        .items
        .iter()
        .map(|item| item.item.clone())
        .collect::<Vec<_>>();
    let mut cursor = first.next_cursor;
    while let Some(next) = cursor {
        let PageReply::Context(page) = (ContinueQuery {
            cursor: next,
            page: PageBudget::default(),
            work: None,
        })
        .execute(&f.engine)
        .unwrap() else {
            panic!()
        };
        items.extend(page.items.into_iter().map(|item| item.item));
        cursor = page.next_cursor;
    }
    assert_eq!(items, expected.items);
    let mut body = f.query();
    body.detail = ContextDetail::Body;
    assert_eq!(body.execute(&f.engine).unwrap().items.len(), 3);
}

#[test]
fn separate_handles_reconstruct_signature_and_whole_body_with_exact_unicode_ranges() {
    let f = Fixture::new(true, true);
    let query = f.page(2048);
    let page = query.clone().execute(&f.engine).unwrap();
    assert!(serde_json::to_vec(&page).unwrap().len() <= 2048);
    let item = &page.items[0];
    assert!(!item.item.complete);
    assert!(
        matches!(&item.item.signature, Some(ContextSignature::Available { requested }) if requested.span == f.signature)
    );
    let signature_cursor = item.expansion.clone().unwrap();
    let body_cursor = item.body_expansion.clone().unwrap();
    let signature = f.expand(
        signature_cursor.clone(),
        item.item.text.clone(),
        f.signature,
    );
    assert_eq!(signature, f.source[f.signature.start..f.signature.end]);
    let full = item.item.target.declaration.span;
    let body = f.expand(body_cursor.clone(), String::new(), full);
    assert_eq!(body, f.source[full.start..full.end]);
    let again = query.execute(&f.engine).unwrap();
    assert_eq!(page.snapshot, again.snapshot);
    assert_eq!(page.items[0].item, again.items[0].item);
    f.vfs
        .write(std::path::Path::new("/ws/a.p"), &format!("{} ", f.source))
        .unwrap();
    // Independent query sessions verify stale rejection for each handle kind.
    for cursor in [
        signature_cursor,
        again.items[0].body_expansion.clone().unwrap(),
    ] {
        assert!(matches!(
            ExpandQuery {
                cursor,
                max_bytes: 2048
            }
            .execute(&f.engine),
            Err(EngineError::StaleQuery)
        ));
    }
}

#[test]
fn unsupported_signature_is_explicit_and_full_body_remains_retrievable() {
    let f = Fixture::new(false, false);
    let one = f.query().execute(&f.engine).unwrap();
    assert_eq!(one.items.len(), 1);
    let item = &one.items[0];
    assert_eq!(item.signature, Some(ContextSignature::Unsupported));
    assert_eq!(
        item.excerpt.span,
        Span::new(f.signature.start, f.signature.start)
    );
    assert!(item.text.is_empty());
    assert!(!item.complete);
    assert_eq!(one.omissions.byte_limit, 0);
    let page = f.page(2048).execute(&f.engine).unwrap();
    assert_eq!(page.items[0].item, *item);
    assert!(page.items[0].expansion.is_none());
    let full = item.target.declaration.span;
    assert_eq!(
        f.expand(
            page.items[0].body_expansion.clone().unwrap(),
            String::new(),
            full
        ),
        f.source[full.start..full.end]
    );
}

#[test]
fn detail_defaults_to_body_and_rejects_unknown_values() {
    let base =
        serde_json::json!({"origin": NavigationQuery::at("a.p", Position::new(0, 0)).origin});
    let query: ContextPageQuery = serde_json::from_value(base.clone()).unwrap();
    assert_eq!(query.detail, ContextDetail::Body);
    let mut invalid = base;
    invalid["detail"] = "summary".into();
    assert!(serde_json::from_value::<ContextPageQuery>(invalid).is_err());
}

#[test]
fn omitted_unsupported_items_still_report_byte_pressure() {
    let f = Fixture::new(false, false);
    let mut query = ContextQuery::new(NavigationQuery::at("a.p", Position::new(0, 4)).origin);
    query.detail = ContextDetail::Signature;
    query.references = true;
    query.budget.max_bytes = 1024;
    let result = query.execute(&f.engine).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(
        result.items[0].signature,
        Some(ContextSignature::Unsupported)
    );
    assert_eq!(result.omissions.byte_limit, 1);
    assert!(serde_json::to_vec(&result).unwrap().len() <= 1024);
}
