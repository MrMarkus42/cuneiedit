//! Text editor widget for egui, backed by a [`pilcrow_core::Document`].

mod movement;
mod rows;
mod state;
mod widget;

pub use state::{EditorState, Repeat};
pub use widget::{Editor, Palette};
