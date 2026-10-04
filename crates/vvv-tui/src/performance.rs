//! Repeatable interactive workloads; timings are diagnostics, not CI thresholds.

use crate::action::{Action, Event};
use crate::fixtures;
use crate::model::Model;
use crate::render::Painter;
use crate::screen::App;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;
use std::hint::black_box;
use std::time::Instant;

struct Browsing {
    model: Model,
    buffer: Buffer,
}

impl Browsing {
    fn large_preview(&mut self) {
        let path = self.model.search.results.current().unwrap().path.clone();
        let text = "let engine = Engine::new();\n".repeat(20_000);
        let mut highlights = Vec::new();
        for (token, kind) in [
            ("let", vvv_engine::HighlightKind::Keyword),
            ("engine", vvv_engine::HighlightKind::Type),
            ("Engine", vvv_engine::HighlightKind::Type),
            ("new", vvv_engine::HighlightKind::Keyword),
        ] {
            highlights.extend(
                text.match_indices(token)
                    .map(|(start, _)| vvv_engine::Highlight {
                        span: vvv_engine::Span::new(start, start + token.len()),
                        kind,
                    }),
            );
        }
        highlights.sort_by_key(|h| h.span.start);
        self.model.on_event(Event::Previewed {
            path,
            text,
            highlights,
            symbols: vec![],
            identifiers: vec![],
        });
        self.model.search.preview_scroll = Some(19_980);
    }

    fn new(files: usize, hits: usize) -> Self {
        let mut model = Model::new("repo".into(), vec!["rust".into()]);
        let matches = (0..files)
            .flat_map(|file| {
                let path = format!("crates/package_{file:04}/src/implementation.rs");
                (0..hits).map(move |line| {
                    let mut m = fixtures::m(&path, line as u32, 0, "Engine", "Engine::new()");
                    m.id = vvv_engine::MatchId::derive(&m.path, m.span, &line.to_string());
                    m
                })
            })
            .collect();
        model.on_event(Event::Searched {
            generation: model.generation,
            matches,
            skipped: vec![],
        });
        model.on_event(Event::Viewport {
            width: 120,
            height: 40,
        });
        Self {
            model,
            buffer: Buffer::empty(Rect::new(0, 0, 120, 40)),
        }
    }

    fn draw(&mut self) {
        self.buffer.reset();
        App::new(&self.model, Painter::colored(), 0).render(self.buffer.area, &mut self.buffer);
        black_box(&self.buffer);
    }

    fn measure(&mut self, label: &str, count: usize, mut operation: impl FnMut(&mut Self)) {
        let mut samples = Vec::new();
        for _ in 0..count {
            let start = Instant::now();
            operation(self);
            samples.push(start.elapsed());
        }
        samples.sort();
        println!(
            "{label}: median {:.3} ms, max {:.3} ms ({count} samples)",
            samples[count / 2].as_secs_f64() * 1000.0,
            samples[count - 1].as_secs_f64() * 1000.0,
        );
    }
}

#[test]
#[ignore = "diagnostic workload; run with --release --ignored --nocapture"]
fn large_result_browsing() {
    let mut browsing = Browsing::new(1_000, 100);
    browsing.draw();
    browsing.measure("100k hits: redraw", 10, Browsing::draw);
    browsing.measure("100k hits: file movement + redraw", 10, |b| {
        black_box(b.model.update(Action::File(1)));
        b.draw();
    });
    browsing.model.update(Action::FilterFiles);
    browsing.measure("100k hits: filter edit + redraw", 5, |b| {
        black_box(b.model.update(Action::Input('0')));
        b.draw();
        black_box(b.model.update(Action::Backspace));
    });
    let mut dense = Browsing::new(10, 10_000);
    dense.draw();
    dense.measure("10k hits in one file: redraw", 10, Browsing::draw);
    let mut preview = Browsing::new(1, 1);
    preview.large_preview();
    preview.measure("20k source lines: redraw near end", 10, Browsing::draw);
    preview.model.update(Action::FocusNth(4));
    preview.model.update(Action::InspectFind);
    for c in "Engine".chars() {
        preview.model.update(Action::Input(c));
    }
    preview.model.update(Action::Enter);
    preview.measure("20k find hits: next hit + redraw", 10, |b| {
        black_box(b.model.update(Action::InspectNext(1)));
        b.draw();
    });
    preview.model.update(Action::ExpandPreview);
    preview.model.update(Action::InspectHorizontal(8));
    preview.measure(
        "20k find hits: expanded horizontal redraw",
        10,
        Browsing::draw,
    );
}
