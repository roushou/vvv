//! Shared key, focus, and help metadata, and rendering bound to a typed view.
//! Mode state, transitions, and screens live together under `crate::modes`.
//! Drawing primitives live in [`crate::render`].

pub(crate) mod defaults;
use crate::modes::history::screen as history;
use crate::modes::moves::screen as moving;
use crate::modes::rename::screen as rename;
use crate::modes::rewrite::screen as rewrite;
use crate::modes::search::screen as search;
#[cfg(test)]
use crate::overlays::screen as overlay;
mod status;

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::Widget;

use super::action::Action;
use super::keymap::{Dispatch, Key, Layer, Row, When};
use super::model::{Model, PanelKind};
use crate::render::{Painter, Region};

use self::status::StatusBar;

/// One region of a screen: its own keys, the kind that selects the shared
/// defaults.
#[derive(Debug, Clone, Copy)]
pub struct Panel {
    pub layer: Layer<Action>,
    /// `None` for a region that is drawn but never focused, such as a
    /// header or an overlay's box.
    pub kind: Option<PanelKind>,
}

/// One screen: the keys that work from any of its panels, the panels it is
/// made of.
#[derive(Debug, Clone, Copy)]
pub struct Screen {
    pub layer: Layer<Action>,
    pub panels: &'static [Panel],
}

impl Screen {
    /// The n-th focusable panel.
    pub fn panel(&self, focus: usize) -> Option<&'static Panel> {
        self.focusable().nth(focus)
    }

    fn focusable(&self) -> impl Iterator<Item = &'static Panel> {
        self.panels.iter().filter(|p| p.kind.is_some())
    }

    /// What `key` does here: the globals first, then the focused panel, the
    /// screen, the panel's kind defaults, then navigation.
    pub fn resolve(
        &self,
        focus: usize,
        key: Key,
        holds: impl Fn(When) -> bool,
    ) -> Option<Dispatch<Action>> {
        let panel = self.panel(focus);
        if let Some(dispatch) = defaults::GLOBAL.resolve(key, &holds) {
            return Some(dispatch);
        }
        let Some(panel) = panel else {
            return self.layer.resolve(key, &holds);
        };
        if let Some(dispatch) = panel.layer.resolve(key, &holds) {
            return Some(dispatch);
        }
        if let Some(dispatch) = self.layer.resolve(key, &holds) {
            return Some(dispatch);
        }
        if let Some(kind) = panel.kind
            && let Some(dispatch) = kind.layer().and_then(|l| l.resolve(key, &holds))
        {
            return Some(dispatch);
        }
        if let Some(dispatch) = defaults::NAVIGATE.resolve(key, &holds) {
            return Some(dispatch);
        }
        if panel.kind != Some(PanelKind::Input)
            && let Some(dispatch) = defaults::DIGITS.resolve(key, &holds)
        {
            return Some(dispatch);
        }
        None
    }

    /// Effective bindings for this focus, grouped by their owning layer.
    pub fn sections(
        &self,
        focus: usize,
        holds: impl Fn(When) -> bool,
    ) -> Vec<(&'static str, Vec<Row<'static, Action>>)> {
        let mut layers = Vec::new();
        if let Some(panel) = self.panel(focus) {
            layers.push(panel.layer);
            layers.push(self.layer);
            if let Some(layer) = panel.kind.and_then(PanelKind::layer) {
                layers.push(*layer);
            }
            layers.push(defaults::NAVIGATE);
            if panel.kind != Some(PanelKind::Input) {
                layers.push(defaults::DIGITS);
            }
        } else {
            layers.push(self.layer);
        }
        layers.push(defaults::GLOBAL);
        let mut sections = Sections::default();
        let mut seen = Vec::new();
        for layer in layers {
            let mut rows: Vec<Row<'static, Action>> = Vec::new();
            for binding in layer.bindings.iter().filter(|b| holds(b.when)) {
                let triggers: Vec<_> = binding
                    .triggers
                    .iter()
                    .copied()
                    .filter(|trigger| {
                        let effective = match trigger {
                            crate::keymap::Trigger::Key(key) => {
                                self.resolve(focus, *key, &holds) == Some(binding.dispatch)
                            }
                            crate::keymap::Trigger::Text => ['a', '#', 'é'].into_iter().any(|c| {
                                self.resolve(focus, Key::char(c), &holds) == Some(binding.dispatch)
                            }),
                            crate::keymap::Trigger::Any => {
                                self.resolve(focus, Key::ctrl('~'), &holds)
                                    == Some(binding.dispatch)
                            }
                        };
                        effective && !seen.contains(trigger)
                    })
                    .collect();
                if triggers.is_empty() {
                    continue;
                }
                seen.extend(triggers.iter().copied());
                let labels = triggers
                    .iter()
                    .map(|t| t.label())
                    .collect::<Vec<_>>()
                    .join(" ");
                let partial = triggers.len() != binding.triggers.len();
                if let Some(row) = rows.last_mut().filter(|r| r.legend == binding.legend) {
                    row.labels.push(' ');
                    row.labels.push_str(&labels);
                    row.partial |= partial;
                } else {
                    rows.push(Row {
                        labels,
                        legend: binding.legend,
                        binding,
                        partial,
                    });
                }
            }
            sections.add(layer.name, rows);
        }
        sections.into_vec()
    }
}

