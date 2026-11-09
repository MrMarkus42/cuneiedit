//! Lightweight, dependency-free syntax highlighting.
//!
//! Each line is lexed on its own, starting from the state the previous line ended in
//! (e.g. inside a block comment). [`Highlighter`] caches these start states per line.

use std::borrow::Cow;
use std::ops::Range;
use std::path::Path;
use std::sync::{PoisonError, RwLock};

use ropey::Rope;

use crate::Document;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Keyword,
    Type,
    String,
    Number,
    Comment,
    /// Preprocessor lines, attributes, decorators, variables, markup tags.
    Meta,
    /// Called or defined functions, macros.
    Function,
    /// Escape sequences in strings.
    Escape,
}

/// A highlighted span; `start` and `end` are char columns within the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

/// Lexer state at the start of a line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineState {
    #[default]
    Normal,
    BlockComment,
    String {
        quote: char,
        triple: bool,
    },
    /// Inside a Rust raw string closed by `"` and this many `#`.
    RawString {
        hashes: u8,
    },
    /// Inside a markup tag, between its name and `>`.
    Tag,
    /// Inside an HTML `<script` (or `<style`, if `css`) tag; its content follows `>`.
    EmbedTag {
        css: bool,
    },
    /// Inside the content of an HTML `<script>` or `<style>` element.
    Embedded {
        css: bool,
        inner: InnerState,
    },
    /// Inside a fenced Markdown code block.
    Fence,
}

/// State of the JavaScript or CSS lexer inside HTML.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InnerState {
    #[default]
    Normal,
    BlockComment,
    String {
        quote: char,
    },
}

impl InnerState {
    fn line_state(self) -> LineState {
        match self {
            Self::Normal => LineState::Normal,
            Self::BlockComment => LineState::BlockComment,
            Self::String { quote } => LineState::String {
                quote,
                triple: false,
            },
        }
    }

    fn from_line_state(state: LineState) -> Self {
        match state {
            LineState::BlockComment => Self::BlockComment,
            LineState::String { quote, .. } => Self::String { quote },
            _ => Self::Normal,
        }
    }
}

/// Keywords after which a name is a function definition.
const DEFINERS: &[&str] = &[
    "def",
    "defmacro",
    "defn",
    "defp",
    "defun",
    "fn",
    "fun",
    "func",
    "function",
    "macro",
    "method",
    "proc",
    "procedure",
    "sub",
    "subroutine",
];

/// Keywords after which `/` starts a regular expression.
const BEFORE_REGEX: &[&str] = &[
    "return", "typeof", "case", "do", "else", "in", "of", "new", "delete", "void", "throw",
    "yield", "await",
];

/// How far the lexer looks ahead for the end of a regex, interpolation or Markdown span,
/// so pathological lines stay linear.
const MAX_SPAN: usize = 1_000;

/// Lines longer than this (in chars) are highlighted only around the shown columns.
pub const HUGE_LINE: usize = 20_000;
/// Chars lexed before and after the shown columns of a huge line.
const HUGE_LINE_MARGIN: usize = 2_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Code,
    Markup,
    Markdown,
    Diff,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Keys {
    None,
    /// `key = value` at the start of a line.
    BeforeEquals,
    /// `key: value` at the start of a line.
    BeforeColon,
    /// `"key": value` anywhere.
    Quoted,
}

struct Spec {
    name: &'static str,
    extensions: &'static [&'static str],
    file_names: &'static [&'static str],
    mode: Mode,
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    quotes: &'static [char],
    multiline_quotes: &'static [char],
    triple_quotes: bool,
    escapes: bool,
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    ignore_case: bool,
    /// Lines starting with this char are meta, e.g. `#include` or `#[derive]`.
    line_meta: Option<char>,
    /// Words starting with one of these chars are meta, e.g. `@decorator`, `$var`.
    meta_prefixes: &'static [char],
    /// Additional identifier chars besides alphanumerics and `_`.
    ident_extra: &'static [char],
    lifetimes: bool,
    keys: Keys,
    section_headers: bool,
    /// Capitalized names like `HashMap` are types.
    pascal_types: bool,
    /// Quotes in which `$name` and `${…}` are interpolated.
    interpolation: &'static [char],
    /// `/…/` regular expression literals (JavaScript).
    regex_literals: bool,
    /// Rust: `r#"…"#` raw strings and `name!(…)` macros.
    rust_syntax: bool,
    /// HTML: `<script>` and `<style>` contents are lexed as JavaScript and CSS.
    embeds: bool,
}

const BASE: Spec = Spec {
    name: "",
    extensions: &[],
    file_names: &[],
    mode: Mode::Code,
    line_comments: &[],
    block_comment: None,
    quotes: &['"', '\''],
    multiline_quotes: &[],
    triple_quotes: false,
    escapes: true,
    keywords: &[],
    types: &[],
    ignore_case: false,
    line_meta: None,
    meta_prefixes: &[],
    ident_extra: &[],
    lifetimes: false,
    keys: Keys::None,
    section_headers: false,
    pascal_types: false,
    interpolation: &[],
    regex_literals: false,
    rust_syntax: false,
    embeds: false,
};

const C_COMMENTS: Spec = Spec {
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    ..BASE
};

const C_TYPES: &[&str] = &[
    "bool", "char", "double", "float", "int", "long", "short", "signed", "unsigned", "void",
    "size_t", "int8_t", "int16_t", "int32_t", "int64_t", "uint8_t", "uint16_t", "uint32_t",
    "uint64_t", "true", "false", "NULL", "nullptr",
];
const C_KEYWORDS: &[&str] = &[
    "auto", "break", "case", "const", "continue", "default", "do", "else", "enum", "extern", "for",
    "goto", "if", "inline", "register", "restrict", "return", "sizeof", "static", "struct",
    "switch", "typedef", "union", "volatile", "while",
];
const CPP_KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "auto",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "consteval",
    "constexpr",
    "const_cast",
    "continue",
    "co_await",
    "co_return",
    "co_yield",
    "decltype",
    "default",
    "delete",
    "do",
    "dynamic_cast",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "final",
    "for",
    "friend",
    "goto",
    "if",
    "inline",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "operator",
    "override",
    "private",
    "protected",
    "public",
    "reinterpret_cast",
    "return",
    "sizeof",
    "static",
    "static_assert",
    "static_cast",
    "struct",
    "switch",
    "template",
    "this",
    "throw",
    "try",
    "typedef",
    "typeid",
    "typename",
    "union",
    "using",
    "virtual",
    "volatile",
    "while",
];
const JS_KEYWORDS: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "of",
    "return",
    "set",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];
const JS_TYPES: &[&str] = &[
    "true",
    "false",
    "null",
    "undefined",
    "NaN",
    "Infinity",
    "Array",
    "Boolean",
    "Date",
    "Error",
    "Map",
    "Number",
    "Object",
    "Promise",
    "RegExp",
    "Set",
    "String",
    "Symbol",
];
const TS_KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "declare",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "implements",
    "import",
    "in",
    "infer",
    "instanceof",
    "interface",
    "is",
    "keyof",
    "let",
    "module",
    "namespace",
    "new",
    "of",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "satisfies",
    "set",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "type",
    "typeof",
    "var",
    "void",
    "while",
    "yield",
];
const TS_TYPES: &[&str] = &[
    "any",
    "bigint",
    "boolean",
    "never",
    "number",
    "object",
    "string",
    "symbol",
    "unknown",
    "true",
    "false",
    "null",
    "undefined",
    "Array",
    "Map",
    "Promise",
    "Record",
    "Set",
    "Partial",
    "Readonly",
];
const SHELL_KEYWORDS: &[&str] = &[
    "case", "do", "done", "elif", "else", "esac", "exit", "export", "fi", "for", "function", "if",
    "in", "local", "readonly", "return", "select", "set", "shift", "source", "then", "unset",
    "until", "while", "echo", "cd", "test",
];
const SQL_KEYWORDS: &[&str] = &[
    "add",
    "all",
    "alter",
    "and",
    "as",
    "asc",
    "begin",
    "between",
    "by",
    "case",
    "check",
    "column",
    "commit",
    "constraint",
    "create",
    "cross",
    "database",
    "default",
    "delete",
    "desc",
    "distinct",
    "drop",
    "else",
    "end",
    "exists",
    "foreign",
    "from",
    "full",
    "group",
    "having",
    "if",
    "in",
    "index",
    "inner",
    "insert",
    "into",
    "is",
    "join",
    "key",
    "left",
    "like",
    "limit",
    "not",
    "null",
    "offset",
    "on",
    "or",
    "order",
    "outer",
    "primary",
    "references",
    "replace",
    "right",
    "rollback",
    "select",
    "set",
    "table",
    "then",
    "top",
    "transaction",
    "truncate",
    "union",
    "unique",
    "update",
    "values",
    "view",
    "when",
    "where",
    "with",
];
const SQL_TYPES: &[&str] = &[
    "bigint",
    "binary",
    "bit",
    "blob",
    "boolean",
    "char",
    "date",
    "datetime",
    "decimal",
    "double",
    "float",
    "int",
    "integer",
    "json",
    "nchar",
    "numeric",
    "nvarchar",
    "real",
    "smallint",
    "text",
    "time",
    "timestamp",
    "tinyint",
    "uuid",
    "varbinary",
    "varchar",
    "true",
    "false",
];

