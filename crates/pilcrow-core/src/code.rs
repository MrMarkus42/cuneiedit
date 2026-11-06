//! Lightweight code structure: symbols for a function list, word completion and
//! line-based search results.

use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

use crate::Document;
use crate::highlight::Language;

/// A named definition, e.g. a function or class, at a zero-based line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub line: usize,
}

struct SymbolPatterns {
    languages: &'static [&'static str],
    pattern: &'static str,
}

/// One pattern per language family; group `name` is the symbol name.
const PATTERNS: &[SymbolPatterns] = &[
    SymbolPatterns {
        languages: &["Rust"],
        pattern: r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:(?:async|const|unsafe|extern\s+\S+)\s+)*(?:fn|struct|enum|trait|impl(?:<[^>]*>)?|mod|type|macro_rules!)\s+(?P<name>[A-Za-z_][\w:<>, ]*?)\s*(?:[<({;=!]|where|for|$)",
    },
    SymbolPatterns {
        languages: &["Python"],
        pattern: r"^\s*(?:async\s+)?(?:def|class)\s+(?P<name>\w+)",
    },
    SymbolPatterns {
        languages: &["JavaScript", "TypeScript"],
        pattern: r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?(?:function\*?\s+(?P<name>\w+)|class\s+(?P<class>\w+)|(?:const|let|var)\s+(?P<arrow>\w+)\s*=\s*(?:async\s*)?\([^)]*\)\s*=>)",
    },
    SymbolPatterns {
        languages: &["Go"],
        pattern: r"^\s*(?:func\s+(?:\([^)]*\)\s*)?(?P<name>\w+)|type\s+(?P<class>\w+)\s+(?:struct|interface))",
    },
    SymbolPatterns {
        languages: &["C", "C++", "C#", "Java", "Kotlin", "Swift", "PHP"],
        pattern: r"^\s*(?:(?:public|private|protected|internal|static|final|abstract|virtual|override|inline|async|open|fun|func|function|extern|const|unsafe|sealed|partial)\s+)*(?:(?:class|struct|interface|enum|record|namespace|object|protocol|extension)\s+(?P<class>\w+)|(?:[\w:<>\[\],*&?]+\s+)*(?P<name>[A-Za-z_]\w*)\s*\([^;]*$)",
    },
    SymbolPatterns {
        languages: &["Ruby", "Lua"],
        pattern: r"^\s*(?:local\s+)?(?:def|function|class|module)\s+(?P<name>[\w.:]+)",
    },
    SymbolPatterns {
        languages: &["Shell"],
        pattern: r"^\s*(?:function\s+)?(?P<name>[A-Za-z_][\w-]*)\s*\(\)\s*\{?",
    },
    SymbolPatterns {
        languages: &["PowerShell"],
        pattern: r"(?i)^\s*function\s+(?P<name>[\w-]+)",
    },
    SymbolPatterns {
        languages: &["Markdown"],
        pattern: r"^(?P<name>#{1,6}\s+.+)$",
    },
    SymbolPatterns {
        languages: &["INI", "TOML"],
        pattern: r"^\s*(?P<name>\[[^\]]+\])",
    },
    SymbolPatterns {
        languages: &["SQL"],
        pattern: r"(?i)^\s*create\s+(?:or\s+replace\s+)?(?:table|view|function|procedure|index|trigger)\s+(?:if\s+not\s+exists\s+)?(?P<name>[\w.\x22`\[\]]+)",
    },
];

/// Words that are control flow, not definitions, but look like C-style function heads.
const NOT_FUNCTIONS: &[&str] = &[
    "if", "for", "while", "switch", "catch", "return", "else", "do", "sizeof", "new", "using",
];

static COMPILED: LazyLock<Vec<(&'static [&'static str], Regex)>> = LazyLock::new(|| {
    PATTERNS
        .iter()
        .map(|p| {
            (
                p.languages,
                Regex::new(p.pattern).expect("valid symbol pattern"),
            )
        })
        .collect()
});

/// Definitions in `doc`, for languages that have a pattern.
pub fn symbols(doc: &Document, language: Language) -> Vec<Symbol> {
    let Some((_, regex)) = COMPILED
        .iter()
        .find(|(languages, _)| languages.contains(&language.name()))
    else {
        return Vec::new();
    };
    let rope = doc.text();
    let mut symbols = Vec::new();
    for (line, slice) in rope.lines().enumerate() {
        let text = slice.to_string();
        let Some(captures) = regex.captures(text.trim_end_matches('\n')) else {
            continue;
        };
        let name = ["name", "class", "arrow"]
            .into_iter()
            .find_map(|group| captures.name(group))
            .map(|m| m.as_str().trim().to_owned());
        if let Some(name) = name.filter(|n| !n.is_empty() && !NOT_FUNCTIONS.contains(&n.as_str())) {
            symbols.push(Symbol { name, line });
        }
    }
    symbols
}

/// The definition line of `name`, e.g. to show the parameters of a function being called.
pub fn signature(doc: &Document, language: Language, name: &str) -> Option<String> {
    /// Longer definitions are cut.
    const MAX_CHARS: usize = 200;
    let symbol = symbols(doc, language)
        .into_iter()
        .find(|symbol| symbol.name == name)?;
    let line = doc.text().line(symbol.line).to_string();
    let line = line.trim();
    let line = line.trim_end_matches(['{', ':']).trim_end();
    Some(line.chars().take(MAX_CHARS).collect())
}

