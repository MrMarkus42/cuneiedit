//! User-defined languages, stored in `languages.ini` in the config folder.

use std::{fs, io};

use pilcrow_core::highlight::UserLanguage;

use crate::settings::{config_file, write_atomically};

const FILE: &str = "languages.ini";
const MAX_LANGUAGES: usize = 100;
const MAX_WORDS: usize = 5_000;

pub fn load() -> Vec<UserLanguage> {
    config_file(FILE)
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|text| parse(&text))
        .unwrap_or_default()
}

pub fn save(languages: &[UserLanguage]) -> io::Result<()> {
    match config_file(FILE) {
        Some(path) => write_atomically(&path, &serialize(languages)),
        None => Ok(()),
    }
}

/// Space-separated words, as typed in the dialog.
pub fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .take(MAX_WORDS)
        .map(str::to_owned)
        .collect()
}

fn parse(text: &str) -> Vec<UserLanguage> {
    let mut languages: Vec<UserLanguage> = Vec::new();
    for line in text.lines() {
        if line == "[language]" {
            if languages.len() >= MAX_LANGUAGES {
                break;
            }
            languages.push(UserLanguage::default());
            continue;
        }
        let (Some(language), Some((key, value))) = (languages.last_mut(), line.split_once('='))
        else {
            continue;
        };
        match key {
            "name" => language.name = value.trim().to_owned(),
            "extensions" => language.extensions = words(value),
            "keywords" => language.keywords = words(value),
            "types" => language.types = words(value),
            "line_comment" => language.line_comment = value.trim().to_owned(),
            "block_comment" => {
                language.block_comment = value
                    .split_once(' ')
                    .map(|(open, close)| (open.trim().to_owned(), close.trim().to_owned()));
            }
            "quotes" => language.quotes = value.chars().filter(|c| !c.is_whitespace()).collect(),
            "ignore_case" => language.ignore_case = value == "true",
            _ => {}
        }
    }
    languages.retain(|language| !language.name.is_empty());
    languages
}

fn serialize(languages: &[UserLanguage]) -> String {
    let clean = |text: &str| text.replace(['\n', '\r'], " ");
    let mut out = String::new();
    for language in languages {
        out.push_str("[language]\n");
        out.push_str(&format!("name={}\n", clean(&language.name)));
        out.push_str(&format!(
            "extensions={}\n",
            clean(&language.extensions.join(" "))
        ));
        out.push_str(&format!(
            "keywords={}\n",
            clean(&language.keywords.join(" "))
        ));
        out.push_str(&format!("types={}\n", clean(&language.types.join(" "))));
        out.push_str(&format!("line_comment={}\n", clean(&language.line_comment)));
        if let Some((open, close)) = &language.block_comment {
            out.push_str(&format!("block_comment={} {}\n", clean(open), clean(close)));
        }
        out.push_str(&format!("quotes={}\n", clean(&language.quotes)));
        out.push_str(&format!("ignore_case={}\n", language.ignore_case));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let languages = vec![UserLanguage {
            name: "Mini".into(),
            extensions: vec!["mini".into(), "mn".into()],
            keywords: vec!["if".into(), "then".into()],
            types: vec!["Int".into()],
            line_comment: "--".into(),
            block_comment: Some(("{-".into(), "-}".into())),
            quotes: "\"'".into(),
            ignore_case: true,
        }];
        assert_eq!(parse(&serialize(&languages)), languages);
        assert!(parse("name=orphan\n[language]\nkeywords=x\n").is_empty());
    }
}