const SPECS: &[Spec] = &[
    Spec {
        name: "Normal text",
        quotes: &[],
        ..BASE
    },
    Spec {
        name: "Rust",
        extensions: &["rs"],
        keywords: &[
            "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
            "extern", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
            "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait",
            "type", "unsafe", "use", "where", "while", "yield",
        ],
        types: &[
            "bool", "char", "f32", "f64", "i8", "i16", "i32", "i64", "i128", "isize", "str", "u8",
            "u16", "u32", "u64", "u128", "usize", "true", "false", "Box", "Option", "Result",
            "Some", "None", "Ok", "Err", "String", "Vec",
        ],
        line_meta: Some('#'),
        lifetimes: true,
        multiline_quotes: &['"'],
        pascal_types: true,
        rust_syntax: true,
        ..C_COMMENTS
    },
    Spec {
        name: "C",
        extensions: &["c", "h"],
        keywords: C_KEYWORDS,
        types: C_TYPES,
        line_meta: Some('#'),
        ..C_COMMENTS
    },
    Spec {
        name: "C++",
        extensions: &[
            "cpp", "cc", "cxx", "c++", "hpp", "hh", "hxx", "h++", "ipp", "inl",
        ],
        keywords: CPP_KEYWORDS,
        types: C_TYPES,
        line_meta: Some('#'),
        ..C_COMMENTS
    },
    Spec {
        name: "C#",
        extensions: &["cs", "csx"],
        keywords: &[
            "abstract",
            "as",
            "async",
            "await",
            "base",
            "break",
            "case",
            "catch",
            "checked",
            "class",
            "const",
            "continue",
            "default",
            "delegate",
            "do",
            "else",
            "enum",
            "event",
            "explicit",
            "extern",
            "finally",
            "fixed",
            "for",
            "foreach",
            "get",
            "goto",
            "if",
            "implicit",
            "in",
            "init",
            "interface",
            "internal",
            "is",
            "lock",
            "namespace",
            "new",
            "operator",
            "out",
            "override",
            "params",
            "partial",
            "private",
            "protected",
            "public",
            "readonly",
            "record",
            "ref",
            "return",
            "sealed",
            "set",
            "sizeof",
            "static",
            "struct",
            "switch",
            "this",
            "throw",
            "try",
            "typeof",
            "using",
            "var",
            "virtual",
            "volatile",
            "when",
            "where",
            "while",
            "yield",
        ],
        types: &[
            "bool", "byte", "char", "decimal", "double", "dynamic", "float", "int", "long",
            "object", "sbyte", "short", "string", "uint", "ulong", "ushort", "void", "true",
            "false", "null",
        ],
        line_meta: Some('#'),
        meta_prefixes: &['@'],
        pascal_types: true,
        ..C_COMMENTS
    },
    Spec {
        name: "Java",
        extensions: &["java"],
        keywords: &[
            "abstract",
            "assert",
            "break",
            "case",
            "catch",
            "class",
            "const",
            "continue",
            "default",
            "do",
            "else",
            "enum",
            "extends",
            "final",
            "finally",
            "for",
            "goto",
            "if",
            "implements",
            "import",
            "instanceof",
            "interface",
            "native",
            "new",
            "package",
            "permits",
            "private",
            "protected",
            "public",
            "record",
            "return",
            "sealed",
            "static",
            "super",
            "switch",
            "synchronized",
            "this",
            "throw",
            "throws",
            "transient",
            "try",
            "var",
            "void",
            "volatile",
            "while",
            "yield",
        ],
        types: &[
            "boolean", "byte", "char", "double", "float", "int", "long", "short", "true", "false",
            "null", "String", "Object", "Integer", "List", "Map",
        ],
        meta_prefixes: &['@'],
        pascal_types: true,
        ..C_COMMENTS
    },
    Spec {
        name: "Kotlin",
        extensions: &["kt", "kts"],
        keywords: &[
            "as",
            "break",
            "by",
            "class",
            "companion",
            "const",
            "continue",
            "data",
            "do",
            "else",
            "enum",
            "for",
            "fun",
            "if",
            "import",
            "in",
            "init",
            "interface",
            "internal",
            "is",
            "lateinit",
            "object",
            "open",
            "override",
            "package",
            "private",
            "protected",
            "public",
            "return",
            "sealed",
            "super",
            "suspend",
            "this",
            "throw",
            "try",
            "typealias",
            "val",
            "var",
            "when",
            "while",
        ],
        types: &[
            "Any", "Boolean", "Byte", "Char", "Double", "Float", "Int", "List", "Long", "Map",
            "Nothing", "Short", "String", "Unit", "true", "false", "null",
        ],
        meta_prefixes: &['@'],
        triple_quotes: true,
        pascal_types: true,
        interpolation: &['"'],
        ..C_COMMENTS
    },
    Spec {
        name: "Swift",
        extensions: &["swift"],
        keywords: &[
            "actor",
            "as",
            "associatedtype",
            "async",
            "await",
            "break",
            "case",
            "catch",
            "class",
            "continue",
            "default",
            "defer",
            "deinit",
            "do",
            "else",
            "enum",
            "extension",
            "fallthrough",
            "fileprivate",
            "for",
            "func",
            "guard",
            "if",
            "import",
            "in",
            "init",
            "inout",
            "internal",
            "is",
            "let",
            "open",
            "operator",
            "private",
            "protocol",
            "public",
            "repeat",
            "rethrows",
            "return",
            "self",
            "Self",
            "static",
            "struct",
            "subscript",
            "super",
            "switch",
            "throw",
            "throws",
            "try",
            "typealias",
            "var",
            "where",
            "while",
        ],
        types: &[
            "Any",
            "Array",
            "Bool",
            "Character",
            "Dictionary",
            "Double",
            "Float",
            "Int",
            "Optional",
            "Set",
            "String",
            "Void",
            "true",
            "false",
            "nil",
        ],
        meta_prefixes: &['@'],
        line_meta: Some('#'),
        triple_quotes: true,
        quotes: &['"'],
        pascal_types: true,
        ..C_COMMENTS
    },
    Spec {
        name: "Go",
        extensions: &["go"],
        keywords: &[
            "break",
            "case",
            "chan",
            "const",
            "continue",
            "default",
            "defer",
            "else",
            "fallthrough",
            "for",
            "func",
            "go",
            "goto",
            "if",
            "import",
            "interface",
            "map",
            "package",
            "range",
            "return",
            "select",
            "struct",
            "switch",
            "type",
            "var",
        ],
        types: &[
            "any",
            "bool",
            "byte",
            "complex64",
            "complex128",
            "error",
            "float32",
            "float64",
            "int",
            "int8",
            "int16",
            "int32",
            "int64",
            "rune",
            "string",
            "uint",
            "uint8",
            "uint16",
            "uint32",
            "uint64",
            "uintptr",
            "true",
            "false",
            "nil",
            "iota",
        ],
        quotes: &['"', '\'', '`'],
        multiline_quotes: &['`'],
        ..C_COMMENTS
    },
    Spec {
        name: "JavaScript",
        extensions: &["js", "mjs", "cjs", "jsx"],
        keywords: JS_KEYWORDS,
        types: JS_TYPES,
        quotes: &['"', '\'', '`'],
        multiline_quotes: &['`'],
        meta_prefixes: &['@'],
        pascal_types: true,
        interpolation: &['`'],
        regex_literals: true,
        ..C_COMMENTS
    },
    Spec {
        name: "TypeScript",
        extensions: &["ts", "tsx", "mts", "cts"],
        keywords: TS_KEYWORDS,
        types: TS_TYPES,
        quotes: &['"', '\'', '`'],
        multiline_quotes: &['`'],
        meta_prefixes: &['@'],
        pascal_types: true,
        interpolation: &['`'],
        regex_literals: true,
        ..C_COMMENTS
    },
    Spec {
        name: "Python",
        extensions: &["py", "pyw", "pyi"],
        line_comments: &["#"],
        triple_quotes: true,
        meta_prefixes: &['@'],
        keywords: &[
            "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
            "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in",
            "is", "lambda", "match", "case", "nonlocal", "not", "or", "pass", "raise", "return",
            "try", "while", "with", "yield",
        ],
        types: &[
            "True", "False", "None", "self", "bool", "bytes", "dict", "float", "int", "list",
            "object", "set", "str", "tuple",
        ],
        ..BASE
    },
    Spec {
        name: "Ruby",
        extensions: &["rb", "rake", "gemspec"],
        file_names: &["Gemfile", "Rakefile"],
        line_comments: &["#"],
        meta_prefixes: &['@', ':'],
        keywords: &[
            "alias",
            "and",
            "begin",
            "break",
            "case",
            "class",
            "def",
            "defined?",
            "do",
            "else",
            "elsif",
            "end",
            "ensure",
            "for",
            "if",
            "in",
            "module",
            "next",
            "not",
            "or",
            "redo",
            "rescue",
            "retry",
            "return",
            "self",
            "super",
            "then",
            "undef",
            "unless",
            "until",
            "when",
            "while",
            "yield",
            "require",
            "attr_accessor",
            "attr_reader",
        ],
        types: &["true", "false", "nil"],
        ..BASE
    },
    Spec {
        name: "PHP",
        extensions: &["php", "phtml"],
        line_comments: &["//", "#"],
        meta_prefixes: &['$'],
        keywords: &[
            "abstract",
            "and",
            "as",
            "break",
            "case",
            "catch",
            "class",
            "clone",
            "const",
            "continue",
            "declare",
            "default",
            "do",
            "echo",
            "else",
            "elseif",
            "enum",
            "extends",
            "final",
            "finally",
            "fn",
            "for",
            "foreach",
            "function",
            "global",
            "if",
            "implements",
            "include",
            "instanceof",
            "interface",
            "match",
            "namespace",
            "new",
            "or",
            "private",
            "protected",
            "public",
            "readonly",
            "require",
            "return",
            "static",
            "switch",
            "throw",
            "trait",
            "try",
            "use",
            "var",
            "while",
            "yield",
        ],
        types: &[
            "array", "bool", "float", "int", "mixed", "null", "object", "string", "void", "true",
            "false",
        ],
        ignore_case: true,
        interpolation: &['"'],
        ..C_COMMENTS
    },
    Spec {
        name: "Lua",
        extensions: &["lua"],
        line_comments: &["--"],
        block_comment: Some(("--[[", "]]")),
        keywords: &[
            "and", "break", "do", "else", "elseif", "end", "for", "function", "goto", "if", "in",
            "local", "not", "or", "repeat", "return", "then", "until", "while",
        ],
        types: &["true", "false", "nil", "self"],
        ..BASE
    },
    Spec {
        name: "Shell",
        extensions: &["sh", "bash", "zsh", "ksh", "fish", "command"],
        file_names: &[
            ".bashrc",
            ".bash_profile",
            ".zshrc",
            ".zprofile",
            ".profile",
        ],
        line_comments: &["#"],
        quotes: &['"', '\'', '`'],
        multiline_quotes: &['"', '\''],
        meta_prefixes: &['$'],
        keywords: SHELL_KEYWORDS,
        types: &["true", "false"],
        interpolation: &['"', '`'],
        ..BASE
    },
    Spec {
        name: "PowerShell",
        extensions: &["ps1", "psm1", "psd1"],
        line_comments: &["#"],
        block_comment: Some(("<#", "#>")),
        meta_prefixes: &['$'],
        ident_extra: &['-'],
        ignore_case: true,
        keywords: &[
            "begin", "break", "catch", "class", "continue", "do", "else", "elseif", "end", "enum",
            "exit", "filter", "finally", "for", "foreach", "function", "if", "in", "param",
            "process", "return", "switch", "throw", "trap", "try", "until", "while",
        ],
        ..BASE
    },
    Spec {
        name: "Batch",
        extensions: &["bat", "cmd"],
        line_comments: &["::", "rem ", "REM ", "@rem ", "@REM "],
        quotes: &['"'],
        escapes: false,
        meta_prefixes: &['%'],
        ignore_case: true,
        keywords: &[
            "call", "cd", "echo", "else", "endlocal", "exist", "exit", "for", "goto", "if", "in",
            "not", "off", "on", "pause", "set", "setlocal", "shift", "start",
        ],
        ..BASE
    },
    Spec {
        name: "SQL",
        extensions: &["sql"],
        line_comments: &["--"],
        quotes: &['\'', '"', '`'],
        escapes: false,
        ignore_case: true,
        keywords: SQL_KEYWORDS,
        types: SQL_TYPES,
        ..C_COMMENTS
    },
    Spec {
        name: "HTML",
        extensions: &["html", "htm", "xhtml", "vue", "svelte"],
        mode: Mode::Markup,
        embeds: true,
        ..BASE
    },
    Spec {
        name: "XML",
        extensions: &[
            "xml",
            "svg",
            "plist",
            "xsd",
            "xsl",
            "xslt",
            "xaml",
            "csproj",
            "vcxproj",
            "props",
            "targets",
            "resx",
            "storyboard",
        ],
        mode: Mode::Markup,
        ..BASE
    },
    Spec {
        name: "CSS",
        extensions: &["css", "scss", "sass", "less"],
        meta_prefixes: &['@', '$', '#', '.'],
        ident_extra: &['-'],
        keys: Keys::BeforeColon,
        types: &["important", "inherit", "initial", "none", "auto"],
        ..C_COMMENTS
    },
    Spec {
        name: "JSON",
        extensions: &["json", "jsonc", "json5", "geojson", "webmanifest"],
        file_names: &[".babelrc", ".eslintrc", "composer.lock"],
        quotes: &['"'],
        keys: Keys::Quoted,
        types: &["true", "false", "null"],
        ..C_COMMENTS
    },
    Spec {
        name: "YAML",
        extensions: &["yml", "yaml"],
        line_comments: &["#"],
        ident_extra: &['-', '.'],
        keys: Keys::BeforeColon,
        types: &["true", "false", "null", "yes", "no", "on", "off"],
        ..BASE
    },
    Spec {
        name: "TOML",
        extensions: &["toml"],
        file_names: &["Cargo.lock", "Pipfile"],
        line_comments: &["#"],
        triple_quotes: true,
        ident_extra: &['-', '.'],
        keys: Keys::BeforeEquals,
        section_headers: true,
        types: &["true", "false"],
        ..BASE
    },
    Spec {
        name: "INI",
        extensions: &[
            "ini",
            "cfg",
            "conf",
            "properties",
            "desktop",
            "inf",
            "reg",
            "editorconfig",
        ],
        file_names: &[".editorconfig", ".gitconfig", ".npmrc"],
        line_comments: &[";", "#"],
        escapes: false,
        ident_extra: &['-', '.'],
        keys: Keys::BeforeEquals,
        section_headers: true,
        types: &["true", "false", "yes", "no", "on", "off"],
        ..BASE
    },
    Spec {
        name: "Markdown",
        extensions: &["md", "markdown", "mdown", "mkd"],
        mode: Mode::Markdown,
        ..BASE
    },
    Spec {
        name: "Makefile",
        extensions: &["mk", "mak"],
        file_names: &["Makefile", "makefile", "GNUmakefile"],
        line_comments: &["#"],
        meta_prefixes: &['$'],
        keys: Keys::BeforeColon,
        ident_extra: &['-', '.'],
        keywords: &[
            "define", "else", "endef", "endif", "export", "ifdef", "ifeq", "ifndef", "ifneq",
            "include", "override", "unexport", ".PHONY",
        ],
        ..BASE
    },
    Spec {
        name: "Dockerfile",
        extensions: &["dockerfile"],
        file_names: &["Dockerfile", "Containerfile"],
        line_comments: &["#"],
        meta_prefixes: &['$'],
        keywords: &[
            "ADD",
            "ARG",
            "AS",
            "CMD",
            "COPY",
            "ENTRYPOINT",
            "ENV",
            "EXPOSE",
            "FROM",
            "HEALTHCHECK",
            "LABEL",
            "MAINTAINER",
            "ONBUILD",
            "RUN",
            "SHELL",
            "STOPSIGNAL",
            "USER",
            "VOLUME",
            "WORKDIR",
        ],
        ..BASE
    },
    Spec {
        name: "Diff",
        extensions: &["diff", "patch"],
        mode: Mode::Diff,
        ..BASE
    },
    Spec {
        name: "Perl",
        extensions: &["pl", "pm", "cgi", "psgi"],
        line_comments: &["#"],
        meta_prefixes: &['$', '@'],
        interpolation: &['"'],
        keywords: &[
            "and", "cmp", "do", "else", "elsif", "eq", "eval", "for", "foreach", "ge", "gt", "if",
            "last", "le", "local", "lt", "my", "ne", "next", "no", "not", "or", "our", "package",
            "print", "qw", "redo", "require", "return", "sub", "unless", "until", "use", "while",
        ],
        types: &["undef"],
        ..BASE
    },
    Spec {
        name: "R",
        extensions: &["r"],
        file_names: &[".Rprofile"],
        line_comments: &["#"],
        ident_extra: &['.'],
        keywords: &[
            "break", "else", "for", "function", "if", "in", "library", "next", "repeat", "require",
            "return", "while",
        ],
        types: &["TRUE", "FALSE", "NULL", "NA", "Inf", "NaN", "T", "F"],
        ..BASE
    },
    Spec {
        name: "Scala",
        extensions: &["scala", "sc", "sbt"],
        triple_quotes: true,
        pascal_types: true,
        interpolation: &['"'],
        meta_prefixes: &['@'],
        keywords: &[
            "abstract",
            "case",
            "catch",
            "class",
            "def",
            "do",
            "else",
            "end",
            "enum",
            "export",
            "extends",
            "final",
            "finally",
            "for",
            "given",
            "if",
            "implicit",
            "import",
            "lazy",
            "match",
            "new",
            "object",
            "override",
            "package",
            "private",
            "protected",
            "return",
            "sealed",
            "super",
            "then",
            "this",
            "throw",
            "trait",
            "try",
            "type",
            "using",
            "val",
            "var",
            "while",
            "with",
            "yield",
        ],
        types: &["true", "false", "null"],
        ..C_COMMENTS
    },
    Spec {
        name: "Dart",
        extensions: &["dart"],
        triple_quotes: true,
        pascal_types: true,
        interpolation: &['"', '\''],
        meta_prefixes: &['@'],
        keywords: &[
            "abstract",
            "as",
            "assert",
            "async",
            "await",
            "break",
            "case",
            "catch",
            "class",
            "const",
            "continue",
            "covariant",
            "default",
            "deferred",
            "do",
            "dynamic",
            "else",
            "enum",
            "export",
            "extends",
            "extension",
            "external",
            "factory",
            "final",
            "finally",
            "for",
            "get",
            "if",
            "implements",
            "import",
            "in",
            "interface",
            "is",
            "late",
            "library",
            "mixin",
            "new",
            "on",
            "operator",
            "part",
            "required",
            "rethrow",
            "return",
            "sealed",
            "set",
            "show",
            "static",
            "super",
            "switch",
            "sync",
            "this",
            "throw",
            "try",
            "typedef",
            "var",
            "when",
            "while",
            "with",
            "yield",
        ],
        types: &[
            "bool", "double", "int", "num", "void", "true", "false", "null",
        ],
        ..C_COMMENTS
    },
    Spec {
        name: "Haskell",
        extensions: &["hs", "lhs"],
        line_comments: &["--"],
        block_comment: Some(("{-", "-}")),
        quotes: &['"'],
        ident_extra: &['\''],
        pascal_types: true,
        keywords: &[
            "as",
            "case",
            "class",
            "data",
            "default",
            "deriving",
            "do",
            "else",
            "family",
            "forall",
            "foreign",
            "hiding",
            "if",
            "import",
            "in",
            "infix",
            "infixl",
            "infixr",
            "instance",
            "let",
            "module",
            "newtype",
            "of",
            "qualified",
            "then",
            "type",
            "where",
        ],
        ..BASE
    },
    Spec {
        name: "Elixir",
        extensions: &["ex", "exs", "heex"],
        file_names: &["mix.lock"],
        line_comments: &["#"],
        triple_quotes: true,
        pascal_types: true,
        meta_prefixes: &['@', ':'],
        ident_extra: &['?', '!'],
        keywords: &[
            "after",
            "alias",
            "and",
            "case",
            "catch",
            "cond",
            "def",
            "defimpl",
            "defmacro",
            "defmodule",
            "defp",
            "defprotocol",
            "defstruct",
            "do",
            "else",
            "end",
            "fn",
            "for",
            "if",
            "import",
            "in",
            "not",
            "or",
            "quote",
            "raise",
            "receive",
            "require",
            "rescue",
            "try",
            "unless",
            "unquote",
            "use",
            "when",
            "with",
        ],
        types: &["true", "false", "nil"],
        ..BASE
    },
    Spec {
        name: "Erlang",
        extensions: &["erl", "hrl", "escript"],
        file_names: &["rebar.config"],
        line_comments: &["%"],
        line_meta: Some('-'),
        meta_prefixes: &['?'],
        keywords: &[
            "after", "and", "andalso", "band", "begin", "bnot", "bor", "bsl", "bsr", "bxor",
            "case", "catch", "cond", "div", "end", "fun", "if", "let", "not", "of", "or", "orelse",
            "receive", "rem", "try", "when", "xor",
        ],
        types: &["true", "false", "undefined", "ok", "error"],
        ..BASE
    },
    Spec {
        name: "Fortran",
        extensions: &["f", "for", "ftn", "f77", "f90", "f95", "f03", "f08"],
        line_comments: &["!"],
        escapes: false,
        ignore_case: true,
        ident_extra: &['.'],
        keywords: &[
            ".and.",
            ".eq.",
            ".ge.",
            ".gt.",
            ".le.",
            ".lt.",
            ".ne.",
            ".not.",
            ".or.",
            "allocate",
            "call",
            "case",
            "close",
            "contains",
            "continue",
            "cycle",
            "deallocate",
            "default",
            "dimension",
            "do",
            "else",
            "elseif",
            "end",
            "enddo",
            "endif",
            "exit",
            "format",
            "function",
            "go",
            "goto",
            "if",
            "implicit",
            "in",
            "inout",
            "intent",
            "interface",
            "module",
            "none",
            "open",
            "out",
            "parameter",
            "pointer",
            "print",
            "private",
            "program",
            "public",
            "read",
            "return",
            "select",
            "stop",
            "subroutine",
            "then",
            "to",
            "type",
            "use",
            "where",
            "while",
            "write",
        ],
        types: &[
            "integer",
            "real",
            "double",
            "precision",
            "complex",
            "character",
            "logical",
            ".true.",
            ".false.",
        ],
        ..BASE
    },
    Spec {
        name: "Pascal",
        extensions: &["pas", "pp", "dpr", "dpk", "lpr", "dfm"],
        line_comments: &["//"],
        block_comment: Some(("{", "}")),
        quotes: &['\''],
        escapes: false,
        ignore_case: true,
        keywords: &[
            "abstract",
            "and",
            "array",
            "as",
            "begin",
            "case",
            "class",
            "const",
            "constructor",
            "destructor",
            "div",
            "do",
            "downto",
            "else",
            "end",
            "except",
            "exports",
            "file",
            "finalization",
            "finally",
            "for",
            "function",
            "goto",
            "if",
            "implementation",
            "in",
            "inherited",
            "initialization",
            "inline",
            "interface",
            "is",
            "label",
            "library",
            "mod",
            "nil",
            "not",
            "object",
            "of",
            "or",
            "out",
            "override",
            "packed",
            "private",
            "procedure",
            "program",
            "property",
            "protected",
            "public",
            "published",
            "raise",
            "record",
            "repeat",
            "set",
            "shl",
            "shr",
            "then",
            "to",
            "try",
            "type",
            "unit",
            "until",
            "uses",
            "var",
            "virtual",
            "while",
            "with",
            "xor",
        ],
        types: &[
            "boolean", "byte", "cardinal", "char", "double", "extended", "int64", "integer",
            "longint", "real", "shortint", "single", "smallint", "string", "word", "true", "false",
        ],
        ..BASE
    },
    Spec {
        name: "Visual Basic",
        extensions: &["vb", "vbs", "bas", "frm", "vba"],
        line_comments: &["'", "rem "],
        quotes: &['"'],
        escapes: false,
        ignore_case: true,
        keywords: &[
            "addressof",
            "and",
            "andalso",
            "as",
            "byref",
            "byval",
            "call",
            "case",
            "catch",
            "class",
            "const",
            "dim",
            "do",
            "each",
            "else",
            "elseif",
            "end",
            "enum",
            "erase",
            "error",
            "event",
            "exit",
            "finally",
            "for",
            "friend",
            "function",
            "get",
            "goto",
            "handles",
            "if",
            "implements",
            "imports",
            "in",
            "inherits",
            "interface",
            "is",
            "let",
            "like",
            "loop",
            "me",
            "mod",
            "module",
            "mybase",
            "new",
            "next",
            "not",
            "of",
            "on",
            "option",
            "optional",
            "or",
            "orelse",
            "overrides",
            "paramarray",
            "private",
            "property",
            "protected",
            "public",
            "raiseevent",
            "readonly",
            "redim",
            "resume",
            "return",
            "select",
            "set",
            "shared",
            "static",
            "step",
            "stop",
            "structure",
            "sub",
            "then",
            "throw",
            "to",
            "try",
            "typeof",
            "until",
            "using",
            "wend",
            "when",
            "while",
            "with",
            "xor",
        ],
        types: &[
            "boolean", "byte", "char", "date", "decimal", "double", "integer", "long", "object",
            "short", "single", "string", "variant", "true", "false", "nothing", "null",
        ],
        ..BASE
    },
    Spec {
        name: "Assembly",
        extensions: &["asm", "s", "nasm", "a51"],
        line_comments: &[";"],
        block_comment: Some(("/*", "*/")),
        meta_prefixes: &['.', '%', '$'],
        ignore_case: true,
        keywords: &[
            "adc", "add", "adr", "adrp", "and", "b", "bl", "blr", "br", "bx", "call", "cbnz",
            "cbz", "cmp", "dec", "div", "enter", "idiv", "imul", "inc", "int", "ja", "jae", "jb",
            "jbe", "je", "jg", "jge", "jl", "jle", "jmp", "jne", "jnz", "jz", "ldp", "ldr", "lea",
            "leave", "loop", "mov", "movsx", "movzx", "mul", "neg", "nop", "not", "or", "pop",
            "push", "rep", "ret", "rol", "ror", "sal", "sar", "shl", "shr", "stp", "str", "sub",
            "syscall", "test", "xchg", "xor",
        ],
        types: &[
            "al", "ah", "ax", "eax", "rax", "bl", "bh", "bx", "ebx", "rbx", "cl", "ch", "cx",
            "ecx", "rcx", "dl", "dh", "dx", "edx", "rdx", "si", "esi", "rsi", "di", "edi", "rdi",
            "bp", "ebp", "rbp", "sp", "esp", "rsp", "r8", "r9", "r10", "r11", "r12", "r13", "r14",
            "r15", "lr", "pc", "fp", "xzr", "wzr",
        ],
        ..BASE
    },
    Spec {
        name: "LaTeX",
        extensions: &["tex", "latex", "ltx", "sty", "cls", "dtx", "bib"],
        line_comments: &["%"],
        quotes: &['$'],
        escapes: false,
        meta_prefixes: &['\\'],
        ..BASE
    },
    Spec {
        name: "CMake",
        extensions: &["cmake"],
        file_names: &["CMakeLists.txt"],
        line_comments: &["#"],
        quotes: &['"'],
        multiline_quotes: &['"'],
        interpolation: &['"'],
        ignore_case: true,
        keywords: &[
            "add_custom_command",
            "add_custom_target",
            "add_definitions",
            "add_dependencies",
            "add_executable",
            "add_library",
            "add_subdirectory",
            "add_test",
            "cmake_minimum_required",
            "else",
            "elseif",
            "endforeach",
            "endfunction",
            "endif",
            "endmacro",
            "endwhile",
            "file",
            "find_package",
            "foreach",
            "function",
            "if",
            "include",
            "include_directories",
            "install",
            "list",
            "macro",
            "message",
            "option",
            "project",
            "return",
            "set",
            "string",
            "target_compile_definitions",
            "target_compile_options",
            "target_include_directories",
            "target_link_libraries",
            "unset",
            "while",
        ],
        types: &["on", "off", "true", "false", "yes", "no"],
        ..BASE
    },
    Spec {
        name: "Groovy",
        extensions: &["groovy", "gradle", "gvy", "gy"],
        file_names: &["Jenkinsfile"],
        triple_quotes: true,
        pascal_types: true,
        interpolation: &['"'],
        meta_prefixes: &['@'],
        keywords: &[
            "abstract",
            "as",
            "assert",
            "break",
            "case",
            "catch",
            "class",
            "const",
            "continue",
            "def",
            "default",
            "do",
            "else",
            "enum",
            "extends",
            "final",
            "finally",
            "for",
            "if",
            "implements",
            "import",
            "in",
            "instanceof",
            "interface",
            "new",
            "package",
            "private",
            "protected",
            "public",
            "return",
            "static",
            "super",
            "switch",
            "synchronized",
            "this",
            "throw",
            "throws",
            "trait",
            "try",
            "var",
            "while",
        ],
        types: &[
            "boolean", "byte", "char", "double", "float", "int", "long", "short", "void", "true",
            "false", "null",
        ],
        ..C_COMMENTS
    },
    Spec {
        name: "Julia",
        extensions: &["jl"],
        line_comments: &["#"],
        block_comment: Some(("#=", "=#")),
        triple_quotes: true,
        pascal_types: true,
        interpolation: &['"'],
        meta_prefixes: &['@'],
        keywords: &[
            "abstract",
            "baremodule",
            "begin",
            "break",
            "catch",
            "const",
            "continue",
            "do",
            "else",
            "elseif",
            "end",
            "export",
            "finally",
            "for",
            "function",
            "global",
            "if",
            "import",
            "in",
            "let",
            "local",
            "macro",
            "module",
            "mutable",
            "primitive",
            "quote",
            "return",
            "struct",
            "try",
            "using",
            "where",
            "while",
        ],
        types: &["true", "false", "nothing", "missing"],
        ..BASE
    },
    Spec {
        name: "Objective-C",
        extensions: &["m", "mm"],
        keywords: C_KEYWORDS,
        types: &[
            "bool",
            "char",
            "double",
            "float",
            "int",
            "long",
            "short",
            "signed",
            "unsigned",
            "void",
            "id",
            "instancetype",
            "self",
            "super",
            "nil",
            "Nil",
            "YES",
            "NO",
            "NULL",
            "BOOL",
            "SEL",
        ],
        line_meta: Some('#'),
        meta_prefixes: &['@'],
        pascal_types: true,
        ..C_COMMENTS
    },
    Spec {
        name: "Lisp",
        extensions: &[
            "lisp", "lsp", "el", "cl", "scm", "ss", "rkt", "clj", "cljs", "cljc", "edn",
        ],
        file_names: &[".emacs"],
        line_comments: &[";"],
        block_comment: Some(("#|", "|#")),
        quotes: &['"'],
        meta_prefixes: &[':'],
        ident_extra: &['-', '?', '!', '*', '+', '<', '>', '=', '/'],
        keywords: &[
            "and",
            "begin",
            "case",
            "cond",
            "def",
            "define",
            "define-syntax",
            "defmacro",
            "defn",
            "defn-",
            "defparameter",
            "defun",
            "defvar",
            "do",
            "fn",
            "if",
            "lambda",
            "let",
            "let*",
            "loop",
            "ns",
            "or",
            "progn",
            "quote",
            "require",
            "setf",
            "setq",
            "unless",
            "when",
        ],
        types: &["t", "nil", "true", "false"],
        ..BASE
    },
    Spec {
        name: "F#",
        extensions: &["fs", "fsi", "fsx"],
        line_comments: &["//"],
        block_comment: Some(("(*", "*)")),
        quotes: &['"'],
        triple_quotes: true,
        ident_extra: &['\''],
        pascal_types: true,
        keywords: &[
            "abstract",
            "and",
            "as",
            "assert",
            "base",
            "begin",
            "class",
            "default",
            "delegate",
            "do",
            "done",
            "downto",
            "elif",
            "else",
            "end",
            "exception",
            "extern",
            "finally",
            "for",
            "fun",
            "function",
            "if",
            "in",
            "inherit",
            "inline",
            "interface",
            "internal",
            "lazy",
            "let",
            "match",
            "member",
            "module",
            "mutable",
            "namespace",
            "new",
            "not",
            "of",
            "open",
            "or",
            "override",
            "private",
            "public",
            "rec",
            "return",
            "static",
            "struct",
            "then",
            "to",
            "try",
            "type",
            "use",
            "val",
            "when",
            "while",
            "with",
            "yield",
        ],
        types: &[
            "bool", "char", "float", "int", "string", "unit", "true", "false", "null",
        ],
        ..BASE
    },
    Spec {
        name: "OCaml",
        extensions: &["ml", "mli"],
        block_comment: Some(("(*", "*)")),
        quotes: &['"'],
        ident_extra: &['\''],
        pascal_types: true,
        keywords: &[
            "and",
            "as",
            "assert",
            "begin",
            "class",
            "constraint",
            "do",
            "done",
            "downto",
            "else",
            "end",
            "exception",
            "external",
            "for",
            "fun",
            "function",
            "functor",
            "if",
            "in",
            "include",
            "inherit",
            "initializer",
            "lazy",
            "let",
            "match",
            "method",
            "module",
            "mutable",
            "new",
            "object",
            "of",
            "open",
            "or",
            "private",
            "rec",
            "sig",
            "struct",
            "then",
            "to",
            "try",
            "type",
            "val",
            "virtual",
            "when",
            "while",
            "with",
        ],
        types: &[
            "bool", "char", "float", "int", "list", "option", "string", "unit", "true", "false",
        ],
        ..BASE
    },
    Spec {
        name: "Ada",
        extensions: &["adb", "ads", "ada"],
        line_comments: &["--"],
        quotes: &['"'],
        escapes: false,
        ignore_case: true,
        keywords: &[
            "abort",
            "abs",
            "abstract",
            "accept",
            "access",
            "aliased",
            "all",
            "and",
            "array",
            "at",
            "begin",
            "body",
            "case",
            "constant",
            "declare",
            "delay",
            "delta",
            "digits",
            "do",
            "else",
            "elsif",
            "end",
            "entry",
            "exception",
            "exit",
            "for",
            "function",
            "generic",
            "goto",
            "if",
            "in",
            "interface",
            "is",
            "limited",
            "loop",
            "mod",
            "new",
            "not",
            "null",
            "of",
            "or",
            "others",
            "out",
            "overriding",
            "package",
            "pragma",
            "private",
            "procedure",
            "protected",
            "raise",
            "range",
            "record",
            "rem",
            "renames",
            "requeue",
            "return",
            "reverse",
            "select",
            "separate",
            "some",
            "subtype",
            "synchronized",
            "tagged",
            "task",
            "terminate",
            "then",
            "type",
            "until",
            "use",
            "when",
            "while",
            "with",
            "xor",
        ],
        types: &[
            "boolean",
            "character",
            "float",
            "integer",
            "natural",
            "positive",
            "string",
            "true",
            "false",
        ],
        ..BASE
    },
    Spec {
        name: "Tcl",
        extensions: &["tcl", "tk", "exp"],
        line_comments: &["#"],
        quotes: &['"'],
        multiline_quotes: &['"'],
        meta_prefixes: &['$'],
        interpolation: &['"'],
        keywords: &[
            "after",
            "append",
            "array",
            "break",
            "catch",
            "continue",
            "else",
            "elseif",
            "error",
            "eval",
            "expr",
            "for",
            "foreach",
            "format",
            "global",
            "if",
            "incr",
            "info",
            "lappend",
            "lindex",
            "list",
            "llength",
            "lrange",
            "lsearch",
            "lsort",
            "namespace",
            "proc",
            "puts",
            "regexp",
            "regsub",
            "return",
            "set",
            "source",
            "split",
            "string",
            "switch",
            "unset",
            "uplevel",
            "upvar",
            "variable",
            "while",
        ],
        ..BASE
    },
    Spec {
        name: "MATLAB",
        line_comments: &["%"],
        block_comment: Some(("%{", "%}")),
        escapes: false,
        keywords: &[
            "break",
            "case",
            "catch",
            "classdef",
            "continue",
            "else",
            "elseif",
            "end",
            "for",
            "function",
            "global",
            "if",
            "otherwise",
            "parfor",
            "persistent",
            "return",
            "spmd",
            "switch",
            "try",
            "while",
        ],
        types: &["true", "false", "pi", "inf", "nan", "eps"],
        ..BASE
    },
    Spec {
        name: "Verilog",
        extensions: &["v", "vh", "sv", "svh"],
        quotes: &['"'],
        meta_prefixes: &['`', '$'],
        keywords: &[
            "always",
            "always_comb",
            "always_ff",
            "assign",
            "begin",
            "case",
            "casex",
            "casez",
            "class",
            "default",
            "else",
            "end",
            "endcase",
            "endclass",
            "endfunction",
            "endinterface",
            "endmodule",
            "endpackage",
            "endtask",
            "enum",
            "for",
            "forever",
            "function",
            "generate",
            "genvar",
            "if",
            "import",
            "initial",
            "inout",
            "input",
            "interface",
            "localparam",
            "module",
            "negedge",
            "output",
            "package",
            "parameter",
            "posedge",
            "repeat",
            "signed",
            "struct",
            "task",
            "typedef",
            "while",
        ],
        types: &[
            "bit", "byte", "int", "integer", "logic", "reg", "wire", "real", "time",
        ],
        ..C_COMMENTS
    },
    Spec {
        name: "VHDL",
        extensions: &["vhd", "vhdl"],
        line_comments: &["--"],
        quotes: &['"'],
        escapes: false,
        ignore_case: true,
        keywords: &[
            "abs",
            "access",
            "after",
            "alias",
            "all",
            "and",
            "architecture",
            "array",
            "assert",
            "attribute",
            "begin",
            "block",
            "body",
            "buffer",
            "bus",
            "case",
            "component",
            "configuration",
            "constant",
            "downto",
            "else",
            "elsif",
            "end",
            "entity",
            "exit",
            "file",
            "for",
            "function",
            "generate",
            "generic",
            "if",
            "in",
            "inout",
            "is",
            "library",
            "loop",
            "map",
            "mod",
            "nand",
            "new",
            "next",
            "nor",
            "not",
            "null",
            "of",
            "on",
            "open",
            "or",
            "others",
            "out",
            "package",
            "port",
            "procedure",
            "process",
            "range",
            "record",
            "register",
            "rem",
            "report",
            "return",
            "select",
            "signal",
            "subtype",
            "then",
            "to",
            "type",
            "until",
            "use",
            "variable",
            "wait",
            "when",
            "while",
            "with",
            "xor",
        ],
        types: &[
            "bit",
            "bit_vector",
            "boolean",
            "integer",
            "natural",
            "positive",
            "signed",
            "std_logic",
            "std_logic_vector",
            "unsigned",
            "true",
            "false",
        ],
        ..BASE
    },
    Spec {
        name: "GraphQL",
        extensions: &["graphql", "gql", "graphqls"],
        line_comments: &["#"],
        quotes: &['"'],
        triple_quotes: true,
        meta_prefixes: &['$', '@'],
        pascal_types: true,
        keywords: &[
            "directive",
            "enum",
            "extend",
            "fragment",
            "implements",
            "input",
            "interface",
            "mutation",
            "on",
            "query",
            "scalar",
            "schema",
            "subscription",
            "type",
            "union",
        ],
        types: &[
            "true", "false", "null", "ID", "Int", "Float", "String", "Boolean",
        ],
        ..BASE
    },
    Spec {
        name: "Protocol Buffers",
        extensions: &["proto"],
        pascal_types: true,
        keywords: &[
            "enum", "extend", "import", "map", "message", "oneof", "option", "optional", "package",
            "public", "repeated", "required", "reserved", "returns", "rpc", "service", "stream",
            "syntax", "weak",
        ],
        types: &[
            "bool", "bytes", "double", "fixed32", "fixed64", "float", "int32", "int64", "sfixed32",
            "sfixed64", "sint32", "sint64", "string", "uint32", "uint64", "true", "false",
        ],
        ..C_COMMENTS
    },
    Spec {
        name: "Zig",
        extensions: &["zig", "zon"],
        line_comments: &["//"],
        meta_prefixes: &['@'],
        pascal_types: true,
        keywords: &[
            "align",
            "allowzero",
            "and",
            "anyframe",
            "anytype",
            "asm",
            "break",
            "callconv",
            "catch",
            "comptime",
            "const",
            "continue",
            "defer",
            "else",
            "enum",
            "errdefer",
            "error",
            "export",
            "extern",
            "fn",
            "for",
            "if",
            "inline",
            "noalias",
            "noinline",
            "nosuspend",
            "opaque",
            "or",
            "orelse",
            "packed",
            "pub",
            "resume",
            "return",
            "struct",
            "suspend",
            "switch",
            "test",
            "threadlocal",
            "try",
            "union",
            "unreachable",
            "usingnamespace",
            "var",
            "volatile",
            "while",
        ],
        types: &[
            "anyerror",
            "bool",
            "comptime_float",
            "comptime_int",
            "f16",
            "f32",
            "f64",
            "f128",
            "i8",
            "i16",
            "i32",
            "i64",
            "i128",
            "isize",
            "noreturn",
            "type",
            "u8",
            "u16",
            "u32",
            "u64",
            "u128",
            "usize",
            "void",
            "true",
            "false",
            "null",
            "undefined",
        ],
        ..BASE
    },
    Spec {
        name: "Nim",
        extensions: &["nim", "nims", "nimble"],
        line_comments: &["#"],
        block_comment: Some(("#[", "]#")),
        triple_quotes: true,
        pascal_types: true,
        keywords: &[
            "addr",
            "and",
            "as",
            "asm",
            "bind",
            "block",
            "break",
            "case",
            "cast",
            "concept",
            "const",
            "continue",
            "converter",
            "defer",
            "discard",
            "distinct",
            "div",
            "do",
            "elif",
            "else",
            "end",
            "enum",
            "except",
            "export",
            "finally",
            "for",
            "from",
            "func",
            "if",
            "import",
            "in",
            "include",
            "interface",
            "is",
            "isnot",
            "iterator",
            "let",
            "macro",
            "method",
            "mixin",
            "mod",
            "nil",
            "not",
            "notin",
            "object",
            "of",
            "or",
            "out",
            "proc",
            "ptr",
            "raise",
            "ref",
            "return",
            "shl",
            "shr",
            "static",
            "template",
            "try",
            "tuple",
            "type",
            "using",
            "var",
            "when",
            "while",
            "xor",
            "yield",
        ],
        types: &[
            "bool", "char", "float", "int", "seq", "string", "true", "false",
        ],
        ..BASE
    },
    Spec {
        name: "D",
        extensions: &["d", "di"],
        pascal_types: true,
        meta_prefixes: &['@'],
        keywords: &[
            "abstract",
            "alias",
            "align",
            "asm",
            "assert",
            "auto",
            "break",
            "case",
            "cast",
            "catch",
            "class",
            "const",
            "continue",
            "debug",
            "default",
            "delegate",
            "deprecated",
            "do",
            "else",
            "enum",
            "export",
            "extern",
            "final",
            "finally",
            "for",
            "foreach",
            "foreach_reverse",
            "function",
            "goto",
            "if",
            "immutable",
            "import",
            "in",
            "inout",
            "interface",
            "invariant",
            "is",
            "lazy",
            "mixin",
            "module",
            "new",
            "nothrow",
            "out",
            "override",
            "package",
            "pragma",
            "private",
            "protected",
            "public",
            "pure",
            "ref",
            "return",
            "scope",
            "shared",
            "static",
            "struct",
            "super",
            "switch",
            "synchronized",
            "template",
            "this",
            "throw",
            "try",
            "typeof",
            "union",
            "unittest",
            "version",
            "while",
            "with",
        ],
        types: &[
            "bool", "byte", "char", "dchar", "double", "float", "int", "long", "real", "short",
            "string", "ubyte", "uint", "ulong", "ushort", "void", "wchar", "true", "false", "null",
        ],
        ..C_COMMENTS
    },
    Spec {
        name: "HCL",
        extensions: &["tf", "tfvars", "hcl", "nomad"],
        line_comments: &["#", "//"],
        block_comment: Some(("/*", "*/")),
        quotes: &['"'],
        interpolation: &['"'],
        ident_extra: &['-'],
        keys: Keys::BeforeEquals,
        keywords: &[
            "data",
            "dynamic",
            "for",
            "if",
            "in",
            "locals",
            "module",
            "output",
            "provider",
            "resource",
            "terraform",
            "variable",
        ],
        types: &["true", "false", "null"],
        ..BASE
    },
    Spec {
        name: "NSIS",
        extensions: &["nsi", "nsh"],
        line_comments: &[";", "#"],
        block_comment: Some(("/*", "*/")),
        quotes: &['"', '\'', '`'],
        escapes: false,
        meta_prefixes: &['$', '!'],
        ignore_case: true,
        keywords: &[
            "call",
            "createdirectory",
            "createshortcut",
            "delete",
            "deleteregkey",
            "exec",
            "execwait",
            "file",
            "function",
            "functionend",
            "goto",
            "icon",
            "installdir",
            "intcmp",
            "messagebox",
            "name",
            "outfile",
            "page",
            "readregstr",
            "requestexecutionlevel",
            "return",
            "rmdir",
            "section",
            "sectionend",
            "setoutpath",
            "strcmp",
            "uninstpage",
            "var",
            "writeregstr",
            "writeuninstaller",
        ],
        ..BASE
    },
];

