//! Lightweight, dependency-free syntax highlighting.
//!
//! Each line is lexed on its own, starting from the state the previous line ended in
//! (e.g. inside a block comment). [`Highlighter`] caches these start states per line.

use std::borrow::Cow;
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
    /// Inside a markup tag, between its name and `>`.
    Tag,
    /// Inside a fenced Markdown code block.
    Fence,
}

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
        meta_prefixes: &['@', '$'],
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
            if self
                .spec
                .line_comments
                .iter()
                .any(|c| self.starts_with(i, c))
            {
                self.push(i, len, TokenKind::Comment);
                break;
            }
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
                        let kind = if is_key {
                            TokenKind::Type
                        } else {
                            TokenKind::String
                        };
                        self.push(start, end, kind);
                        i = end;
                        continue;
                    }
                    None => {
                        self.push(start, len, TokenKind::String);
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
                while i < len
                    && (self.chars[i].is_alphanumeric() || matches!(self.chars[i], '.' | '_'))
                {
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
                        Some(TokenKind::Meta)
                    } else {
                        self.word_kind(start, i, first)
                    };
                    if let Some(kind) = kind {
                        self.push(start, i, kind);
                    }
                }
                continue;
            }
            i += 1;
        }
        LineState::Normal
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
        } else {
            None
        }
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
            let ch = self.chars[i];
            if state == LineState::Tag {
                match ch {
                    '>' => {
                        self.push(i, i + 1, TokenKind::Keyword);
                        state = LineState::Normal;
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
                state = LineState::Tag;
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
            _ => {}
        }
        let mut i = first;
        while i < len {
            if self.chars[i] == '`'
                && let Some(end) = (i + 1..len).find(|&j| self.chars[j] == '`')
            {
                self.push(i, end + 1, TokenKind::String);
                i = end + 1;
                continue;
            }
            i += 1;
        }
        LineState::Normal
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
            let state = highlight_line(
                self.language,
                &line_text(rope, line),
                self.states[line],
                None,
            );
            self.states.push(state);
        }
        self.states.len() > target
    }

    /// Tokens of `line`, using the cached start state (call [`Self::prepare`] first).
    pub fn tokens(&self, doc: &Document, line: usize) -> Vec<Token> {
        let mut tokens = Vec::new();
        if self.language != Language::PLAIN_TEXT {
            let state = self.states.get(line).copied().unwrap_or_default();
            highlight_line(
                self.language,
                &line_text(doc.text(), line),
                state,
                Some(&mut tokens),
            );
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
            name: "Pilcrow Test".into(),
            extensions: vec![".ptest".into()],
            keywords: vec!["when".into()],
            types: vec!["Num".into()],
            line_comment: "%%".into(),
            block_comment: Some(("{-".into(), "-}".into())),
            quotes: "\"".into(),
            ignore_case: true,
        }]);
        let language = Language::from_path(Path::new("a.PTEST"));
        assert_eq!(language.name(), "Pilcrow Test");
        assert!(language.is_user_defined());
        assert!(!Language::from_name("Rust").unwrap().is_user_defined());
        assert_eq!(
            tokens("Pilcrow Test", "WHEN num = \"s\" {- x -} %% c"),
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
                t(r#""x\"y""#, String),
                t("// done", Comment)
            ]
        );
        assert_eq!(
            tokens("Rust", "#[derive(Debug)]"),
            vec![t("#[derive(Debug)]", Meta)]
        );
        assert_eq!(
            tokens("Rust", "let c = 'x'; 0x1F"),
            vec![t("let", Keyword), t("'x'", String), t("0x1F", Number)]
        );
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
}
