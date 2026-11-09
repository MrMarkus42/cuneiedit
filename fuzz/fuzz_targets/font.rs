//! Reading an arbitrary font file (as chosen in the settings) and embedding it in a PDF.
#![no_main]

use libfuzzer_sys::fuzz_target;
use cuneiedit_core::Document;
use cuneiedit_core::export;
use cuneiedit_core::font::TrueType;
use cuneiedit_core::highlight::Language;

fuzz_target!(|data: &[u8]| {
    for index in 0..2 {
        let Some(font) = TrueType::parse(data, index) else {
            continue;
        };
        for c in ['A', 'z', '€', 'ß', '中', '😀', '\u{FFFF}', '\u{10FFFF}'] {
            if let Some(glyph) = font.glyph(c) {
                font.advance(glyph);
            }
        }
        let mut doc = Document::new();
        doc.replace(0..0, "Aß€\n中😀");
        let colors = |_| [0, 0, 0];
        export::pdf(&doc, Language::PLAIN_TEXT, "fuzz", 4, &colors, &[font]);
    }
});