/// Syntax of a document: an index into the built-in languages, followed by the user's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Language(usize);

/// A language defined by the user: keywords, comments and strings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserLanguage {
    pub name: String,
    /// File extensions without the dot.
    pub extensions: Vec<String>,
    pub keywords: Vec<String>,
    pub types: Vec<String>,
    pub line_comment: String,
    pub block_comment: Option<(String, String)>,
    /// Characters that start and end strings.
    pub quotes: String,
    pub ignore_case: bool,
}

/// Specs of user languages. Each definition is leaked once when registered, so specs can be
/// `'static` like the built-in ones; redefining a language leaks the old one (a few KB).
static USER_SPECS: RwLock<Vec<&'static Spec>> = RwLock::new(Vec::new());

fn leak_str(text: &str) -> &'static str {
    Box::leak(text.to_owned().into_boxed_str())
}

fn leak_list(items: &[String]) -> &'static [&'static str] {
    let list: Vec<&'static str> = items
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(leak_str)
        .collect();
    Box::leak(list.into_boxed_slice())
}

impl UserLanguage {
    fn to_spec(&self) -> Spec {
        let extensions: Vec<String> = self
            .extensions
            .iter()
            .map(|e| e.trim().trim_start_matches('.').to_lowercase())
            .collect();
        let line_comment = self.line_comment.trim();
        let line_comments: &'static [&'static str] = if line_comment.is_empty() {
            &[]
        } else {
            Box::leak(vec![leak_str(line_comment)].into_boxed_slice())
        };
        let block_comment = self
            .block_comment
            .as_ref()
            .map(|(open, close)| (open.trim(), close.trim()))
            .filter(|(open, close)| !open.is_empty() && !close.is_empty())
            .map(|(open, close)| (leak_str(open), leak_str(close)));
        let quotes: Vec<char> = self.quotes.chars().filter(|c| !c.is_whitespace()).collect();
        Spec {
            name: leak_str(self.name.trim()),
            extensions: leak_list(&extensions),
            line_comments,
            block_comment,
            quotes: Box::leak(quotes.into_boxed_slice()),
            keywords: leak_list(&self.keywords),
            types: leak_list(&self.types),
            ignore_case: self.ignore_case,
            ..BASE
        }
    }
}

