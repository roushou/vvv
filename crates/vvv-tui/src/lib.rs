//! The interactive interface of `vvv`: modes of panels — search as the hub,
//! then rename, move, rewrite and history, each with its own layout and keys.
//!
//! The surface is [`Tui`]: give it an [`Engine`](vvv_engine::Engine), say which editor `e` opens
//! and whether to colour, and [`Tui::run`] takes the terminal until the user
//! quits. Everything else is private and Elm-shaped so it can be tested
//! without a terminal: `Model` is plain data, `Model::update` is pure and
//! returns the `Effect`s it wants run, `Worker` runs them against the engine
//! on its own thread and answers with `Event`s. Mode views render their state
//! through typed screen callbacks; shared views lay out report rows.
//! Only [`Tui::run`] touches the terminal — and the editor.

mod action;
mod error;
mod input;
mod keymap;
mod model;
mod modes;
mod overlays;
mod preferences;
mod render;
mod screen;
mod tui;
mod view;
mod worker;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod performance;
#[cfg(test)]
mod tests;

pub use error::Error;
pub use tui::Tui;
