//! UI-independent document model for Pilcrow: text buffer, encodings, line endings, undo.

pub mod checksum;
pub mod code;
pub mod diff;
mod document;
mod encoding;
pub mod export;
pub mod files;
pub mod fold;
pub mod highlight;
mod history;
mod line_ending;
pub mod ops;
mod search;
pub mod transform;

pub use document::{DiskStatus, Document, SaveError};
pub use encoding::{CP437, CP850, CP858, EncodeError, Encoding, OemCodePage, Script};
pub use line_ending::LineEnding;
pub use ropey::{Rope, RopeSlice};
pub use search::{Search, SearchError, SearchQuery, match_index, next_match, prev_match};