/// Words in `doc` and keywords of `language` that start with `prefix` (case-insensitive),
/// excluding `prefix` itself. At most `limit`, sorted.
pub fn completions(doc: &Document, language: Language, prefix: &str, limit: usize) -> Vec<String> {
    /// Only this much text is scanned, so huge files stay responsive.
    const MAX_SCAN_CHARS: usize = 4_000_000;
    let lower = prefix.to_lowercase();
    let matches = |word: &str| word.len() > prefix.len() && word.to_lowercase().starts_with(&lower);
    let mut words = BTreeSet::new();
    let mut current = String::new();
    for c in doc.text().chars().take(MAX_SCAN_CHARS).chain([' ']) {
        if c.is_alphanumeric() || c == '_' {
            current.push(c);
        } else if !current.is_empty() {
            if matches(&current) {
                words.insert(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    words.extend(
        language
            .keywords()
            .filter(|k| matches(k))
            .map(str::to_owned),
    );
    words.into_iter().take(limit).collect()
}

/// A line containing matches: zero-based line number, its text and the matched char ranges
/// within the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineHits {
    pub line: usize,
    pub text: String,
    pub hits: Vec<Range<usize>>,
}

/// Groups char-range matches of `doc` by line, e.g. for a results list.
pub fn hits_by_line(doc: &Document, matches: &[Range<usize>]) -> Vec<LineHits> {
    /// Long lines are cut in the results list.
    const MAX_LINE_CHARS: usize = 400;
    let rope = doc.text();
    let mut lines: Vec<LineHits> = Vec::new();
    for found in matches {
        let line = rope.char_to_line(found.start);
        let start = rope.line_to_char(line);
        let hit = found.start - start..found.end.min(start + rope.line(line).len_chars()) - start;
        match lines.last_mut() {
            Some(last) if last.line == line => last.hits.push(hit),
            _ => {
                let text: String = rope
                    .line(line)
                    .chars()
                    .take_while(|&c| c != '\n')
                    .take(MAX_LINE_CHARS)
                    .collect();
                lines.push(LineHits {
                    line,
                    text,
                    hits: vec![hit],
                });
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        let mut doc = Document::new();
        doc.replace(0..0, text);
        doc
    }

    fn names(text: &str, language: &str) -> Vec<(String, usize)> {
        symbols(&doc(text), Language::from_name(language).unwrap())
            .into_iter()
            .map(|s| (s.name, s.line))
            .collect()
    }

    #[test]
    fn finds_symbols() {
        assert_eq!(
            names(
                "pub fn main() {\n    let x = 1;\n}\nstruct Point<T> {\nimpl Foo for Bar {",
                "Rust"
            ),
            vec![("main".into(), 0), ("Point".into(), 3), ("Foo".into(), 4)]
        );
        assert_eq!(
            names("class A:\n    def run(self):", "Python"),
            vec![("A".into(), 0), ("run".into(), 1)]
        );
        assert_eq!(
            names("int main(int argc) {\n  if (x) {\n}\nclass Widget {", "C++"),
            vec![("main".into(), 0), ("Widget".into(), 3)]
        );
        assert_eq!(
            names(
                "export async function load() {}\nconst f = (a) => a",
                "JavaScript"
            ),
            vec![("load".into(), 0), ("f".into(), 1)]
        );
        assert_eq!(
            names("# Title\ntext\n## Sub", "Markdown"),
            vec![("# Title".into(), 0), ("## Sub".into(), 2)]
        );
        assert!(names("anything", "Normal text").is_empty());
    }

    #[test]
    fn finds_signatures() {
        let doc = doc("fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n");
        let rust = Language::from_name("Rust").unwrap();
        assert_eq!(
            signature(&doc, rust, "add").as_deref(),
            Some("fn add(a: i32, b: i32) -> i32")
        );
        assert_eq!(signature(&doc, rust, "sub"), None);
    }

    #[test]
    fn completes_words_and_keywords() {
        let doc = doc("let value = valid + Variable; va");
        let rust = Language::from_name("Rust").unwrap();
        assert_eq!(
            completions(&doc, rust, "va", 10),
            vec!["Variable", "valid", "value"]
        );
        assert_eq!(completions(&doc, rust, "wh", 10), vec!["where", "while"]);
        assert_eq!(completions(&doc, rust, "va", 1).len(), 1);
    }

    #[test]
    #[expect(clippy::single_range_in_vec_init, reason = "one hit is a range")]
    fn groups_hits_by_line() {
        let doc = doc("ab ab\ncd\nab");
        let hits = hits_by_line(&doc, &[0..2, 3..5, 9..11]);
        assert_eq!(
            hits,
            vec![
                LineHits {
                    line: 0,
                    text: "ab ab".into(),
                    hits: vec![0..2, 3..5],
                },
                LineHits {
                    line: 2,
                    text: "ab".into(),
                    hits: vec![0..2],
                },
            ]
        );
    }
}
