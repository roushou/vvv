//! Rendering a [`Model`] into a frame. A [`Screen`] is the keys that work
//! from any of its panels, the [`Panel`]s it is made of, and how they are
//! placed; a [`Panel`] is one region — its own keys, the kind that selects
//! its shared defaults, and how it draws. Rendering is a walk of that tree.
//! Every draw is a pure function of `&Model` and a [`Painter`], so it can be
//! rendered into a `TestBackend` and snapshotted.
//!
//! The primitives a screen draws with live in [`crate::render`].

pub(crate) mod defaults;
pub(crate) mod history;
pub(crate) mod moving;
pub(crate) mod overlay;
pub(crate) mod rename;
pub(crate) mod rewrite;
pub(crate) mod search;
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
/// defaults, and how it draws.
#[derive(Debug, Clone, Copy)]
pub struct Panel {
    pub layer: Layer<Action>,
    /// `None` for a region that is drawn but never focused, such as a
    /// header or an overlay's box.
    pub kind: Option<PanelKind>,
    pub content: fn(&Model, Painter, Rect, &mut Buffer),
}

/// One screen: the keys that work from any of its panels, the panels it is
/// made of, and how they are placed.
#[derive(Debug, Clone, Copy)]
pub struct Screen {
    pub layer: Layer<Action>,
    pub panels: &'static [Panel],
    /// One rect per panel, in the same order.
    pub layout: fn(&Model, Painter, Region) -> Vec<Region>,
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
            && let Some(dispatch) = defaults::default_for(kind).and_then(|l| l.resolve(key, &holds))
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

    /// Draw every panel of the screen.
    pub fn render(&self, model: &Model, painter: Painter, area: Rect, buf: &mut Buffer) {
        let regions = (self.layout)(model, painter, Region::new(area));
        debug_assert_eq!(
            self.panels.len(),
            regions.len(),
            "{} laid out {} panels into {} regions",
            self.layer.name,
            self.panels.len(),
            regions.len(),
        );
        for (panel, region) in self.panels.iter().zip(regions) {
            (panel.content)(model, painter, region.rect(), buf);
        }
    }

    /// The help's sections for a focus: the panel, the screen, the kind
    /// default and the globals, grouped by name.
    pub fn sections(&self, focus: usize) -> Vec<(&'static str, Vec<Row<'static, Action>>)> {
        let mut sections = Sections::default();
        if let Some(panel) = self.panel(focus) {
            sections.add(panel.layer);
            sections.add(self.layer);
            if let Some(kind) = panel.kind
                && let Some(layer) = defaults::default_for(kind)
            {
                sections.add(*layer);
            }
            sections.add(defaults::NAVIGATE);
            if panel.kind != Some(PanelKind::Input) {
                sections.add(defaults::DIGITS);
            }
        } else {
            sections.add(self.layer);
        }
        sections.add(defaults::GLOBAL);
        sections.into_vec()
    }

    /// The status bar's rows for a focus, most specific first.
    pub fn rows(&self, focus: usize) -> Vec<Row<'static, Action>> {
        let mut rows: Vec<Row<'static, Action>> = Vec::new();
        if let Some(panel) = self.panel(focus) {
            rows.extend(panel.layer.rows());
            rows.extend(self.layer.rows());
            if let Some(kind) = panel.kind
                && let Some(layer) = defaults::default_for(kind)
            {
                rows.extend(layer.rows());
            }
            rows.extend(defaults::NAVIGATE.rows());
            if panel.kind != Some(PanelKind::Input) {
                rows.extend(defaults::DIGITS.rows());
            }
        } else {
            rows.extend(self.layer.rows());
        }
        rows.extend(defaults::GLOBAL.rows());
        rows
    }
}

/// The help's sections while they are built: layers grouped by name, in the
/// order they are first added.
#[derive(Default)]
struct Sections(Vec<(&'static str, Vec<Row<'static, Action>>)>);

impl Sections {
    fn add(&mut self, layer: Layer<Action>) {
        let rows = layer.rows();
        if rows.is_empty() {
            return;
        }
        match self.0.iter_mut().find(|(name, _)| *name == layer.name) {
            Some((_, all)) => all.extend(rows),
            None => self.0.push((layer.name, rows)),
        }
    }

    fn into_vec(self) -> Vec<(&'static str, Vec<Row<'static, Action>>)> {
        self.0
    }
}

pub struct App<'a> {
    model: &'a Model,
    painter: Painter,
}

impl<'a> App<'a> {
    pub fn new(model: &'a Model, painter: Painter) -> Self {
        Self { model, painter }
    }
}

impl Widget for App<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [body, bottom] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let (m, t) = (self.model, self.painter);
        m.mode_screen().render(m, t, body, buf);
        StatusBar::new(m, t).render(bottom, buf);
        if let Some(overlay) = m.overlay_screen() {
            overlay.render(m, t, area, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::Trigger;

    /// Every layer a screen or the defaults contributes.
    fn layers() -> Vec<&'static Layer<Action>> {
        let mut layers = vec![
            &defaults::GLOBAL,
            &defaults::NAVIGATE,
            &defaults::DIGITS,
            &defaults::LIST,
            &defaults::TEXT,
        ];
        for screen in [
            &search::SEARCH,
            &rename::RENAME,
            &moving::MOVE,
            &rewrite::REWRITE,
            &history::HISTORY,
            &overlay::MENU_SCREEN,
            &overlay::CONFIRM_SCREEN,
            &overlay::HELP_SCREEN,
        ] {
            layers.push(&screen.layer);
            for panel in screen.panels {
                layers.push(&panel.layer);
            }
        }
        layers
    }

    /// Every layer binds a key at most once per condition, and every bar
    /// label is spelled from the keys of the row it describes.
    #[test]
    fn layers_are_well_formed() {
        for layer in layers() {
            for row in layer.rows() {
                let Some(bar) = row.legend.bar else {
                    continue;
                };
                let spelled: Vec<&str> = row.spelled.split_whitespace().collect();
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
                        row.spelled
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
