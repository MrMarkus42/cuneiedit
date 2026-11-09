//! Highlighting arbitrary text in every language, line by line as the editor does, and
//! exporting it as HTML and PDF.
#![no_main]

use libfuzzer_sys::fuzz_target;
use cuneiedit_core::Document;
use cuneiedit_core::export;
use cuneiedit_core::highlight::{Highlighter, Language, LineState, highlight_line};

fuzz_target!(|data: &[u8]| {
    let Some((&pick, rest)) = data.split_first() else {
        return;
    };
    let languages: Vec<Language> = Language::all().collect();
    let language = languages[usize::from(pick) % languages.len()];
    let text = String::from_utf8_lossy(rest);

    let mut state = LineState::Normal;
    for line in text.split('\n') {
        let mut tokens = Vec::new();
        let end = highlight_line(language, line, state, Some(&mut tokens));
        assert_eq!(
            end,
            highlight_line(language, line, state, None),
            "state without tokens"
        );
        let len = line.chars().count();
        let mut previous_end = 0;
        for token in &tokens {
            assert!(
                token.start >= previous_end,
                "tokens are sorted and disjoint: {tokens:?}"
            );
            assert!(
                token.start < token.end && token.end <= len,
                "{token:?} in {len}"
            );
            previous_end = token.end;
        }
        state = end;
    }

    let mut doc = Document::new();
    doc.replace(0..0, &text);
    let mut highlighter = Highlighter::new(language);
    let last = doc.text().len_lines() - 1;
    highlighter.prepare(&doc, last, usize::MAX);
    highlighter.tokens_in(&doc, last, 0..80);
    let colors = |_| [0, 0, 0];
    export::html(&doc, language, "fuzz", &colors, [255; 3]);
    if text.len() < 4096 {
        export::pdf(&doc, language, "fuzz", 4, &colors, &[]);
    }
});