/// Replaces the user-defined languages. Documents using one should reset their highlighter.
pub fn set_user_languages(languages: &[UserLanguage]) {
    let specs: Vec<&'static Spec> = languages
        .iter()
        .filter(|language| !language.name.trim().is_empty())
        .map(|language| &*Box::leak(Box::new(language.to_spec())))
        .collect();
    *USER_SPECS.write().unwrap_or_else(PoisonError::into_inner) = specs;
}

fn user_spec_count() -> usize {
    USER_SPECS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .len()
}

impl Language {
    pub const PLAIN_TEXT: Self = Self(0);

    pub fn all() -> impl Iterator<Item = Self> {
        (0..SPECS.len() + user_spec_count()).map(Self)
    }

    /// Whether this language was defined by the user.
    pub fn is_user_defined(self) -> bool {
        self.0 >= SPECS.len()
    }

    pub fn name(self) -> &'static str {
        self.spec().name
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::all().find(|language| language.name() == name)
    }

    /// Detects the language from a file name or extension (case-insensitive). User languages
    /// take precedence.
    pub fn from_path(path: &Path) -> Self {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let user = SPECS.len()..SPECS.len() + user_spec_count();
        let ordered = || user.clone().chain(0..SPECS.len()).map(Self);
        ordered()
            .find(|language| language.spec().file_names.contains(&file_name.as_ref()))
            .or_else(|| {
                ordered().find(|language| language.spec().extensions.contains(&extension.as_str()))
            })
            .unwrap_or(Self::PLAIN_TEXT)
    }

    /// Prefix for commenting out a line, if the language has line comments.
    pub fn line_comment(self) -> Option<&'static str> {
        self.spec().line_comments.first().copied()
    }

    /// Opening and closing marker of a block comment, if the language has them.
    pub fn block_comment(self) -> Option<(&'static str, &'static str)> {
        match self.spec().mode {
            Mode::Markup | Mode::Markdown => Some(("<!--", "-->")),
            _ => self.spec().block_comment,
        }
    }

    /// Keywords and built-in type names, e.g. for auto-completion.
    pub fn keywords(self) -> impl Iterator<Item = &'static str> {
        let spec = self.spec();
        spec.keywords.iter().chain(spec.types).copied()
    }

    fn spec(self) -> &'static Spec {
        SPECS.get(self.0).unwrap_or_else(|| {
            USER_SPECS
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .get(self.0 - SPECS.len())
                .copied()
                .unwrap_or(&SPECS[0])
        })
    }
}

