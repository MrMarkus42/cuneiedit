//! Opening an arbitrary file: encoding detection, line endings, folding, brackets and tags,
//! statistics, saving back and comparing with an edited copy.
#![no_main]

use libfuzzer_sys::fuzz_target;
use cuneiedit_core::diff::{DiffOptions, compare};
use cuneiedit_core::{Document, LineEnding, fold, transform};

fuzz_target!(|data: &[u8]| {
    let doc = Document::from_bytes(data.to_vec());
    let rope = doc.text();
    let len = rope.len_chars();
    for line in 0..rope.len_lines().min(64) {
        fold::is_header(rope, line, 4);
        if let Some(range) = fold::fold_range(rope, line, 4) {
            assert!(range.start > line && range.end <= rope.len_lines());
        }
        fold::enclosing_header(rope, line, 4);
    }
    for pos in [0, len / 3, len / 2, len.saturating_sub(1), len] {
        if let Some((a, b)) = transform::matching_bracket(rope, pos) {
            assert!(a < len && b < len);
        }
        transform::enclosing_brackets(rope, pos);
        transform::matching_tags(rope, pos);
    }
    transform::summary(&doc);
    // Saving and reading back in the same encoding keeps the text (line endings unified).
    if let Ok(bytes) = doc.to_bytes() {
        let decoded = doc.encoding().decode(&bytes);
        assert_eq!(LineEnding::normalize(&decoded), rope.to_string());
    }
    let mut edited = Document::from_bytes(data.to_vec());
    edited.replace(len / 2..len / 2, "x\ny");
    compare(&doc, &edited, DiffOptions::default());
});
