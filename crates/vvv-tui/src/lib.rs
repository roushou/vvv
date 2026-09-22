//! The interactive interface of `vvv`: modes of panels — search as the hub,
//! then rename, move, rewrite and history, each with its own layout and keys.
//!
//! The surface is [`Tui`]: give it an [`Engine`](vvv_engine::Engine), say which editor `e` opens
//! and whether to colour, and [`Tui::run`] takes the terminal until the user
//! quits. Everything else is private and Elm-shaped so it can be tested
//! without a terminal: `Model` is plain data, `Model::update` is pure and
//! returns the `Effect`s it wants run, `Worker` runs them against the engine
//! on its own thread and answers with `Event`s, and `view` renders a model
//! into a frame. Only [`Tui::run`] touches the terminal — and the editor.

mod action;
mod error;
mod keymap;
mod model;
mod query;
mod render;
mod screen;
mod tui;
mod update;
mod view;
mod worker;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;

pub use error::Error;
pub use tui::Tui;
