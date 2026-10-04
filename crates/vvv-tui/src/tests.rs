//! The TUI without a terminal: `update` with plain assertions, every mode
//! rendered into a `TestBackend` and snapshotted.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::Widget;
use vvv_engine::{Answer, Intent, Request, Selection, SymbolKind};

use super::action::{Action, Effect, Event, Planned};
use super::model::{Level, MenuTarget, Mode, Model, Overlay, ReportView};
use crate::fixtures as fx;
use crate::modes::moves::MovePanel;
use crate::modes::rename::RenamePanel;
use crate::modes::rewrite::RewritePanel;
use crate::modes::search::query::Filter;
use crate::modes::search::{Relation, SearchPanel};
use crate::render::Painter;
use crate::screen::App;

// Included in this module to retain snapshot names and paths.
include!("tests_support.rs");
include!("tests_review.rs");
include!("tests_navigation.rs");
include!("tests_workspace.rs");
include!("tests_search.rs");
include!("tests_presentation.rs");
include!("tests_preferences.rs");
include!("tests_input.rs");