/// Rendering callbacks bound to the mode state that supplies their data.
pub(crate) struct BoundScreen<V, const N: usize> {
    view: V,
    metadata: &'static Screen,
    layout: fn(&V, Region) -> Vec<Region>,
    panels: [fn(&V, Rect, &mut Buffer); N],
}
impl<V, const N: usize> BoundScreen<V, N> {
    pub fn new(
        view: V,
        metadata: &'static Screen,
        layout: fn(&V, Region) -> Vec<Region>,
        panels: [fn(&V, Rect, &mut Buffer); N],
    ) -> Self {
        Self {
            view,
            metadata,
            layout,
            panels,
        }
    }
    pub fn render(self, area: Rect, buf: &mut Buffer) {
        let regions = (self.layout)(&self.view, Region::new(area));
        debug_assert_eq!(self.metadata.panels.len(), N);
        debug_assert_eq!(N, regions.len());
        for (draw, region) in self.panels.iter().zip(regions) {
            draw(&self.view, region.rect(), buf);
        }
    }
}
/// The help's sections while they are built: layers grouped by name, in the
/// order they are first added.
#[derive(Default)]
struct Sections(Vec<(&'static str, Vec<Row<'static, Action>>)>);

impl Sections {
    fn add(&mut self, name: &'static str, rows: Vec<Row<'static, Action>>) {
        if rows.is_empty() {
            return;
        }
        match self.0.iter_mut().find(|(title, _)| *title == name) {
            Some((_, all)) => all.extend(rows),
            None => self.0.push((name, rows)),
        }
    }

    fn into_vec(self) -> Vec<(&'static str, Vec<Row<'static, Action>>)> {
        self.0
    }
}

pub struct App<'a> {
    model: &'a Model,
    painter: Painter,
    now: u64,
}

impl<'a> App<'a> {
    pub fn new(model: &'a Model, painter: Painter, now: u64) -> Self {
        Self {
            model,
            painter,
            now,
        }
    }
}

impl Widget for App<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [body, bottom] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let (m, t) = (self.model, self.painter);
        match m.shown() {
            crate::model::Mode::Rename(r) => rename::RenameView::new(r, t, m.split, m.view)
                .screen()
                .render(body, buf),
            crate::model::Mode::Search => {
                search::SearchView::new(&m.search, &m.root, m.status.busy, t, m.split, m.view)
                    .screen()
                    .render(body, buf)
            }
            crate::model::Mode::Move(mv) => moving::MoveView::new(mv, t, m.split, m.view)
                .screen()
                .render(body, buf),
            crate::model::Mode::Rewrite(rw) => rewrite::RewriteView::new(rw, t, m.split, m.view)
                .screen()
                .render(body, buf),
            crate::model::Mode::History(h) => history::HistoryView::new(h, t, m.split, self.now)
                .screen()
                .render(body, buf),
        }
        if let Some(overlay) = &m.overlay {
            overlay.render(t, body, buf);
        }
        StatusBar::new(m, t).render(bottom, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::Trigger;

    /// Every layer a screen or the defaults contributes.
    struct Layers(Vec<&'static Layer<Action>>);
    impl Layers {
        fn new() -> Self {
            let mut layers = vec![
                &defaults::GLOBAL,
                &defaults::NAVIGATE,
                &defaults::DIGITS,
                &defaults::LIST,
                &defaults::TEXT,
            ];
            for screen in [
                &search::SEARCH,
                &search::INSPECT_SOURCE,
                &search::INSPECT_BODY,
                &rename::RENAME,
                &moving::MOVE,
                &rewrite::REWRITE,
                &history::HISTORY,
                &overlay::MENU_SCREEN,
                &overlay::CONFIRM_SCREEN,
                &overlay::HELP_SCREEN,
                &overlay::PLACES_SCREEN,
                &overlay::NAVIGATION_SCREEN,
            ] {
                layers.push(&screen.layer);
                for panel in screen.panels {
                    layers.push(&panel.layer);
                }
            }
            Self(layers)
        }
    }

    /// Every layer binds a key at most once per condition, and every bar
    /// label is spelled from the keys of the row it describes.
    #[test]
    fn layers_are_well_formed() {
        for layer in Layers::new().0 {
            for row in layer.rows() {
                let Some(bar) = row.legend.bar else {
                    continue;
                };
                let spelled: Vec<&str> = row.labels.split_whitespace().collect();
                let parts: Vec<&str> = if spelled.contains(&bar.keys) {
                    vec![bar.keys]
                } else {
                    bar.keys
                        .split(['/', ' '])
                        .filter(|p| !p.is_empty())
                        .collect()
                };
                for part in parts {
                    assert!(
                        spelled.contains(&part) || part == "tab",
                        "{:?} names {part:?}, which {:?} does not spell",
                        bar.keys,
                        row.labels
                    );
                }
            }
            let mut seen: Vec<(Key, When)> = Vec::new();
            for binding in layer.bindings {
                for trigger in binding.triggers {
                    let Trigger::Key(key) = trigger else {
                        continue;
                    };
                    assert!(
                        !seen.contains(&(*key, binding.when)),
                        "{key:?} is bound twice in {:?}",
                        layer.name
                    );
                    seen.push((*key, binding.when));
                }
            }
        }
    }
}