/// Lexes one line (without its `\n`). Tokens are only collected if `tokens` is given;
/// without it only the end state is computed, which is faster.
pub fn highlight_line(
    language: Language,
    line: &str,
    state: LineState,
    tokens: Option<&mut Vec<Token>>,
) -> LineState {
    let spec = language.spec();
    if language == Language::PLAIN_TEXT {
        return LineState::Normal;
    }
    let chars: Vec<char> = line.chars().collect();
    let mut lexer = Lexer {
        spec,
        chars: &chars,
        tokens,
    };
    match spec.mode {
        Mode::Code => lexer.code(state),
        Mode::Markup => lexer.markup(state),
        Mode::Markdown => lexer.markdown(state),
        Mode::Diff => lexer.diff(),
    }
}

struct Lexer<'a> {
    spec: &'static Spec,
    chars: &'a [char],
    tokens: Option<&'a mut Vec<Token>>,
}

impl Lexer<'_> {
    fn push(&mut self, start: usize, end: usize, kind: TokenKind) {
        if let Some(tokens) = &mut self.tokens
            && start < end
        {
            tokens.push(Token { start, end, kind });
        }
    }

    fn starts_with(&self, at: usize, pattern: &str) -> bool {
        let mut i = at;
        for expected in pattern.chars() {
            match self.chars.get(i) {
                Some(&ch) if ch == expected => i += 1,
                Some(&ch) if self.spec.ignore_case && ch.eq_ignore_ascii_case(&expected) => i += 1,
                _ => return false,
            }
        }
        true
    }

    fn find(&self, from: usize, pattern: &str) -> Option<usize> {
        (from..self.chars.len()).find(|&i| self.starts_with(i, pattern))
    }

    fn first_non_blank(&self) -> usize {
        self.chars
            .iter()
            .position(|c| !c.is_whitespace())
            .unwrap_or(self.chars.len())
    }

    fn is_ident(&self, ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_' || self.spec.ident_extra.contains(&ch)
    }

    fn next_non_blank(&self, from: usize) -> Option<char> {
        self.chars[from..]
            .iter()
            .copied()
            .find(|c| !c.is_whitespace())
    }

    /// End (exclusive) of a string whose content starts at `from`, if it ends on this line.
    fn string_end(&self, from: usize, quote: char, triple: bool) -> Option<usize> {
        let mut i = from;
        while i < self.chars.len() {
            let ch = self.chars[i];
            if self.spec.escapes && ch == '\\' {
                i += 2;
                continue;
            }
            if ch == quote {
                if !triple {
                    return Some(i + 1);
                }
                if self.chars.get(i + 1) == Some(&quote) && self.chars.get(i + 2) == Some(&quote) {
                    return Some(i + 3);
                }
            }
            i += 1;
        }
        None
    }

    fn code(&mut self, state: LineState) -> LineState {
        let len = self.chars.len();
        let mut i = 0;
        match state {
            LineState::BlockComment => {
                let (_, close) = self
                    .spec
                    .block_comment
                    .expect("only set with block comments");
                match self.find(0, close) {
                    Some(end) => {
                        i = end + close.chars().count();
                        self.push(0, i, TokenKind::Comment);
                    }
                    None => {
                        self.push(0, len, TokenKind::Comment);
                        return state;
                    }
                }
            }
            LineState::String { quote, triple } => match self.string_end(0, quote, triple) {
                Some(end) => {
                    self.push_string(0, end, quote);
                    i = end;
                }
                None => {
                    self.push_string(0, len, quote);
                    return state;
                }
            },
            LineState::RawString { hashes } => match self.raw_string_end(0, hashes) {
                Some(end) => {
                    self.push(0, end, TokenKind::String);
                    i = end;
                }
                None => {
                    self.push(0, len, TokenKind::String);
                    return state;
                }
            },
            _ => {}
        }

        let first = self.first_non_blank();
        if i == 0 && first < len {
            if self.spec.section_headers && self.chars[first] == '[' {
                self.push(first, len, TokenKind::Keyword);
                return LineState::Normal;
            }
            let is_rust_attribute =
                self.spec.lifetimes && matches!(self.chars.get(first + 1), Some('[' | '!'));
            if self.spec.line_meta == Some(self.chars[first])
                && (!self.spec.lifetimes || is_rust_attribute)
            {
                self.push(first, len, TokenKind::Meta);
                return LineState::Normal;
            }
        }

        while i < len {
            let ch = self.chars[i];
            if let Some((open, close)) = self.spec.block_comment
                && self.starts_with(i, open)
            {
                let from = i + open.chars().count();
                match self.find(from, close) {
                    Some(end) => {
                        let end = end + close.chars().count();
                        self.push(i, end, TokenKind::Comment);
                        i = end;
                        continue;
                    }
                    None => {
                        self.push(i, len, TokenKind::Comment);
                        return LineState::BlockComment;
                    }
                }
            }
            if self
                .spec
                .line_comments
                .iter()
                .any(|c| self.starts_with(i, c))
            {
                self.push(i, len, TokenKind::Comment);
                break;
            }
            if self.spec.rust_syntax
                && let Some((open_end, hashes)) = self.raw_string_start(i)
            {
                match self.raw_string_end(open_end, hashes) {
                    Some(end) => {
                        self.push(i, end, TokenKind::String);
                        i = end;
                        continue;
                    }
                    None => {
                        self.push(i, len, TokenKind::String);
                        return LineState::RawString { hashes };
                    }
                }
            }
            if self.spec.regex_literals
                && ch == '/'
                && self.regex_allowed(i)
                && let Some(end) = self.regex_end(i + 1)
            {
                self.push(i, end, TokenKind::String);
                i = end;
                continue;
            }
            if self.spec.quotes.contains(&ch) {
                if self.spec.lifetimes && ch == '\'' && self.is_lifetime(i) {
                    i += 1;
                    continue;
                }
                let triple = self.spec.triple_quotes
                    && self.chars.get(i + 1) == Some(&ch)
                    && self.chars.get(i + 2) == Some(&ch);
                let start = i;
                let content = i + if triple { 3 } else { 1 };
                match self.string_end(content, ch, triple) {
                    Some(end) => {
                        let is_key =
                            self.spec.keys == Keys::Quoted && self.next_non_blank(end) == Some(':');
                        if is_key {
                            self.push(start, end, TokenKind::Type);
                        } else {
                            self.push_string(start, end, ch);
                        }
                        i = end;
                        continue;
                    }
                    None => {
                        self.push_string(start, len, ch);
                        if triple || self.spec.multiline_quotes.contains(&ch) {
                            return LineState::String { quote: ch, triple };
                        }
                        break;
                    }
                }
            }
            if ch.is_ascii_digit()
                || (ch == '.' && self.chars.get(i + 1).is_some_and(char::is_ascii_digit))
            {
                let start = i;
                let hex = ch == '0' && matches!(self.chars.get(i + 1), Some('x' | 'X'));
                while i < len {
                    let c = self.chars[i];
                    let exponent_sign = matches!(c, '+' | '-')
                        && !hex
                        && matches!(self.chars[i - 1], 'e' | 'E')
                        && self.chars.get(i + 1).is_some_and(char::is_ascii_digit);
                    if !(c.is_alphanumeric() || matches!(c, '.' | '_') || exponent_sign) {
                        break;
                    }
                    i += 1;
                }
                if !(start > 0 && self.is_ident(self.chars[start - 1])) {
                    self.push(start, i, TokenKind::Number);
                }
                continue;
            }
            let is_meta = self.spec.meta_prefixes.contains(&ch);
            if self.is_ident(ch) || is_meta {
                let start = i;
                i += 1;
                while i < len && self.is_ident(self.chars[i]) {
                    i += 1;
                }
                if self.tokens.is_some() {
                    let kind = if is_meta {
                        (i > start + 1 || self.is_ident(ch)).then_some(TokenKind::Meta)
                    } else {
                        self.word_kind(start, i, first)
                    };
                    let is_macro = self.spec.rust_syntax
                        && kind.is_none()
                        && self.chars.get(i) == Some(&'!')
                        && matches!(self.chars.get(i + 1), Some('(' | '[' | '{'));
                    if is_macro {
                        i += 1;
                        self.push(start, i, TokenKind::Function);
                    } else if let Some(kind) = kind {
                        self.push(start, i, kind);
                    }
                }
                continue;
            }
            i += 1;
        }
        LineState::Normal
    }

    /// For `r"`, `r#"`, `br"` … at `at`: where the content starts and the number of `#`.
    fn raw_string_start(&self, at: usize) -> Option<(usize, u8)> {
        if at > 0 && self.is_ident(self.chars[at - 1]) {
            return None;
        }
        let r = match self.chars[at] {
            'r' => at,
            'b' if self.chars.get(at + 1) == Some(&'r') => at + 1,
            _ => return None,
        };
        let hashes = self.chars[r + 1..]
            .iter()
            .take_while(|&&c| c == '#')
            .count();
        let quote = r + 1 + hashes;
        (self.chars.get(quote) == Some(&'"'))
            .then(|| u8::try_from(hashes).ok().map(|h| (quote + 1, h)))
            .flatten()
    }

    fn raw_string_end(&self, from: usize, hashes: u8) -> Option<usize> {
        let hashes = usize::from(hashes);
        (from..self.chars.len())
            .find(|&i| {
                self.chars[i] == '"'
                    && self.chars.len() > i + hashes
                    && self.chars[i + 1..=i + hashes].iter().all(|&c| c == '#')
            })
            .map(|i| i + 1 + hashes)
    }

    /// Whether a `/` at `at` can start a regex: after an operator, an opening bracket or
    /// a keyword like `return`, not after a value.
    fn regex_allowed(&self, at: usize) -> bool {
        let Some(before) = self.chars[..at].iter().rposition(|c| !c.is_whitespace()) else {
            return true;
        };
        let ch = self.chars[before];
        if self.is_ident(ch) {
            let start = self.chars[..=before]
                .iter()
                .rposition(|&c| !self.is_ident(c))
                .map_or(0, |p| p + 1);
            let word: String = self.chars[start..=before].iter().collect();
            return BEFORE_REGEX.contains(&word.as_str());
        }
        "(,=:[!&|?{};+-*%<>~^".contains(ch)
    }

    /// End (exclusive) of a regex literal whose body starts at `from`, including flags.
    fn regex_end(&self, from: usize) -> Option<usize> {
        let limit = self.chars.len().min(from + MAX_SPAN);
        let mut in_class = false;
        let mut i = from;
        while i < limit {
            match self.chars[i] {
                '\\' => i += 1,
                '[' => in_class = true,
                ']' => in_class = false,
                '/' if !in_class => {
                    i += 1;
                    while i < self.chars.len() && self.chars[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    return Some(i);
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// Pushes a string from `start` to `end`, with escapes and interpolations as own tokens.
    fn push_string(&mut self, start: usize, end: usize, quote: char) {
        if self.tokens.is_none() {
            return;
        }
        let interpolate = self.spec.interpolation.contains(&quote);
        let mut run = start;
        let mut i = start;
        while i < end {
            let piece = match self.chars[i] {
                '\\' if self.spec.escapes => Some((self.escape_end(i).min(end), TokenKind::Escape)),
                '$' if interpolate => self.interpolation_end(i, end).map(|e| (e, TokenKind::Meta)),
                _ => None,
            };
            match piece {
                Some((piece_end, kind)) => {
                    self.push(run, i, TokenKind::String);
                    self.push(i, piece_end, kind);
                    i = piece_end;
                    run = piece_end;
                }
                None => i += 1,
            }
        }
        self.push(run, end, TokenKind::String);
    }

    /// End of the escape sequence starting with the backslash at `at`.
    fn escape_end(&self, at: usize) -> usize {
        let after = at + 2;
        let count = |from: usize, max: usize, valid: fn(&char) -> bool| {
            from + self.chars[from.min(self.chars.len())..]
                .iter()
                .take(max)
                .take_while(|c| valid(c))
                .count()
        };
        match self.chars.get(at + 1) {
            Some('x') => count(after, 2, char::is_ascii_hexdigit),
            Some('u') if self.chars.get(after) == Some(&'{') => {
                let close = count(after + 1, 8, char::is_ascii_hexdigit);
                if self.chars.get(close) == Some(&'}') {
                    close + 1
                } else {
                    close
                }
            }
            Some('u') => count(after, 4, char::is_ascii_hexdigit),
            Some('U') => count(after, 8, char::is_ascii_hexdigit),
            Some('0'..='7') => count(after, 2, |c| matches!(c, '0'..='7')),
            _ => after,
        }
    }

    /// End of `$name` or `${…}` at `at`, if it is one and ends before `end`.
    fn interpolation_end(&self, at: usize, end: usize) -> Option<usize> {
        let next = *self.chars.get(at + 1)?;
        if next == '{' {
            let limit = end.min(at + MAX_SPAN);
            return (at + 2..limit)
                .find(|&i| self.chars[i] == '}')
                .map(|i| i + 1);
        }
        if !(next.is_alphabetic() || next == '_') {
            return None;
        }
        let mut i = at + 2;
        while i < end && (self.chars[i].is_alphanumeric() || self.chars[i] == '_') {
            i += 1;
        }
        Some(i)
    }

    fn is_lifetime(&self, at: usize) -> bool {
        let next = self.chars.get(at + 1).copied();
        next.is_some_and(|c| c.is_alphabetic() || c == '_')
            && self.chars.get(at + 2) != Some(&'\'')
            && !(next == Some('\\'))
    }

    fn word_kind(&self, start: usize, end: usize, first: usize) -> Option<TokenKind> {
        let is_key = match self.spec.keys {
            Keys::BeforeEquals => start == first && self.next_non_blank(end) == Some('='),
            Keys::BeforeColon => {
                let line_start = start == first || (first + 2 == start && self.chars[first] == '-');
                line_start && self.chars.get(end) == Some(&':')
            }
            _ => false,
        };
        if is_key {
            return Some(TokenKind::Type);
        }
        let word: String = self.chars[start..end].iter().collect();
        let matches = |list: &[&str]| {
            if self.spec.ignore_case {
                list.iter().any(|k| k.eq_ignore_ascii_case(&word))
            } else {
                list.contains(&word.as_str())
            }
        };
        if matches(self.spec.keywords) {
            Some(TokenKind::Keyword)
        } else if matches(self.spec.types) {
            Some(TokenKind::Type)
        } else if self.follows_definer(start)
            || (self.mode_is_code() && self.chars.get(end) == Some(&'('))
        {
            Some(TokenKind::Function)
        } else if self.spec.pascal_types
            && word.starts_with(|c: char| c.is_uppercase())
            && word.chars().any(char::is_lowercase)
        {
            Some(TokenKind::Type)
        } else {
            None
        }
    }

    fn mode_is_code(&self) -> bool {
        self.spec.mode == Mode::Code && self.spec.keys == Keys::None
    }

    /// Whether the word before `start` is a keyword like `fn` or `def`.
    fn follows_definer(&self, start: usize) -> bool {
        let Some(end) = self.chars[..start].iter().rposition(|c| !c.is_whitespace()) else {
            return false;
        };
        if end + 1 == start {
            return false;
        }
        let word_start = self.chars[..=end]
            .iter()
            .rposition(|&c| !self.is_ident(c))
            .map_or(0, |p| p + 1);
        let word: String = self.chars[word_start..=end].iter().collect();
        DEFINERS.iter().any(|d| d.eq_ignore_ascii_case(&word))
            && self
                .spec
                .keywords
                .iter()
                .any(|k| k.eq_ignore_ascii_case(&word))
    }

    fn markup(&mut self, mut state: LineState) -> LineState {
        let len = self.chars.len();
        let mut i = 0;
        if state == LineState::BlockComment {
            match self.find(0, "-->") {
                Some(end) => {
                    i = end + 3;
                    self.push(0, i, TokenKind::Comment);
                    state = LineState::Normal;
                }
                None => {
                    self.push(0, len, TokenKind::Comment);
                    return state;
                }
            }
        }
        while i < len {
            if let LineState::Embedded { css, inner } = state {
                let close = if css { "</style" } else { "</script" };
                let end = self.find_ignore_case(i, close).unwrap_or(len);
                let inner = self.embedded(i, end, css, inner);
                if end == len {
                    return LineState::Embedded { css, inner };
                }
                state = LineState::Normal;
                i = end;
                continue;
            }
            let ch = self.chars[i];
            if matches!(state, LineState::Tag | LineState::EmbedTag { .. }) {
                match ch {
                    '>' => {
                        self.push(i, i + 1, TokenKind::Keyword);
                        state = match state {
                            LineState::EmbedTag { css } => LineState::Embedded {
                                css,
                                inner: InnerState::Normal,
                            },
                            _ => LineState::Normal,
                        };
                        i += 1;
                    }
                    '/' | '?' if self.chars.get(i + 1) == Some(&'>') => {
                        self.push(i, i + 2, TokenKind::Keyword);
                        state = LineState::Normal;
                        i += 2;
                    }
                    '"' | '\'' => {
                        let end = self.string_end(i + 1, ch, false).unwrap_or(len);
                        self.push(i, end, TokenKind::String);
                        i = end;
                    }
                    c if c.is_alphanumeric() => {
                        let start = i;
                        while i < len
                            && (self.chars[i].is_alphanumeric()
                                || matches!(self.chars[i], '-' | ':' | '_' | '.'))
                        {
                            i += 1;
                        }
                        self.push(start, i, TokenKind::Type);
                    }
                    _ => i += 1,
                }
                continue;
            }
            if self.starts_with(i, "<!--") {
                match self.find(i + 4, "-->") {
                    Some(end) => {
                        self.push(i, end + 3, TokenKind::Comment);
                        i = end + 3;
                        continue;
                    }
                    None => {
                        self.push(i, len, TokenKind::Comment);
                        return LineState::BlockComment;
                    }
                }
            }
            if ch == '<'
                && self
                    .chars
                    .get(i + 1)
                    .is_some_and(|c| c.is_alphabetic() || matches!(c, '/' | '!' | '?'))
            {
                let start = i;
                i += 2;
                while i < len
                    && (self.chars[i].is_alphanumeric()
                        || matches!(self.chars[i], '-' | ':' | '_' | '.'))
                {
                    i += 1;
                }
                self.push(start, i, TokenKind::Keyword);
                let name: String = self.chars[start + 1..i].iter().collect();
                state = match name.to_ascii_lowercase().as_str() {
                    "script" if self.spec.embeds => LineState::EmbedTag { css: false },
                    "style" if self.spec.embeds => LineState::EmbedTag { css: true },
                    _ => LineState::Tag,
                };
                continue;
            }
            if ch == '&'
                && let Some(end) = (i + 1..len.min(i + 12)).find(|&j| self.chars[j] == ';')
            {
                self.push(i, end + 1, TokenKind::Number);
                i = end + 1;
                continue;
            }
            i += 1;
        }
        state
    }

    fn find_ignore_case(&self, from: usize, pattern: &str) -> Option<usize> {
        let pattern: Vec<char> = pattern.chars().collect();
        (from..self.chars.len()).find(|&i| {
            self.chars[i..]
                .iter()
                .zip(&pattern)
                .filter(|(a, b)| a.eq_ignore_ascii_case(b))
                .count()
                == pattern.len()
        })
    }

    /// Lexes `start..end` as JavaScript or CSS, continuing from `inner`.
    fn embedded(&mut self, start: usize, end: usize, css: bool, inner: InnerState) -> InnerState {
        let name = if css { "CSS" } else { "JavaScript" };
        let spec = SPECS.iter().find(|s| s.name == name).unwrap_or(&SPECS[0]);
        let mut tokens = Vec::new();
        let mut lexer = Lexer {
            spec,
            chars: &self.chars[start..end],
            tokens: self.tokens.is_some().then_some(&mut tokens),
        };
        let state = lexer.code(inner.line_state());
        for token in tokens {
            self.push(token.start + start, token.end + start, token.kind);
        }
        InnerState::from_line_state(state)
    }

    fn markdown(&mut self, state: LineState) -> LineState {
        let len = self.chars.len();
        let first = self.first_non_blank();
        let fence = self.starts_with(first, "```") || self.starts_with(first, "~~~");
        if state == LineState::Fence {
            self.push(0, len, TokenKind::String);
            return if fence { LineState::Normal } else { state };
        }
        if fence {
            self.push(0, len, TokenKind::String);
            return LineState::Fence;
        }
        match self.chars.get(first) {
            Some('#') => {
                self.push(first, len, TokenKind::Keyword);
                return LineState::Normal;
            }
            Some('>') => {
                self.push(first, len, TokenKind::Comment);
                return LineState::Normal;
            }
            Some('-' | '*' | '+') if self.chars.get(first + 1) == Some(&' ') => {
                self.push(first, first + 1, TokenKind::Meta);
            }
            Some('0'..='9') => {
                let digits = self.chars[first..]
                    .iter()
                    .take_while(|c| c.is_ascii_digit())
                    .count();
                let marker = first + digits;
                if matches!(self.chars.get(marker), Some('.' | ')'))
                    && self.chars.get(marker + 1) == Some(&' ')
                {
                    self.push(first, marker + 1, TokenKind::Meta);
                }
            }
            _ => {}
        }
        let mut i = first;
        while i < len {
            let limit = len.min(i + MAX_SPAN);
            if self.chars[i] == '`'
                && let Some(end) = (i + 1..limit).find(|&j| self.chars[j] == '`')
            {
                self.push(i, end + 1, TokenKind::String);
                i = end + 1;
                continue;
            }
            if let Some((end, kind)) = self.emphasis(i, limit) {
                self.push(i, end, kind);
                i = end;
                continue;
            }
            if self.chars[i] == '['
                && let Some(close) = (i + 1..limit).find(|&j| self.chars[j] == ']')
                && self.chars.get(close + 1) == Some(&'(')
                && let Some(paren) = (close + 2..limit).find(|&j| self.chars[j] == ')')
            {
                self.push(i, close + 1, TokenKind::Meta);
                self.push(close + 1, paren + 1, TokenKind::String);
                i = paren + 1;
                continue;
            }
            i += 1;
        }
        LineState::Normal
    }

    /// `**strong**` or `__strong__` (keyword color), `*emphasis*` or `_emphasis_` (type
    /// color) starting at `at`, ending before `limit`.
    fn emphasis(&self, at: usize, limit: usize) -> Option<(usize, TokenKind)> {
        let marker = self.chars[at];
        if !matches!(marker, '*' | '_') || (at > 0 && self.is_ident(self.chars[at - 1])) {
            return None;
        }
        let width = if self.chars.get(at + 1) == Some(&marker) {
            2
        } else {
            1
        };
        let content = at + width;
        if self
            .chars
            .get(content)
            .is_none_or(|c| c.is_whitespace() || *c == marker)
        {
            return None;
        }
        let close = (content + 1..limit.saturating_sub(width - 1)).find(|&j| {
            self.chars[j..j + width].iter().all(|&c| c == marker)
                && !self.chars[j - 1].is_whitespace()
                && !(marker == '_' && self.chars.get(j + width).is_some_and(|&c| self.is_ident(c)))
        })?;
        let kind = if width == 2 {
            TokenKind::Keyword
        } else {
            TokenKind::Type
        };
        Some((close + width, kind))
    }

    fn diff(&mut self) -> LineState {
        let len = self.chars.len();
        let kind = if self.starts_with(0, "+++")
            || self.starts_with(0, "---")
            || self.starts_with(0, "@@")
        {
            Some(TokenKind::Meta)
        } else {
            match self.chars.first() {
                Some('+') => Some(TokenKind::Type),
                Some('-') => Some(TokenKind::String),
                _ => None,
            }
        };
        if let Some(kind) = kind {
            self.push(0, len, kind);
        }
        LineState::Normal
    }
}

fn line_text(rope: &Rope, line: usize) -> Cow<'_, str> {
    let slice = rope.line(line);
    let text = match slice.as_str() {
        Some(text) => Cow::Borrowed(text),
        None => Cow::Owned(slice.to_string()),
    };
    match text {
        Cow::Borrowed(text) => Cow::Borrowed(text.strip_suffix('\n').unwrap_or(text)),
        Cow::Owned(mut text) => {
            if text.ends_with('\n') {
                text.pop();
            }
            Cow::Owned(text)
        }
    }
}

/// Caches the lexer state at the start of every line up to the furthest line requested.
#[derive(Clone, Debug, Default)]
pub struct Highlighter {
    language: Language,
    states: Vec<LineState>,
    revision: u64,
}

impl Highlighter {
    pub fn new(language: Language) -> Self {
        Self {
            language,
            ..Self::default()
        }
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn set_language(&mut self, language: Language) {
        if language != self.language {
            *self = Self::new(language);
        }
    }

    /// Forgets cached states, e.g. after the definition of a user language changed.
    pub fn reset(&mut self) {
        *self = Self::new(self.language);
    }

    /// Computes line start states up to `line`, lexing at most `budget` lines.
    /// Returns `false` if the budget ran out; tokens are then approximate until called again.
    pub fn prepare(&mut self, doc: &Document, line: usize, budget: usize) -> bool {
        if self.language == Language::PLAIN_TEXT {
            return true;
        }
        let rope = doc.text();
        if let Some(changed) = doc.first_change_since(self.revision) {
            let changed_line = rope.char_to_line(changed.min(rope.len_chars()));
            self.states.truncate(changed_line + 1);
        }
        self.revision = doc.revision();
        if self.states.is_empty() {
            self.states.push(LineState::Normal);
        }
        let target = line.min(rope.len_lines() - 1);
        for _ in 0..budget {
            if self.states.len() > target {
                return true;
            }
            let line = self.states.len() - 1;
            // Huge lines (minified files) are not lexed as a whole; they end in a neutral state.
            let state = if rope.line(line).len_chars() > HUGE_LINE {
                LineState::Normal
            } else {
                highlight_line(
                    self.language,
                    &line_text(rope, line),
                    self.states[line],
                    None,
                )
            };
            self.states.push(state);
        }
        self.states.len() > target
    }

    /// Tokens of `line`, using the cached start state (call [`Self::prepare`] first).
    pub fn tokens(&self, doc: &Document, line: usize) -> Vec<Token> {
        self.tokens_in(doc, line, 0..usize::MAX)
    }

    /// Tokens of `line` covering at least the char columns `columns`. Lines longer than
    /// [`HUGE_LINE`] are only lexed around `columns`, starting in a neutral state there.
    pub fn tokens_in(&self, doc: &Document, line: usize, columns: Range<usize>) -> Vec<Token> {
        let mut tokens = Vec::new();
        if self.language == Language::PLAIN_TEXT {
            return tokens;
        }
        let slice = doc.text().line(line);
        let len = slice.len_chars();
        if len <= HUGE_LINE {
            let state = self.states.get(line).copied().unwrap_or_default();
            let text = line_text(doc.text(), line);
            highlight_line(self.language, &text, state, Some(&mut tokens));
            return tokens;
        }
        let end = columns.end.saturating_add(HUGE_LINE_MARGIN).min(len);
        let start = columns.start.saturating_sub(HUGE_LINE_MARGIN).min(end);
        let text: String = slice
            .slice(start..end)
            .chars()
            .filter(|&c| c != '\n')
            .collect();
        highlight_line(self.language, &text, LineState::Normal, Some(&mut tokens));
        for token in &mut tokens {
            token.start += start;
            token.end += start;
        }
        tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(name: &str) -> Language {
        Language::from_name(name).unwrap()
    }

    fn lex(
        language: &str,
        line: &str,
        state: LineState,
    ) -> (Vec<(std::string::String, TokenKind)>, LineState) {
        let mut tokens = Vec::new();
        let end = highlight_line(lang(language), line, state, Some(&mut tokens));
        let chars: Vec<char> = line.chars().collect();
        let tokens = tokens
            .into_iter()
            .map(|t| (chars[t.start..t.end].iter().collect(), t.kind))
            .collect();
        (tokens, end)
    }

    fn tokens(language: &str, line: &str) -> Vec<(std::string::String, TokenKind)> {
        lex(language, line, LineState::Normal).0
    }

    fn t(text: &str, kind: TokenKind) -> (std::string::String, TokenKind) {
        (text.to_owned(), kind)
    }

    use TokenKind::*;

    #[test]
    fn every_language_has_a_unique_name() {
        let names: Vec<_> = Language::all().map(Language::name).collect();
        for name in &names {
            assert_eq!(names.iter().filter(|n| *n == name).count(), 1, "{name}");
            assert_eq!(Language::from_name(name).map(Language::name), Some(*name));
        }
    }

    #[test]
    fn detects_language_from_path() {
        assert_eq!(Language::from_path(Path::new("/a/main.RS")).name(), "Rust");
        assert_eq!(
            Language::from_path(Path::new("Makefile")).name(),
            "Makefile"
        );
        assert_eq!(
            Language::from_path(Path::new("x.unknown")),
            Language::PLAIN_TEXT
        );
        assert_eq!(Language::from_path(Path::new("Cargo.lock")).name(), "TOML");
    }

    #[test]
    fn highlights_user_languages() {
        set_user_languages(&[UserLanguage {
            name: "CuneiEdit Test".into(),
            extensions: vec![".ptest".into()],
            keywords: vec!["when".into()],
            types: vec!["Num".into()],
            line_comment: "%%".into(),
            block_comment: Some(("{-".into(), "-}".into())),
            quotes: "\"".into(),
            ignore_case: true,
        }]);
        let language = Language::from_path(Path::new("a.PTEST"));
        assert_eq!(language.name(), "CuneiEdit Test");
        assert!(language.is_user_defined());
        assert!(!Language::from_name("Rust").unwrap().is_user_defined());
        assert_eq!(
            tokens("CuneiEdit Test", "WHEN num = \"s\" {- x -} %% c"),
            vec![
                t("WHEN", Keyword),
                t("num", Type),
                t("\"s\"", String),
                t("{- x -}", Comment),
                t("%% c", Comment)
            ]
        );
    }

    #[test]
    fn lexes_rust() {
        assert_eq!(
            tokens("Rust", r#"let s: &'a str = "x\"y"; // done"#),
            vec![
                t("let", Keyword),
                t("str", Type),
                t("\"x", String),
                t("\\\"", Escape),
                t("y\"", String),
                t("// done", Comment)
            ]
        );
        assert_eq!(
            tokens("Rust", "#[derive(Debug)]"),
            vec![t("#[derive(Debug)]", Meta)]
        );
        assert_eq!(
            tokens("Rust", "let c = 'x'; 0x1F 1e-3"),
            vec![
                t("let", Keyword),
                t("'x'", String),
                t("0x1F", Number),
                t("1e-3", Number)
            ]
        );
        assert_eq!(
            tokens(
                "Rust",
                "fn run<T>(x: HashMap) { println!(\"\\u{1F600}\"); x.len() }"
            ),
            vec![
                t("fn", Keyword),
                t("run", Function),
                t("HashMap", Type),
                t("println!", Function),
                t("\"", String),
                t("\\u{1F600}", Escape),
                t("\"", String),
                t("len", Function)
            ]
        );
        let (first, state) = lex("Rust", r###"let r = br#"a "quoted""###, LineState::Normal);
        assert_eq!(
            first,
            vec![t("let", Keyword), t(r##"br#"a "quoted""##, String)]
        );
        assert_eq!(state, LineState::RawString { hashes: 1 });
        let (rest, state) = lex("Rust", r##"b"# + 1"##, state);
        assert_eq!(rest, vec![t(r##"b"#"##, String), t("1", Number)]);
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn lexes_interpolation_and_regexes() {
        assert_eq!(
            tokens("Shell", r#"echo "hi $USER ${HOME}\n" '$x'"#),
            vec![
                t("echo", Keyword),
                t("\"hi ", String),
                t("$USER", Meta),
                t(" ", String),
                t("${HOME}", Meta),
                t("\\n", Escape),
                t("\"", String),
                t("'$x'", String)
            ]
        );
        assert_eq!(
            tokens("JavaScript", "const r = /a\\/[/]b/gi; x = a / b / c"),
            vec![t("const", Keyword), t("/a\\/[/]b/gi", String)]
        );
        assert_eq!(
            tokens("TypeScript", "f(`${a}`)"),
            vec![
                t("f", Function),
                t("`", String),
                t("${a}", Meta),
                t("`", String)
            ]
        );
    }

    #[test]
    fn lexes_embedded_script_and_style() {
        let (first, state) = lex(
            "HTML",
            "<script type=\"module\">let a = 1;",
            LineState::Normal,
        );
        assert_eq!(
            first,
            vec![
                t("<script", Keyword),
                t("type", Type),
                t("\"module\"", String),
                t(">", Keyword),
                t("let", Keyword),
                t("1", Number)
            ]
        );
        assert_eq!(
            state,
            LineState::Embedded {
                css: false,
                inner: InnerState::Normal
            }
        );
        let (second, state) = lex("HTML", "/* c */ f()</SCRIPT><style>", state);
        assert_eq!(
            second,
            vec![
                t("/* c */", Comment),
                t("f", Function),
                t("</SCRIPT", Keyword),
                t(">", Keyword),
                t("<style", Keyword),
                t(">", Keyword)
            ]
        );
        assert_eq!(
            state,
            LineState::Embedded {
                css: true,
                inner: InnerState::Normal
            }
        );
        let (third, state) = lex("HTML", "color: #fff; } </style>", state);
        assert_eq!(
            third,
            vec![
                t("color", Type),
                t("#fff", Meta),
                t("</style", Keyword),
                t(">", Keyword)
            ]
        );
        assert_eq!(state, LineState::Normal);
        let (_, state) = lex("XML", "<script>", LineState::Normal);
        assert_eq!(state, LineState::Normal, "only HTML embeds");
    }

    #[test]
    fn lexes_new_languages() {
        type Case<'a> = (&'a str, &'a str, &'a [(&'a str, TokenKind)]);
        let cases: &[Case<'_>] = &[
            (
                "Perl",
                "my $x = 1; # c",
                &[
                    ("my", Keyword),
                    ("$x", Meta),
                    ("1", Number),
                    ("# c", Comment),
                ],
            ),
            (
                "R",
                "x <- is.na(TRUE)",
                &[("is.na", Function), ("TRUE", Type)],
            ),
            (
                "Scala",
                "val m: Map = s\"$a\"",
                &[
                    ("val", Keyword),
                    ("Map", Type),
                    ("\"", String),
                    ("$a", Meta),
                    ("\"", String),
                ],
            ),
            (
                "Dart",
                "final List x = null;",
                &[("final", Keyword), ("List", Type), ("null", Type)],
            ),
            (
                "Haskell",
                "data Tree = Leaf -- c",
                &[
                    ("data", Keyword),
                    ("Tree", Type),
                    ("Leaf", Type),
                    ("-- c", Comment),
                ],
            ),
            (
                "Elixir",
                "defp go(:ok), do: nil",
                &[
                    ("defp", Keyword),
                    ("go", Function),
                    (":ok", Meta),
                    ("do", Keyword),
                    ("nil", Type),
                ],
            ),
            ("Erlang", "-module(m).", &[("-module(m).", Meta)]),
            (
                "Fortran",
                "IF (a .AND. .true.) THEN ! c",
                &[
                    ("IF", Keyword),
                    (".AND.", Keyword),
                    (".true.", Type),
                    ("THEN", Keyword),
                    ("! c", Comment),
                ],
            ),
            (
                "Pascal",
                "begin { c } x := 'a' end;",
                &[
                    ("begin", Keyword),
                    ("{ c }", Comment),
                    ("'a'", String),
                    ("end", Keyword),
                ],
            ),
            (
                "Visual Basic",
                "Dim s As String ' c",
                &[
                    ("Dim", Keyword),
                    ("As", Keyword),
                    ("String", Type),
                    ("' c", Comment),
                ],
            ),
            (
                "Assembly",
                "mov eax, 1 ; c",
                &[
                    ("mov", Keyword),
                    ("eax", Type),
                    ("1", Number),
                    ("; c", Comment),
                ],
            ),
            (
                "LaTeX",
                "\\section{A} $x$ % c",
                &[("\\section", Meta), ("$x$", String), ("% c", Comment)],
            ),
            (
                "CMake",
                "add_executable(app main.c)",
                &[("add_executable", Keyword)],
            ),
            (
                "Groovy",
                "def greet(name) { \"$name\" }",
                &[
                    ("def", Keyword),
                    ("greet", Function),
                    ("\"", String),
                    ("$name", Meta),
                    ("\"", String),
                ],
            ),
            (
                "Julia",
                "function f(x) #= c =#",
                &[("function", Keyword), ("f", Function), ("#= c =#", Comment)],
            ),
            (
                "Objective-C",
                "@interface A : NSObject",
                &[("@interface", Meta), ("NSObject", Type)],
            ),
            (
                "Lisp",
                "(defun add-1 (x) ; c",
                &[("defun", Keyword), ("add-1", Function), ("; c", Comment)],
            ),
            (
                "F#",
                "let f x = (* c *) 1",
                &[("let", Keyword), ("(* c *)", Comment), ("1", Number)],
            ),
            (
                "OCaml",
                "let rec f = None",
                &[("let", Keyword), ("rec", Keyword), ("None", Type)],
            ),
            (
                "Ada",
                "procedure Main is -- c",
                &[
                    ("procedure", Keyword),
                    ("Main", Function),
                    ("is", Keyword),
                    ("-- c", Comment),
                ],
            ),
            (
                "Tcl",
                "proc p {} { puts \"$a\" }",
                &[
                    ("proc", Keyword),
                    ("p", Function),
                    ("puts", Keyword),
                    ("\"", String),
                    ("$a", Meta),
                    ("\"", String),
                ],
            ),
            (
                "MATLAB",
                "function y = f(x) % c",
                &[
                    ("function", Keyword),
                    ("y", Function),
                    ("f", Function),
                    ("% c", Comment),
                ],
            ),
            (
                "Verilog",
                "module m(input wire a); `define X",
                &[
                    ("module", Keyword),
                    ("m", Function),
                    ("input", Keyword),
                    ("wire", Type),
                    ("`define", Meta),
                ],
            ),
            (
                "VHDL",
                "signal s : STD_LOGIC; -- c",
                &[("signal", Keyword), ("STD_LOGIC", Type), ("-- c", Comment)],
            ),
            (
                "GraphQL",
                "query User($id: ID) # c",
                &[
                    ("query", Keyword),
                    ("User", Function),
                    ("$id", Meta),
                    ("ID", Type),
                    ("# c", Comment),
                ],
            ),
            (
                "Protocol Buffers",
                "message User { int32 a = 1; }",
                &[
                    ("message", Keyword),
                    ("User", Type),
                    ("int32", Type),
                    ("1", Number),
                ],
            ),
            (
                "Zig",
                "pub fn main() void {}",
                &[
                    ("pub", Keyword),
                    ("fn", Keyword),
                    ("main", Function),
                    ("void", Type),
                ],
            ),
            (
                "Nim",
                "proc p(x: int) #[ c ]#",
                &[
                    ("proc", Keyword),
                    ("p", Function),
                    ("int", Type),
                    ("#[ c ]#", Comment),
                ],
            ),
            (
                "D",
                "import std.stdio; void main() {}",
                &[("import", Keyword), ("void", Type), ("main", Function)],
            ),
            (
                "HCL",
                "count = \"${var.n}\"",
                &[
                    ("count", Type),
                    ("\"", String),
                    ("${var.n}", Meta),
                    ("\"", String),
                ],
            ),
            (
                "NSIS",
                "!include \"x\" ; c",
                &[("!include", Meta), ("\"x\"", String), ("; c", Comment)],
            ),
        ];
        for (language, line, expected) in cases {
            let expected: Vec<_> = expected.iter().map(|&(s, k)| t(s, k)).collect();
            assert_eq!(tokens(language, line), expected, "{language}: {line}");
        }
    }

    #[test]
    fn extensions_are_unique() {
        let mut seen = std::collections::HashMap::new();
        for spec in SPECS {
            for extension in spec.extensions.iter().chain(spec.file_names) {
                assert_eq!(*extension, extension.trim());
                if let Some(other) = seen.insert(*extension, spec.name) {
                    panic!("{extension} is used by {other} and {}", spec.name);
                }
            }
        }
    }

    #[test]
    fn pathological_lines_stay_fast() {
        let start = std::time::Instant::now();
        for (language, unit) in [
            ("JavaScript", "=/["),
            ("Markdown", " _a"),
            ("Shell", "\"${"),
            ("HTML", "<a "),
            ("Rust", "r#"),
        ] {
            let line = unit.repeat(20_000);
            let mut tokens = Vec::new();
            highlight_line(lang(language), &line, LineState::Normal, Some(&mut tokens));
        }
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    }

    #[test]
    fn continues_block_comments_and_strings() {
        let (first, state) = lex("C", "int a; /* start", LineState::Normal);
        assert_eq!(first, vec![t("int", Type), t("/* start", Comment)]);
        assert_eq!(state, LineState::BlockComment);
        let (second, state) = lex("C", "end */ return 1;", state);
        assert_eq!(
            second,
            vec![t("end */", Comment), t("return", Keyword), t("1", Number)]
        );
        assert_eq!(state, LineState::Normal);

        let (_, state) = lex("Python", "x = \"\"\"doc", LineState::Normal);
        assert_eq!(
            state,
            LineState::String {
                quote: '"',
                triple: true
            }
        );
        let (rest, state) = lex("Python", "more\"\"\" if", state);
        assert_eq!(rest, vec![t("more\"\"\"", String), t("if", Keyword)]);
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn single_line_strings_end_at_line_end() {
        let (_, state) = lex("C", "char *s = \"open", LineState::Normal);
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn lexes_keys_and_sections() {
        assert_eq!(
            tokens("JSON", r#"{"a": "b", "n": 1}"#),
            vec![
                t(r#""a""#, Type),
                t(r#""b""#, String),
                t(r#""n""#, Type),
                t("1", Number)
            ]
        );
        assert_eq!(tokens("TOML", "[package]"), vec![t("[package]", Keyword)]);
        assert_eq!(
            tokens("TOML", "name = \"x\" # c"),
            vec![t("name", Type), t("\"x\"", String), t("# c", Comment)]
        );
        assert_eq!(
            tokens("YAML", "- key: true"),
            vec![t("key", Type), t("true", Type)]
        );
    }

    #[test]
    fn ignores_case_where_needed() {
        assert_eq!(
            tokens("SQL", "Select x FROM t"),
            vec![t("Select", Keyword), t("FROM", Keyword)]
        );
        assert_eq!(tokens("Batch", "REM hello"), vec![t("REM hello", Comment)]);
    }

    #[test]
    fn lexes_markup() {
        let (line, state) = lex(
            "HTML",
            r#"<a href="x">&amp; <!-- c --></a>"#,
            LineState::Normal,
        );
        assert_eq!(
            line,
            vec![
                t("<a", Keyword),
                t("href", Type),
                t("\"x\"", String),
                t(">", Keyword),
                t("&amp;", Number),
                t("<!-- c -->", Comment),
                t("</a", Keyword),
                t(">", Keyword)
            ]
        );
        assert_eq!(state, LineState::Normal);
        let (_, state) = lex("XML", "<item id=\"1\"", LineState::Normal);
        assert_eq!(state, LineState::Tag);
    }

    #[test]
    fn lexes_markdown_and_diff() {
        assert_eq!(tokens("Markdown", "# Title"), vec![t("# Title", Keyword)]);
        assert_eq!(
            tokens("Markdown", "- use `code`"),
            vec![t("-", Meta), t("`code`", String)]
        );
        let (_, state) = lex("Markdown", "```rust", LineState::Normal);
        assert_eq!(state, LineState::Fence);
        assert_eq!(tokens("Diff", "+added"), vec![t("+added", Type)]);
    }

    #[test]
    fn highlighter_relexes_from_changed_line() {
        let mut doc = Document::new();
        doc.replace(0..0, "a\n/* x\ny */\nb");
        let mut highlighter = Highlighter::new(lang("C"));
        assert!(highlighter.prepare(&doc, 3, 100));
        assert_eq!(highlighter.tokens(&doc, 2)[0].kind, Comment);

        doc.replace(2..4, "");
        assert!(highlighter.prepare(&doc, 3, 100));
        assert!(
            highlighter.tokens(&doc, 2).is_empty(),
            "comment start was removed"
        );
    }

    #[test]
    fn highlighter_respects_budget() {
        let mut doc = Document::new();
        doc.replace(0..0, &"x\n".repeat(10));
        let mut highlighter = Highlighter::new(lang("C"));
        assert!(!highlighter.prepare(&doc, 10, 3));
        assert!(highlighter.prepare(&doc, 10, 100));
    }

    #[test]
    fn highlights_huge_lines_around_shown_columns() {
        let mut doc = Document::new();
        let huge = format!("/* {} */ int x;\nint y;", "a ".repeat(HUGE_LINE));
        doc.replace(0..0, &huge);
        let mut highlighter = Highlighter::new(lang("C"));
        assert!(highlighter.prepare(&doc, 1, 100));
        let end = doc.text().line(0).len_chars() - 1;
        let tokens = highlighter.tokens_in(&doc, 0, end - 10..end);
        assert!(
            tokens
                .iter()
                .all(|t| t.start + HUGE_LINE_MARGIN + 10 >= end)
        );
        assert!(
            tokens
                .iter()
                .any(|t| t.kind == Type && t.end == end - 3 && t.start == end - 6)
        );
        assert_eq!(highlighter.tokens(&doc, 1)[0].kind, Type);
    }
}
