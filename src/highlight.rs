//! Built-in syntax highlighting for code files and fenced code blocks.
//!
//! A small table-driven lexer per language (no LSP, no grammars): keywords,
//! types, strings, numbers, comments, function calls, macros/decorators.
//! It works line by line; the only thing carried from one line to the next
//! is `LineState` (inside a block comment, a multi-line string...), so the
//! editor can colour just the visible lines and re-lex from an edit onwards.
//! It never needs a complete program: fragments in notes colour the same.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Lang {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    /// Also JSONC / JSON5 (comments, single quotes, bare keys).
    Json,
    Toml,
    Yaml,
    /// sh, bash, zsh.
    Shell,
    PowerShell,
    C,
    Cpp,
    Go,
    Sql,
    /// Also XML and SVG.
    Html,
    /// Also SCSS / Less.
    Css,
}

impl Lang {
    /// From a fence tag (` ```rust `) or a file extension.
    pub(crate) fn from_tag(tag: &str) -> Option<Self> {
        match tag.trim().to_ascii_lowercase().as_str() {
            "rust" | "rs" => Some(Self::Rust),
            "python" | "py" | "python3" | "pyw" | "pyi" => Some(Self::Python),
            "javascript" | "js" | "jsx" | "mjs" | "cjs" | "node" => Some(Self::JavaScript),
            "typescript" | "ts" | "tsx" | "mts" | "cts" => Some(Self::TypeScript),
            "json" | "jsonc" | "json5" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            "sh" | "bash" | "zsh" | "shell" => Some(Self::Shell),
            "powershell" | "pwsh" | "ps1" | "psm1" | "psd1" => Some(Self::PowerShell),
            "c" => Some(Self::C),
            // `.h` is shared by C and C++; C++ colours C headers fine.
            "cpp" | "c++" | "cc" | "cxx" | "h" | "hpp" | "hh" | "hxx" | "h++" => Some(Self::Cpp),
            "go" | "golang" => Some(Self::Go),
            "sql" | "mysql" | "postgres" | "postgresql" | "psql" | "sqlite" | "plsql" | "tsql" => {
                Some(Self::Sql)
            }
            "html" | "htm" | "xhtml" | "xml" | "svg" => Some(Self::Html),
            "css" | "scss" | "less" => Some(Self::Css),
            _ => None,
        }
    }

    pub(crate) fn from_path(path: &std::path::Path) -> Option<Self> {
        path.extension()
            .and_then(|extension| extension.to_str())
            .and_then(Self::from_tag)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    Keyword,
    Type,
    Function,
    String,
    Number,
    /// true/false/None/null, ALL_CAPS constants, self.
    Constant,
    Comment,
    /// Rust macros and attributes, Python/TS decorators.
    Macro,
}

impl Token {
    /// Slate's palette (the same blues, greens and ambers as the rest of the UI).
    pub(crate) fn color(self) -> eframe::egui::Color32 {
        use eframe::egui::Color32;
        match self {
            Self::Keyword => Color32::from_rgb(180, 142, 173),
            Self::Type => Color32::from_rgb(143, 188, 187),
            Self::Function => Color32::from_rgb(136, 192, 208),
            Self::String => Color32::from_rgb(163, 190, 140),
            Self::Number | Self::Constant => Color32::from_rgb(208, 135, 112),
            Self::Comment => Color32::from_rgb(110, 122, 146),
            Self::Macro => Color32::from_rgb(235, 203, 139),
        }
    }
}

/// What an unfinished construct at the end of a line carries into the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LineState {
    #[default]
    Normal,
    /// Rust nests block comments.
    BlockComment { depth: u8 },
    /// A `"…"` string still open (Rust strings may span lines).
    String,
    /// Rust `r#"…"#` with this many `#`.
    RawString { hashes: u8 },
    /// Python `'''…'''` / `"""…"""`, TOML multi-line strings.
    TripleString { quote: u8 },
    /// JS/TS `` `…` ``.
    Template,
    /// Go `` `…` `` (no escapes).
    RawBacktick,
    /// Shell / PowerShell `"…"` or `'…'` still open.
    ShellString { quote: u8 },
    /// PowerShell `@"…"@` / `@'…'@`, closed by a line starting `"@`.
    HereString { quote: u8 },
    /// PowerShell `<# … #>`.
    PsBlockComment,
    /// HTML `<!-- … -->`.
    HtmlComment,
    /// Inside an HTML tag, among its attributes.
    HtmlTag,
    /// Inside a quoted HTML attribute value.
    HtmlAttr { quote: u8 },
    /// YAML `|` / `>` block scalar: lines indented deeper than `indent` are text.
    YamlBlock { indent: u8 },
}

const RUST_KEYWORDS: &[&str] = &[
    "as",
    "async",
    "await",
    "break",
    "const",
    "continue",
    "crate",
    "dyn",
    "else",
    "enum",
    "extern",
    "fn",
    "for",
    "if",
    "impl",
    "in",
    "let",
    "loop",
    "match",
    "mod",
    "move",
    "mut",
    "pub",
    "ref",
    "return",
    "static",
    "struct",
    "super",
    "trait",
    "type",
    "unsafe",
    "use",
    "where",
    "while",
    "yield",
    "macro_rules",
    "union",
];
const RUST_CONSTANTS: &[&str] = &["true", "false", "self", "Self", "None", "Some", "Ok", "Err"];
const RUST_TYPES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32",
    "f64", "bool", "char", "str",
];

const PYTHON_KEYWORDS: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif",
    "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda",
    "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield", "match",
    "case",
];
const PYTHON_CONSTANTS: &[&str] = &["True", "False", "None", "self", "cls"];
const PYTHON_TYPES: &[&str] = &[
    "int", "float", "str", "bool", "bytes", "list", "dict", "set", "tuple", "object", "type",
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
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "as",
];
const TS_KEYWORDS: &[&str] = &[
    "abstract",
    "declare",
    "enum",
    "implements",
    "interface",
    "keyof",
    "namespace",
    "private",
    "protected",
    "public",
    "readonly",
    "type",
    "satisfies",
    "is",
    "infer",
    "override",
];
const JS_CONSTANTS: &[&str] = &[
    "true",
    "false",
    "null",
    "undefined",
    "this",
    "NaN",
    "Infinity",
];
const TS_TYPES: &[&str] = &[
    "string", "number", "boolean", "any", "unknown", "never", "object", "bigint", "symbol",
];

const C_KEYWORDS: &[&str] = &[
    "auto",
    "break",
    "case",
    "const",
    "continue",
    "default",
    "do",
    "else",
    "enum",
    "extern",
    "for",
    "goto",
    "if",
    "inline",
    "register",
    "restrict",
    "return",
    "sizeof",
    "static",
    "struct",
    "switch",
    "typedef",
    "union",
    "volatile",
    "while",
    "_Alignas",
    "_Alignof",
    "_Atomic",
    "_Generic",
    "_Noreturn",
    "_Static_assert",
    "_Thread_local",
];
const CPP_KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "catch",
    "class",
    "concept",
    "consteval",
    "constexpr",
    "constinit",
    "const_cast",
    "co_await",
    "co_return",
    "co_yield",
    "decltype",
    "delete",
    "dynamic_cast",
    "explicit",
    "export",
    "final",
    "friend",
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
    "requires",
    "static_assert",
    "static_cast",
    "template",
    "throw",
    "try",
    "typeid",
    "typename",
    "using",
    "virtual",
];
const C_CONSTANTS: &[&str] = &["true", "false", "NULL"];
const CPP_CONSTANTS: &[&str] = &["nullptr", "this"];
const C_TYPES: &[&str] = &[
    "void",
    "char",
    "short",
    "int",
    "long",
    "float",
    "double",
    "signed",
    "unsigned",
    "bool",
    "_Bool",
    "size_t",
    "ssize_t",
    "ptrdiff_t",
    "intptr_t",
    "uintptr_t",
    "int8_t",
    "int16_t",
    "int32_t",
    "int64_t",
    "uint8_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
    "wchar_t",
    "FILE",
];
const CPP_TYPES: &[&str] = &[
    "std",
    "string",
    "wstring",
    "string_view",
    "vector",
    "map",
    "unordered_map",
    "set",
    "unordered_set",
    "list",
    "deque",
    "array",
    "pair",
    "tuple",
    "optional",
    "variant",
    "unique_ptr",
    "shared_ptr",
    "weak_ptr",
    "function",
    "char8_t",
    "char16_t",
    "char32_t",
    "nullptr_t",
];

const GO_KEYWORDS: &[&str] = &[
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
];
const GO_CONSTANTS: &[&str] = &["true", "false", "nil", "iota"];
const GO_TYPES: &[&str] = &[
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
    "any",
    "comparable",
];

/// Matched case-insensitively.
const SQL_KEYWORDS: &[&str] = &[
    "select",
    "from",
    "where",
    "and",
    "or",
    "not",
    "insert",
    "into",
    "values",
    "update",
    "set",
    "delete",
    "create",
    "alter",
    "drop",
    "table",
    "index",
    "view",
    "trigger",
    "procedure",
    "function",
    "database",
    "schema",
    "if",
    "exists",
    "join",
    "inner",
    "left",
    "right",
    "full",
    "outer",
    "cross",
    "natural",
    "on",
    "using",
    "as",
    "distinct",
    "all",
    "union",
    "intersect",
    "except",
    "order",
    "by",
    "group",
    "having",
    "limit",
    "offset",
    "fetch",
    "asc",
    "desc",
    "case",
    "when",
    "then",
    "else",
    "end",
    "is",
    "in",
    "between",
    "like",
    "ilike",
    "primary",
    "key",
    "foreign",
    "references",
    "unique",
    "check",
    "default",
    "constraint",
    "add",
    "column",
    "begin",
    "commit",
    "rollback",
    "transaction",
    "with",
    "recursive",
    "returning",
    "grant",
    "revoke",
    "cascade",
    "replace",
    "temporary",
    "declare",
    "returns",
    "return",
    "over",
    "partition",
    "explain",
    "top",
];
const SQL_CONSTANTS: &[&str] = &["true", "false", "null"];
const SQL_TYPES: &[&str] = &[
    "int",
    "integer",
    "bigint",
    "smallint",
    "tinyint",
    "serial",
    "bigserial",
    "varchar",
    "char",
    "character",
    "text",
    "boolean",
    "bool",
    "date",
    "time",
    "timestamp",
    "timestamptz",
    "interval",
    "float",
    "real",
    "double",
    "numeric",
    "decimal",
    "blob",
    "bytea",
    "json",
    "jsonb",
    "uuid",
];

const SHELL_KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "case", "esac", "for", "select", "while", "until", "do",
    "done", "in", "function", "time", "return", "break", "continue", "local", "export", "readonly",
    "declare", "typeset", "unset", "shift", "exit", "source", "alias", "eval", "exec", "trap",
];
const SHELL_CONSTANTS: &[&str] = &["true", "false"];
const SHELL_BUILTINS: &[&str] = &[
    "echo", "printf", "read", "cd", "pwd", "pushd", "popd", "test", "set", "shopt", "getopts",
    "wait", "kill", "let", "command", "type", "hash", "ulimit", "umask",
];

/// Matched case-insensitively.
const PS_KEYWORDS: &[&str] = &[
    "begin",
    "break",
    "catch",
    "class",
    "continue",
    "data",
    "do",
    "dynamicparam",
    "else",
    "elseif",
    "end",
    "enum",
    "exit",
    "filter",
    "finally",
    "for",
    "foreach",
    "function",
    "hidden",
    "if",
    "in",
    "param",
    "process",
    "return",
    "static",
    "switch",
    "throw",
    "trap",
    "try",
    "until",
    "using",
    "while",
    "workflow",
];
/// `-eq`, `-like`… (case-insensitive, without the dash).
const PS_OPERATORS: &[&str] = &[
    "eq",
    "ne",
    "gt",
    "ge",
    "lt",
    "le",
    "like",
    "notlike",
    "match",
    "notmatch",
    "contains",
    "notcontains",
    "in",
    "notin",
    "replace",
    "split",
    "join",
    "and",
    "or",
    "not",
    "xor",
    "band",
    "bor",
    "bxor",
    "bnot",
    "shl",
    "shr",
    "is",
    "isnot",
    "as",
    "f",
    "ceq",
    "cne",
    "ieq",
    "ine",
    "clike",
    "ilike",
    "cmatch",
    "imatch",
    "creplace",
    "ireplace",
];

const YAML_CONSTANTS: &[&str] = &["true", "false", "null", "yes", "no", "on", "off", "~"];

/// Colours one line. `state` is what the previous line left open; returns the
/// coloured byte ranges (uncoloured text is plain) and the state for the next
/// line.
pub(crate) fn highlight_line(
    lang: Lang,
    state: LineState,
    line: &str,
) -> (Vec<(Range<usize>, Token)>, LineState) {
    let mut lexer = Lexer {
        lang,
        bytes: line.as_bytes(),
        line,
        at: 0,
        tokens: Vec::new(),
    };
    let state = lexer.run(state);
    (lexer.tokens, state)
}

struct Lexer<'a> {
    lang: Lang,
    bytes: &'a [u8],
    line: &'a str,
    at: usize,
    tokens: Vec<(Range<usize>, Token)>,
}

impl Lexer<'_> {
    fn push(&mut self, range: Range<usize>, token: Token) {
        if !range.is_empty() {
            self.tokens.push((range, token));
        }
    }

    fn peek(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.at + offset).copied()
    }

    fn starts_with(&self, text: &str) -> bool {
        self.bytes[self.at..].starts_with(text.as_bytes())
    }

    fn prev_byte(&self, at: usize) -> Option<u8> {
        at.checked_sub(1).map(|before| self.bytes[before])
    }

    fn prev_non_space(&self, at: usize) -> Option<u8> {
        self.bytes[..at]
            .iter()
            .rev()
            .copied()
            .find(|c| *c != b' ' && *c != b'\t')
    }

    /// Nothing but indentation before the cursor.
    fn only_space_before(&self) -> bool {
        self.bytes[..self.at]
            .iter()
            .all(|c| matches!(c, b' ' | b'\t'))
    }

    /// Advances while `keep` holds. `keep` must accept all bytes >= 0x80 or
    /// none of them, so the cursor stays on a character boundary.
    fn skip_while(&mut self, keep: impl Fn(u8) -> bool) {
        while self.peek(0).is_some_and(&keep) {
            self.at += 1;
        }
    }

    /// Skips one whole character (it may be multi-byte).
    fn skip_char(&mut self) {
        self.at += self.line[self.at..]
            .chars()
            .next()
            .map_or(1, char::len_utf8);
    }

    /// The rest of the line is one token.
    fn rest_is(&mut self, token: Token) {
        self.push(self.at..self.bytes.len(), token);
        self.at = self.bytes.len();
    }

    fn run(&mut self, mut state: LineState) -> LineState {
        loop {
            state = match state {
                LineState::Normal => match self.normal() {
                    Some(next) => next,
                    None => return LineState::Normal,
                },
                LineState::BlockComment { depth } => self.block_comment(depth),
                LineState::String => {
                    let start = self.at;
                    self.rust_string_body(start)
                }
                LineState::RawString { hashes } => self.raw_string(hashes),
                LineState::TripleString { quote } => self.triple_string(quote),
                LineState::Template => self.template(),
                LineState::RawBacktick => {
                    self.delimited(self.at, "`", Token::String, LineState::RawBacktick)
                }
                LineState::ShellString { quote } => {
                    let start = self.at;
                    self.shell_string(start, quote, true)
                }
                LineState::HereString { quote } => self.here_string(quote),
                LineState::PsBlockComment => {
                    self.delimited(self.at, "#>", Token::Comment, LineState::PsBlockComment)
                }
                LineState::HtmlComment => {
                    self.delimited(self.at, "-->", Token::Comment, LineState::HtmlComment)
                }
                LineState::HtmlTag => self.html_tag(),
                LineState::HtmlAttr { quote } => {
                    let start = self.at;
                    let closed = self.skip_quoted(quote, None);
                    self.push(start..self.at, Token::String);
                    if closed {
                        LineState::HtmlTag
                    } else {
                        LineState::HtmlAttr { quote }
                    }
                }
                LineState::YamlBlock { indent } => self.yaml_block(indent),
            };
            if self.at >= self.bytes.len() {
                return state;
            }
        }
    }

    /// Lexes until the line ends (`None`) or a multi-line construct opens.
    fn normal(&mut self) -> Option<LineState> {
        match self.lang {
            Lang::Json => return self.json(),
            Lang::Toml => return self.toml(),
            Lang::Yaml => return self.yaml(),
            Lang::Shell | Lang::PowerShell => return self.shell(),
            Lang::Html => return self.html(),
            Lang::Css => return self.css(),
            _ => {}
        }
        while self.at < self.bytes.len() {
            let c = self.bytes[self.at];
            let start = self.at;
            match (self.lang, c) {
                (Lang::Python, b'#') => {
                    self.push(start..self.bytes.len(), Token::Comment);
                    self.at = self.bytes.len();
                }
                (Lang::Sql, b'-') if self.peek(1) == Some(b'-') => self.rest_is(Token::Comment),
                (_, b'/')
                    if !matches!(self.lang, Lang::Python | Lang::Sql)
                        && self.peek(1) == Some(b'/') =>
                {
                    self.push(start..self.bytes.len(), Token::Comment);
                    self.at = self.bytes.len();
                }
                (_, b'/') if self.lang != Lang::Python && self.peek(1) == Some(b'*') => {
                    return Some(LineState::BlockComment { depth: 0 });
                }
                (Lang::Rust, b'#') if matches!(self.peek(1), Some(b'[') | Some(b'!')) => {
                    // Attribute: up to the closing bracket on this line.
                    let end = self.line[start..]
                        .find(']')
                        .map_or(self.bytes.len(), |offset| start + offset + 1);
                    self.push(start..end, Token::Macro);
                    self.at = end;
                }
                (Lang::C | Lang::Cpp, b'#') if self.only_space_before() => self.preprocessor(),
                (Lang::Python | Lang::TypeScript | Lang::JavaScript, b'@')
                    if self.peek(1).is_some_and(is_ident_start) =>
                {
                    self.at += 1;
                    self.skip_ident_and_dots();
                    self.push(start..self.at, Token::Macro);
                }
                (Lang::Sql, b'@') if self.peek(1).is_some_and(is_ident_start) => {
                    self.at += 1;
                    self.skip_while(is_ident_continue);
                    self.push(start..self.at, Token::Macro);
                }
                (Lang::Rust, b'r')
                    if matches!(self.peek(1), Some(b'"') | Some(b'#'))
                        && self.raw_string_opening().is_some() =>
                {
                    let (hashes, opening) = self.raw_string_opening().unwrap();
                    self.push(start..start + opening, Token::String);
                    self.at += opening;
                    return Some(LineState::RawString { hashes });
                }
                (Lang::Rust, b'b') if self.peek(1) == Some(b'"') => {
                    self.push(start..start + 1, Token::String);
                    self.at += 1;
                }
                (Lang::Rust, b'"') => {
                    self.at += 1;
                    let next = self.rust_string_body(start);
                    if next != LineState::Normal {
                        return Some(next);
                    }
                }
                (Lang::Rust, b'\'') => self.rust_quote(),
                (Lang::Python, b'"' | b'\'') => {
                    if let Some(next) = self.python_string(c) {
                        return Some(next);
                    }
                }
                (
                    Lang::JavaScript | Lang::TypeScript | Lang::C | Lang::Cpp | Lang::Go,
                    b'"' | b'\'',
                ) => {
                    self.at += 1;
                    self.single_line_string(start, c);
                }
                (Lang::JavaScript | Lang::TypeScript, b'`') => {
                    self.push(start..start + 1, Token::String);
                    self.at += 1;
                    return Some(LineState::Template);
                }
                (Lang::Go, b'`') => {
                    self.at += 1;
                    let next = self.delimited(start, "`", Token::String, LineState::RawBacktick);
                    if next != LineState::Normal {
                        return Some(next);
                    }
                }
                (Lang::Sql, b'\'') => {
                    // `''` inside closes and reopens: two adjacent string tokens.
                    self.at += 1;
                    self.skip_quoted(b'\'', None);
                    self.push(start..self.at, Token::String);
                }
                (Lang::Sql, b'"' | b'`') => {
                    // A quoted identifier: plain, but no keywords inside.
                    self.at += 1;
                    self.skip_quoted(c, None);
                }
                (_, b'0'..=b'9') => self.number(),
                _ if is_ident_start(c) => self.identifier(),
                _ => self.skip_char(),
            }
        }
        None
    }

    /// A number from the cursor (which may sit on a sign or a leading dot):
    /// digits, suffixes (`10u8`, `1.5f`, `10px`), hex, exponents.
    fn number(&mut self) {
        let start = self.at;
        self.at += 1;
        let hex = self.bytes[start] == b'0' && matches!(self.peek(0), Some(b'x' | b'X'));
        while let Some(c) = self.peek(0) {
            let next_is_digit = self.peek(1).is_some_and(|c| c.is_ascii_digit());
            let keep = c.is_ascii_alphanumeric()
                || c == b'_'
                // `1..2` is a range, not a float.
                || (c == b'.' && self.peek(1) != Some(b'.'))
                || (matches!(c, b'+' | b'-')
                    && !hex
                    && matches!(self.bytes[self.at - 1], b'e' | b'E')
                    && next_is_digit)
                // TOML dates and times: 1979-05-27T07:32:00-08:00
                || (self.lang == Lang::Toml && matches!(c, b'-' | b':' | b'+') && next_is_digit)
                || (self.lang == Lang::Css && c == b'%');
            if !keep {
                break;
            }
            self.at += 1;
        }
        self.push(start..self.at, Token::Number);
    }

    fn identifier(&mut self) {
        let start = self.at;
        while self.peek(0).is_some_and(is_ident_continue) {
            self.at += 1;
        }
        // Python string prefixes: f"…", rb'…', u"…"
        if self.lang == Lang::Python
            && matches!(self.peek(0), Some(b'"') | Some(b'\''))
            && self.at - start <= 2
            && self.line[start..self.at]
                .chars()
                .all(|c| matches!(c.to_ascii_lowercase(), 'r' | 'b' | 'f' | 'u'))
        {
            self.push(start..self.at, Token::String);
            return;
        }
        let word = &self.line[start..self.at];
        // C/C++ string prefixes: L"…", u8"…", R"(…)"
        if matches!(self.lang, Lang::C | Lang::Cpp)
            && matches!(self.peek(0), Some(b'"') | Some(b'\''))
            && matches!(
                word,
                "L" | "u" | "U" | "u8" | "R" | "LR" | "uR" | "UR" | "u8R"
            )
        {
            self.push(start..self.at, Token::String);
            return;
        }
        type Tables = &'static [&'static [&'static str]];
        let (keywords, constants, types): (Tables, Tables, Tables) = match self.lang {
            Lang::Rust => (&[RUST_KEYWORDS], &[RUST_CONSTANTS], &[RUST_TYPES]),
            Lang::Python => (&[PYTHON_KEYWORDS], &[PYTHON_CONSTANTS], &[PYTHON_TYPES]),
            Lang::JavaScript => (&[JS_KEYWORDS], &[JS_CONSTANTS], &[]),
            Lang::TypeScript => (&[JS_KEYWORDS, TS_KEYWORDS], &[JS_CONSTANTS], &[TS_TYPES]),
            Lang::C => (&[C_KEYWORDS], &[C_CONSTANTS], &[C_TYPES]),
            Lang::Cpp => (
                &[C_KEYWORDS, CPP_KEYWORDS],
                &[C_CONSTANTS, CPP_CONSTANTS],
                &[C_TYPES, CPP_TYPES],
            ),
            Lang::Go => (&[GO_KEYWORDS], &[GO_CONSTANTS], &[GO_TYPES]),
            Lang::Sql => (&[SQL_KEYWORDS], &[SQL_CONSTANTS], &[SQL_TYPES]),
            _ => (&[], &[], &[]),
        };
        let ignore_case = self.lang == Lang::Sql;
        let found = |tables: Tables| {
            tables.iter().any(|table| {
                if ignore_case {
                    table.iter().any(|known| known.eq_ignore_ascii_case(word))
                } else {
                    table.contains(&word)
                }
            })
        };
        let is_call = self.next_non_space() == Some(b'(');
        let token = if found(keywords) {
            Some(Token::Keyword)
        } else if found(constants) {
            Some(Token::Constant)
        } else if found(types) {
            Some(Token::Type)
        } else if self.lang == Lang::Rust && self.peek(0) == Some(b'!') {
            self.at += 1;
            Some(Token::Macro)
        } else if self.lang == Lang::Sql {
            // `count(*)`, but not the table in `CREATE TABLE t (id …)`.
            (self.peek(0) == Some(b'(')).then_some(Token::Function)
        } else if self.lang == Lang::Go && is_call {
            // `fmt.Println(`: exported names are capitalised, calls win.
            Some(Token::Function)
        } else if word.len() > 1
            && word
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
            && word.bytes().any(|c| c.is_ascii_uppercase())
        {
            Some(Token::Constant)
        } else if word.as_bytes()[0].is_ascii_uppercase() {
            Some(Token::Type)
        } else if is_call || (self.lang == Lang::Rust && self.starts_with("::<")) {
            Some(Token::Function)
        } else {
            None
        };
        if let Some(token) = token {
            self.push(start..self.at, token);
        }
    }

    fn skip_ident_and_dots(&mut self) {
        while self
            .peek(0)
            .is_some_and(|c| is_ident_continue(c) || c == b'.')
        {
            self.at += 1;
        }
    }

    fn next_non_space(&self) -> Option<u8> {
        self.bytes[self.at..]
            .iter()
            .copied()
            .find(|c| *c != b' ' && *c != b'\t')
    }

    fn block_comment(&mut self, mut depth: u8) -> LineState {
        let start = self.at;
        while self.at < self.bytes.len() {
            if self.starts_with("/*") {
                // Only Rust nests; JS/TS treat an inner `/*` as text.
                if self.lang == Lang::Rust || depth == 0 {
                    depth = depth.saturating_add(1);
                }
                self.at += 2;
            } else if self.starts_with("*/") {
                self.at += 2;
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    self.push(start..self.at, Token::Comment);
                    return LineState::Normal;
                }
            } else {
                self.at += 1;
            }
        }
        self.push(start..self.at, Token::Comment);
        LineState::BlockComment { depth }
    }

    /// From `self.at` up to and including `close` (or the end of the line,
    /// leaving `open` for the next one). The token starts at `start`.
    fn delimited(&mut self, start: usize, close: &str, token: Token, open: LineState) -> LineState {
        match self.line[self.at..].find(close) {
            Some(offset) => {
                self.at += offset + close.len();
                self.push(start..self.at, token);
                LineState::Normal
            }
            None => {
                self.at = self.bytes.len();
                self.push(start..self.at, token);
                open
            }
        }
    }

    /// From just after an opening `quote`, moves past the closing one (or to
    /// the end of the line). True if it closed.
    fn skip_quoted(&mut self, quote: u8, escape: Option<u8>) -> bool {
        while self.at < self.bytes.len() {
            let c = self.bytes[self.at];
            if Some(c) == escape {
                self.at += 2;
            } else if c == quote {
                self.at += 1;
                return true;
            } else {
                self.at += 1;
            }
        }
        self.at = self.bytes.len();
        false
    }

    /// The rest of a Rust `"…"` from `self.at`; it may continue on the next
    /// line. The token starts at `start` (the opening quote, or column 0).
    fn rust_string_body(&mut self, start: usize) -> LineState {
        while self.at < self.bytes.len() {
            match self.bytes[self.at] {
                b'\\' => self.at += 2,
                b'"' => {
                    self.at += 1;
                    self.push(start..self.at, Token::String);
                    return LineState::Normal;
                }
                _ => self.at += 1,
            }
        }
        self.at = self.bytes.len();
        self.push(start..self.at, Token::String);
        LineState::String
    }

    fn single_line_string(&mut self, start: usize, quote: u8) {
        self.skip_quoted(quote, Some(b'\\'));
        self.push(start..self.at, Token::String);
    }

    /// `r"`, `r#"`, `r##"`…: (hashes, length of the opening).
    fn raw_string_opening(&self) -> Option<(u8, usize)> {
        let mut offset = 1;
        let mut hashes = 0u8;
        while self.peek(offset) == Some(b'#') {
            hashes = hashes.saturating_add(1);
            offset += 1;
        }
        (self.peek(offset) == Some(b'"')).then_some((hashes, offset + 1))
    }

    fn raw_string(&mut self, hashes: u8) -> LineState {
        let start = self.at;
        let closing = format!("\"{}", "#".repeat(usize::from(hashes)));
        match self.line[self.at..].find(&closing) {
            Some(offset) => {
                self.at += offset + closing.len();
                self.push(start..self.at, Token::String);
                LineState::Normal
            }
            None => {
                self.at = self.bytes.len();
                self.push(start..self.at, Token::String);
                LineState::RawString { hashes }
            }
        }
    }

    /// `'a'` / `'\n'` are characters; `'a` alone is a lifetime.
    fn rust_quote(&mut self) {
        let start = self.at;
        let rest = &self.line[start + 1..];
        let mut chars = rest.char_indices();
        let is_char = match chars.next() {
            Some((_, '\\')) => true,
            Some((_, c)) => chars.next().is_some_and(|(_, next)| next == '\'') || c == '\'',
            None => false,
        };
        if is_char {
            self.at += 1;
            self.single_line_string(start, b'\'');
        } else {
            self.at += 1;
            while self.peek(0).is_some_and(is_ident_continue) {
                self.at += 1;
            }
            self.push(start..self.at, Token::Keyword);
        }
    }

    fn python_string(&mut self, quote: u8) -> Option<LineState> {
        let start = self.at;
        if self.peek(1) == Some(quote) && self.peek(2) == Some(quote) {
            self.at += 3;
            return Some(self.triple_body(start, quote));
        }
        self.at += 1;
        self.single_line_string(start, quote);
        None
    }

    fn triple_string(&mut self, quote: u8) -> LineState {
        let start = self.at;
        self.triple_body(start, quote)
    }

    fn triple_body(&mut self, start: usize, quote: u8) -> LineState {
        let closing = [quote; 3];
        let closing = std::str::from_utf8(&closing).unwrap();
        match self.line[self.at..].find(closing) {
            Some(offset) => {
                self.at += offset + 3;
                self.push(start..self.at, Token::String);
                LineState::Normal
            }
            None => {
                self.at = self.bytes.len();
                self.push(start..self.at, Token::String);
                LineState::TripleString { quote }
            }
        }
    }

    fn template(&mut self) -> LineState {
        let start = self.at;
        while self.at < self.bytes.len() {
            match self.bytes[self.at] {
                b'\\' => self.at += 2,
                b'`' => {
                    self.at += 1;
                    self.push(start..self.at, Token::String);
                    return LineState::Normal;
                }
                _ => self.at += 1,
            }
        }
        self.at = self.bytes.len();
        self.push(start..self.at, Token::String);
        LineState::Template
    }

    /// C/C++ `#include <x>` / `#define X`: the directive is a macro.
    fn preprocessor(&mut self) {
        let start = self.at;
        self.at += 1;
        self.skip_while(|c| c == b' ' || c == b'\t');
        let word = self.at;
        self.skip_while(|c| c.is_ascii_alphanumeric() || c == b'_');
        let directive = &self.line[word..self.at];
        self.push(start..self.at, Token::Macro);
        if matches!(directive, "include" | "include_next" | "import") {
            self.skip_while(|c| c == b' ' || c == b'\t');
            if self.peek(0) == Some(b'<') {
                let path = self.at;
                self.at += 1;
                self.skip_quoted(b'>', None);
                self.push(path..self.at, Token::String);
            }
        }
    }

    /// JSON, JSONC, JSON5: keys (strings or bare words before `:`) are types.
    fn json(&mut self) -> Option<LineState> {
        while self.at < self.bytes.len() {
            let c = self.bytes[self.at];
            let start = self.at;
            match c {
                b'/' if self.peek(1) == Some(b'/') => self.rest_is(Token::Comment),
                b'/' if self.peek(1) == Some(b'*') => {
                    return Some(LineState::BlockComment { depth: 0 });
                }
                b'"' | b'\'' => {
                    self.at += 1;
                    self.skip_quoted(c, Some(b'\\'));
                    let token = if self.next_non_space() == Some(b':') {
                        Token::Type
                    } else {
                        Token::String
                    };
                    self.push(start..self.at, token);
                }
                b'0'..=b'9' => self.number(),
                b'-' | b'+' | b'.' if self.peek(1).is_some_and(|c| c.is_ascii_digit()) => {
                    self.number()
                }
                _ if is_ident_start(c) => {
                    self.skip_while(is_ident_continue);
                    let word = &self.line[start..self.at];
                    if matches!(word, "true" | "false" | "null" | "NaN" | "Infinity") {
                        self.push(start..self.at, Token::Constant);
                    } else if self.next_non_space() == Some(b':') {
                        self.push(start..self.at, Token::Type);
                    }
                }
                _ => self.skip_char(),
            }
        }
        None
    }

    fn toml(&mut self) -> Option<LineState> {
        let mut expect_key = self.only_space_before();
        let mut braces = 0u32;
        while self.at < self.bytes.len() {
            let c = self.bytes[self.at];
            let start = self.at;
            match c {
                b' ' | b'\t' => self.at += 1,
                b'#' => self.rest_is(Token::Comment),
                b'[' if expect_key && self.toml_header_end().is_some() => {
                    let end = self.toml_header_end().unwrap();
                    self.push(start..end, Token::Keyword);
                    self.at = end;
                    expect_key = false;
                }
                _ if expect_key && (is_bare_key(c) || c == b'"' || c == b'\'') => {
                    expect_key = false;
                    // Not a key after all (an array item...): lex it as a value.
                    if let Some(end) = self.toml_key_end() {
                        self.push(start..end, Token::Type);
                        self.at = end;
                    }
                }
                b'"' | b'\'' if self.peek(1) == Some(c) && self.peek(2) == Some(c) => {
                    self.at += 3;
                    let next = self.triple_body(start, c);
                    if next != LineState::Normal {
                        return Some(next);
                    }
                }
                b'"' | b'\'' => {
                    self.at += 1;
                    self.skip_quoted(c, (c == b'"').then_some(b'\\'));
                    self.push(start..self.at, Token::String);
                }
                b'{' => {
                    braces += 1;
                    expect_key = true;
                    self.at += 1;
                }
                b'}' => {
                    braces = braces.saturating_sub(1);
                    self.at += 1;
                }
                b',' => {
                    expect_key = braces > 0;
                    self.at += 1;
                }
                b'0'..=b'9' => self.number(),
                b'+' | b'-' if self.peek(1).is_some_and(|c| c.is_ascii_digit()) => self.number(),
                _ if c.is_ascii_alphabetic() => {
                    self.skip_while(is_bare_key);
                    if matches!(&self.line[start..self.at], "true" | "false" | "inf" | "nan") {
                        self.push(start..self.at, Token::Constant);
                    }
                }
                _ => self.skip_char(),
            }
        }
        None
    }

    /// `[table]` / `[[array]]` with nothing but a comment after it.
    fn toml_header_end(&self) -> Option<usize> {
        let close = self.at + self.line[self.at..].find(']')? + 1;
        let close = if self.bytes.get(close) == Some(&b']') {
            close + 1
        } else {
            close
        };
        let rest = self.line[close..].trim_start();
        (!self.line[self.at..close].contains(',') && (rest.is_empty() || rest.starts_with('#')))
            .then_some(close)
    }

    /// `a.b."c" =`: the end of the dotted key if an `=` follows it.
    fn toml_key_end(&self) -> Option<usize> {
        let mut at = self.at;
        loop {
            match self.bytes.get(at) {
                Some(&quote @ (b'"' | b'\'')) => {
                    at += 1 + self.line[at + 1..].find(char::from(quote))? + 1;
                }
                Some(&c) if is_bare_key(c) => {
                    while self.bytes.get(at).is_some_and(|c| is_bare_key(*c)) {
                        at += 1;
                    }
                }
                _ => return None,
            }
            let end = at;
            while matches!(self.bytes.get(at), Some(b' ' | b'\t')) {
                at += 1;
            }
            match self.bytes.get(at) {
                Some(b'.') => {
                    at += 1;
                    while matches!(self.bytes.get(at), Some(b' ' | b'\t')) {
                        at += 1;
                    }
                }
                Some(b'=') => return Some(end),
                _ => return None,
            }
        }
    }

    fn yaml(&mut self) -> Option<LineState> {
        let indent = self.bytes.iter().take_while(|c| **c == b' ').count();
        let mut key_position = self.only_space_before();
        if self.at == 0
            && (self.starts_with("---") || self.starts_with("..."))
            && matches!(self.peek(3), None | Some(b' ' | b'\t'))
        {
            self.push(0..3, Token::Keyword);
            self.at = 3;
            key_position = false;
        }
        let mut flow = 0u32;
        let mut block = false;
        while self.at < self.bytes.len() {
            let c = self.bytes[self.at];
            let start = self.at;
            let next_is_blank = matches!(self.peek(1), None | Some(b' ' | b'\t'));
            match c {
                b' ' | b'\t' => self.at += 1,
                b'#' if matches!(self.prev_byte(start), None | Some(b' ' | b'\t')) => {
                    self.rest_is(Token::Comment)
                }
                // `- item`, `? complex key`
                b'-' | b'?' if key_position && next_is_blank => self.at += 1,
                // Anchors, aliases, tags.
                b'&' | b'*' | b'!' if !next_is_blank => {
                    self.at += 1;
                    self.skip_while(|c| {
                        !matches!(c, b' ' | b'\t' | b',' | b'[' | b']' | b'{' | b'}')
                    });
                    self.push(start..self.at, Token::Macro);
                }
                b'{' | b'[' => {
                    flow += 1;
                    key_position = c == b'{';
                    self.at += 1;
                }
                b'}' | b']' => {
                    flow = flow.saturating_sub(1);
                    key_position = false;
                    self.at += 1;
                }
                b',' if flow > 0 => {
                    key_position = true;
                    self.at += 1;
                }
                b'"' | b'\'' => {
                    self.at += 1;
                    self.skip_quoted(c, (c == b'"').then_some(b'\\'));
                    let token = if key_position && self.next_non_space() == Some(b':') {
                        Token::Type
                    } else {
                        Token::String
                    };
                    self.push(start..self.at, token);
                    key_position = false;
                }
                b'|' | b'>' if self.yaml_block_indicator_end().is_some() => {
                    let end = self.yaml_block_indicator_end().unwrap();
                    self.push(start..end, Token::Keyword);
                    self.at = end;
                    block = true;
                    key_position = false;
                }
                _ => {
                    let end = self.yaml_plain_end(flow > 0);
                    if end == start {
                        self.skip_char();
                        continue;
                    }
                    let text_end = start + self.line[start..end].trim_end().len();
                    if key_position && self.bytes.get(end) == Some(&b':') {
                        self.push(start..text_end, Token::Type);
                        self.at = end + 1;
                    } else {
                        if let Some(token) = yaml_scalar(&self.line[start..text_end]) {
                            self.push(start..text_end, token);
                        }
                        self.at = end;
                    }
                    key_position = false;
                }
            }
        }
        block.then_some(LineState::YamlBlock {
            indent: u8::try_from(indent).unwrap_or(u8::MAX),
        })
    }

    /// `|`, `>-`, `|2+`… ending the line (a comment may follow).
    fn yaml_block_indicator_end(&self) -> Option<usize> {
        let mut end = self.at + 1;
        while self
            .bytes
            .get(end)
            .is_some_and(|c| matches!(c, b'-' | b'+' | b'0'..=b'9'))
        {
            end += 1;
        }
        let rest = &self.line[end..];
        let trimmed = rest.trim_start();
        (trimmed.is_empty() || (trimmed.starts_with('#') && trimmed.len() < rest.len()))
            .then_some(end)
    }

    /// End of a plain (unquoted) scalar: before `: `, ` #`, or a flow bracket.
    fn yaml_plain_end(&self, flow: bool) -> usize {
        let mut at = self.at;
        while at < self.bytes.len() {
            let c = self.bytes[at];
            let after = self.bytes.get(at + 1).copied();
            let ends = match c {
                b':' => {
                    after.is_none_or(|c| matches!(c, b' ' | b'\t'))
                        || (flow && matches!(after, Some(b',' | b']' | b'}')))
                }
                b'#' => at > self.at && matches!(self.bytes[at - 1], b' ' | b'\t'),
                b',' | b'[' | b']' | b'{' | b'}' => flow,
                _ => false,
            };
            if ends {
                break;
            }
            at += 1;
        }
        at
    }

    /// A line under `key: |`: text while it is indented deeper than the key.
    fn yaml_block(&mut self, indent: u8) -> LineState {
        let lead = self
            .bytes
            .iter()
            .take_while(|c| matches!(c, b' ' | b'\t'))
            .count();
        if lead == self.bytes.len() {
            self.at = self.bytes.len();
            LineState::YamlBlock { indent }
        } else if lead > usize::from(indent) {
            self.push(lead..self.bytes.len(), Token::String);
            self.at = self.bytes.len();
            LineState::YamlBlock { indent }
        } else {
            LineState::Normal
        }
    }

    /// sh/bash/zsh and PowerShell.
    fn shell(&mut self) -> Option<LineState> {
        let ps = self.lang == Lang::PowerShell;
        while self.at < self.bytes.len() {
            let c = self.bytes[self.at];
            let start = self.at;
            let after_word = self
                .prev_byte(start)
                .is_some_and(|p| p.is_ascii_alphanumeric() || matches!(p, b'_' | b']' | b')'));
            match c {
                b'$' if self.variable().is_some() => {
                    let (end, token) = self.variable().unwrap();
                    self.push(start..end, token);
                    self.at = end;
                }
                b'<' if ps && self.peek(1) == Some(b'#') => {
                    self.at += 2;
                    let next =
                        self.delimited(start, "#>", Token::Comment, LineState::PsBlockComment);
                    if next != LineState::Normal {
                        return Some(next);
                    }
                }
                b'#' if matches!(
                    self.prev_byte(start),
                    None | Some(b' ' | b'\t' | b';' | b'(' | b'{' | b'|' | b'&')
                ) =>
                {
                    self.rest_is(Token::Comment)
                }
                b'@' if ps
                    && matches!(self.peek(1), Some(b'"' | b'\''))
                    && self.line[start + 2..].trim().is_empty() =>
                {
                    let quote = self.bytes[start + 1];
                    self.rest_is(Token::String);
                    return Some(LineState::HereString { quote });
                }
                b'"' | b'\'' => {
                    self.at += 1;
                    let next = self.shell_string(start, c, true);
                    if next != LineState::Normal {
                        return Some(next);
                    }
                }
                b'`' if !ps => {
                    self.at += 1;
                    self.single_line_string(start, b'`');
                }
                // `[string]`, `[System.IO.Path]`
                b'[' if ps
                    && !after_word
                    && self.peek(1).is_some_and(|c| c.is_ascii_alphabetic()) =>
                {
                    self.at += 1;
                    let name = self.at;
                    self.skip_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.'));
                    self.push(name..self.at, Token::Type);
                }
                // `-eq`, `-like` (other `-Name`s are parameters, plain).
                b'-' if ps
                    && !after_word
                    && self.peek(1).is_some_and(|c| c.is_ascii_alphabetic()) =>
                {
                    self.at += 1;
                    self.skip_while(|c| c.is_ascii_alphanumeric());
                    let name = &self.line[start + 1..self.at];
                    if PS_OPERATORS.iter().any(|op| op.eq_ignore_ascii_case(name)) {
                        self.push(start..self.at, Token::Keyword);
                    }
                }
                b'0'..=b'9' if ps => self.number(),
                b'0'..=b'9' => {
                    self.skip_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.'));
                    if self.line[start..self.at]
                        .bytes()
                        .all(|c| c.is_ascii_digit() || c == b'.')
                    {
                        self.push(start..self.at, Token::Number);
                    }
                }
                _ if c.is_ascii_alphabetic() || c == b'_' => self.shell_word(),
                // `--if`, `-f`: options, plain (never keywords).
                b'-' if !ps && !after_word => {
                    self.skip_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
                }
                _ => self.skip_char(),
            }
        }
        None
    }

    fn shell_word(&mut self) {
        let start = self.at;
        let token = if self.lang == Lang::PowerShell {
            self.skip_while(|c| c.is_ascii_alphanumeric() || c == b'_');
            let mut cmdlet = false;
            // `Get-ChildItem`
            while self.peek(0) == Some(b'-')
                && self.peek(1).is_some_and(|c| c.is_ascii_alphabetic())
            {
                self.at += 1;
                self.skip_while(|c| c.is_ascii_alphanumeric() || c == b'_');
                cmdlet = true;
            }
            let word = &self.line[start..self.at];
            if cmdlet {
                Some(Token::Function)
            } else if PS_KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(word)) {
                Some(Token::Keyword)
            } else if self.peek(0) == Some(b'(') {
                Some(Token::Function)
            } else {
                None
            }
        } else {
            self.skip_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'));
            let word = &self.line[start..self.at];
            if SHELL_KEYWORDS.contains(&word) {
                Some(Token::Keyword)
            } else if SHELL_CONSTANTS.contains(&word) {
                Some(Token::Constant)
            } else if SHELL_BUILTINS.contains(&word) || self.next_non_space() == Some(b'(') {
                Some(Token::Function)
            } else {
                None
            }
        };
        if let Some(token) = token {
            self.push(start..self.at, token);
        }
    }

    /// `$name`, `${…}`, `$1`, `$?`, `$(`, PowerShell `$env:Path`: (end, token).
    fn variable(&self) -> Option<(usize, Token)> {
        let start = self.at;
        let ps = self.lang == Lang::PowerShell;
        let name_end = |mut at: usize| {
            while self
                .bytes
                .get(at)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                at += 1;
            }
            at
        };
        let end = match self.peek(1)? {
            b'{' => self.line[start + 2..]
                .find('}')
                .map_or(self.bytes.len(), |offset| start + 2 + offset + 1),
            b'(' => start + 2,
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let mut end = name_end(start + 2);
                if ps
                    && self.bytes.get(end) == Some(&b':')
                    && self
                        .bytes
                        .get(end + 1)
                        .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
                {
                    end = name_end(end + 1);
                }
                let name = &self.line[start + 1..end];
                if ps
                    && ["true", "false", "null"]
                        .iter()
                        .any(|k| k.eq_ignore_ascii_case(name))
                {
                    return Some((end, Token::Constant));
                }
                end
            }
            b'0'..=b'9' | b'@' | b'*' | b'#' | b'?' | b'$' | b'!' | b'-' | b'^' => start + 2,
            _ => return None,
        };
        Some((end, Token::Macro))
    }

    /// A shell string from `self.at` (just past the opening quote, or the
    /// start of a continuation line). `"…"` colours `$variables` inside.
    /// Without `close` (PowerShell here-strings) it runs to the line's end.
    fn shell_string(&mut self, start: usize, quote: u8, close: bool) -> LineState {
        let escape = if self.lang == Lang::PowerShell {
            b'`'
        } else {
            b'\\'
        };
        let interpolate = quote == b'"';
        let mut piece = start;
        while self.at < self.bytes.len() {
            let c = self.bytes[self.at];
            if interpolate && c == escape {
                self.at += 2;
                continue;
            }
            if close && c == quote {
                self.at += 1;
                self.push(piece..self.at, Token::String);
                return LineState::Normal;
            }
            if interpolate && c == b'$' {
                if let Some((end, token)) = self.variable() {
                    self.push(piece..self.at, Token::String);
                    self.push(self.at..end, token);
                    self.at = end;
                    piece = end;
                    continue;
                }
            }
            self.at += 1;
        }
        self.at = self.bytes.len();
        self.push(piece..self.at, Token::String);
        if close {
            LineState::ShellString { quote }
        } else {
            LineState::HereString { quote }
        }
    }

    /// A line inside a PowerShell here-string; `"@` at its start closes it.
    fn here_string(&mut self, quote: u8) -> LineState {
        let start = self.at;
        if self.peek(0) == Some(quote) && self.peek(1) == Some(b'@') {
            self.at += 2;
            self.push(start..self.at, Token::String);
            return LineState::Normal;
        }
        self.shell_string(start, quote, false)
    }

    /// HTML/XML text: tags, entities, comments.
    fn html(&mut self) -> Option<LineState> {
        while self.at < self.bytes.len() {
            let c = self.bytes[self.at];
            let start = self.at;
            if self.starts_with("<!--") {
                self.at += 4;
                return Some(self.delimited(start, "-->", Token::Comment, LineState::HtmlComment));
            }
            match c {
                b'<' if self.peek(1).is_some_and(|c| {
                    c.is_ascii_alphabetic() || matches!(c, b'/' | b'!' | b'?')
                }) =>
                {
                    self.at += 1;
                    if self.peek(0) == Some(b'/') {
                        self.at += 1;
                    }
                    let name = self.at;
                    if matches!(self.peek(0), Some(b'!' | b'?')) {
                        self.at += 1;
                    }
                    self.skip_while(|c| {
                        c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b':' | b'.')
                    });
                    self.push(name..self.at, Token::Keyword);
                    return Some(self.html_tag());
                }
                b'&' => match self.entity_end() {
                    Some(end) => {
                        self.push(start..end, Token::Constant);
                        self.at = end;
                    }
                    None => self.at += 1,
                },
                _ => self.skip_char(),
            }
        }
        None
    }

    /// `&amp;`, `&#160;`, `&#x1F600;`
    fn entity_end(&self) -> Option<usize> {
        let mut at = self.at + 1;
        if self.bytes.get(at) == Some(&b'#') {
            at += 1;
        }
        let name = at;
        while at - name < 32 && self.bytes.get(at).is_some_and(u8::is_ascii_alphanumeric) {
            at += 1;
        }
        (at > name && self.bytes.get(at) == Some(&b';')).then_some(at + 1)
    }

    /// Attributes up to the tag's `>`.
    fn html_tag(&mut self) -> LineState {
        while self.at < self.bytes.len() {
            let c = self.bytes[self.at];
            let start = self.at;
            match c {
                b'>' => {
                    self.at += 1;
                    return LineState::Normal;
                }
                b'/' | b'?' if self.peek(1) == Some(b'>') => {
                    self.at += 2;
                    return LineState::Normal;
                }
                b'"' | b'\'' => {
                    self.at += 1;
                    let closed = self.skip_quoted(c, None);
                    self.push(start..self.at, Token::String);
                    if !closed {
                        return LineState::HtmlAttr { quote: c };
                    }
                }
                b' ' | b'\t' | b'=' => self.at += 1,
                _ => {
                    let value = self.prev_non_space(start) == Some(b'=');
                    while let Some(c) = self.peek(0) {
                        if matches!(c, b' ' | b'\t' | b'=' | b'>' | b'"' | b'\'')
                            || (c == b'/' && self.peek(1) == Some(b'>'))
                        {
                            break;
                        }
                        self.at += 1;
                    }
                    if self.at == start {
                        self.skip_char();
                        continue;
                    }
                    let token = if value { Token::String } else { Token::Type };
                    self.push(start..self.at, token);
                }
            }
        }
        LineState::HtmlTag
    }

    /// CSS/SCSS: selectors, properties, values. Whether a word is a selector
    /// or a property is guessed from the line (no brace depth is carried).
    fn css(&mut self) -> Option<LineState> {
        let selector_line = self.bytes.contains(&b'{') || self.line.trim_end().ends_with(',');
        let mut value = false;
        let mut parens = 0u32;
        while self.at < self.bytes.len() {
            let c = self.bytes[self.at];
            let start = self.at;
            match c {
                b'/' if self.peek(1) == Some(b'*') => {
                    return Some(LineState::BlockComment { depth: 0 });
                }
                // SCSS `//`, but not `url(//cdn…)` or `http://`.
                b'/' if self.peek(1) == Some(b'/')
                    && matches!(
                        self.prev_byte(start),
                        None | Some(b' ' | b'\t' | b';' | b'{' | b'}')
                    ) =>
                {
                    self.rest_is(Token::Comment)
                }
                b'"' | b'\'' => {
                    self.at += 1;
                    self.single_line_string(start, c);
                }
                // `@media`, SCSS `$variable`
                b'@' | b'$' if self.peek(1).is_some_and(is_css_ident_start) => {
                    self.at += 1;
                    self.skip_while(is_css_ident);
                    self.push(start..self.at, Token::Macro);
                }
                b'#' if self.peek(1).is_some_and(is_css_ident) => {
                    self.at += 1;
                    self.skip_while(is_css_ident);
                    let token = if value || !selector_line {
                        Token::Constant
                    } else {
                        Token::Function
                    };
                    self.push(start..self.at, token);
                }
                b'.' if self.peek(1).is_some_and(|c| c.is_ascii_digit()) => self.number(),
                // `.class`, `:hover`, `::before`
                b'.' | b':'
                    if !value
                        && selector_line
                        && (self.peek(1).is_some_and(is_css_ident_start)
                            || (c == b':' && self.peek(1) == Some(b':'))) =>
                {
                    self.at += 1;
                    if self.peek(0) == Some(b':') {
                        self.at += 1;
                    }
                    self.skip_while(is_css_ident);
                    self.push(start..self.at, Token::Function);
                }
                b';' | b'{' | b'}' => {
                    value = false;
                    self.at += 1;
                }
                b'(' => {
                    parens += 1;
                    self.at += 1;
                }
                b')' => {
                    parens = parens.saturating_sub(1);
                    self.at += 1;
                }
                b'!' if self.peek(1).is_some_and(|c| c.is_ascii_alphabetic()) => {
                    self.at += 1;
                    self.skip_while(|c| c.is_ascii_alphabetic());
                    self.push(start..self.at, Token::Keyword);
                }
                b'0'..=b'9' => self.number(),
                b'-' | b'+'
                    if value
                        && (self.peek(1).is_some_and(|c| c.is_ascii_digit())
                            || (self.peek(1) == Some(b'.')
                                && self.peek(2).is_some_and(|c| c.is_ascii_digit()))) =>
                {
                    self.number()
                }
                _ if is_css_ident_start(c) => {
                    self.skip_while(is_css_ident);
                    let word = &self.line[start..self.at];
                    if !value && self.css_property_follows(parens) {
                        self.push(start..self.at, Token::Type);
                        value = true;
                        // Past the colon, so it is not read as `:pseudo`.
                        self.skip_while(|c| c == b' ' || c == b'\t');
                        self.at += 1;
                    } else if self.peek(0) == Some(b'(') {
                        self.push(start..self.at, Token::Function);
                    } else if !value
                        && selector_line
                        && word.bytes().any(|c| c.is_ascii_alphabetic())
                    {
                        self.push(start..self.at, Token::Keyword);
                    }
                }
                _ => self.skip_char(),
            }
        }
        None
    }

    /// After a word: `: value` up to `;`/`}` is a declaration; `:hover {` is a
    /// selector.
    fn css_property_follows(&self, parens: u32) -> bool {
        let rest = &self.bytes[self.at..];
        let Some(colon) = rest.iter().position(|c| *c != b' ' && *c != b'\t') else {
            return false;
        };
        if rest[colon] != b':' || rest.get(colon + 1) == Some(&b':') {
            return false;
        }
        if parens > 0 {
            return true;
        }
        for c in &rest[colon + 1..] {
            match c {
                b'{' => return false,
                b';' | b'}' => return true,
                _ => {}
            }
        }
        !self.line.trim_end().ends_with(',')
    }
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c >= 0x80
}

fn is_ident_continue(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80
}

fn is_bare_key(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

fn is_css_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'-' || c >= 0x80
}

fn is_css_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c >= 0x80
}

/// A whole plain YAML value: `true`/`no`/`~` are constants, numbers and
/// dates are numbers, anything else is plain text.
fn yaml_scalar(text: &str) -> Option<Token> {
    if YAML_CONSTANTS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(text))
    {
        return Some(Token::Constant);
    }
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    if unsigned.eq_ignore_ascii_case(".inf") || unsigned.eq_ignore_ascii_case(".nan") {
        return Some(Token::Number);
    }
    let bytes = unsigned.as_bytes();
    let starts_numeric = bytes.first().is_some_and(u8::is_ascii_digit)
        || (bytes.first() == Some(&b'.') && bytes.get(1).is_some_and(u8::is_ascii_digit));
    if !starts_numeric {
        return None;
    }
    let numeric = if let Some(hex) = unsigned.strip_prefix("0x") {
        !hex.is_empty() && hex.bytes().all(|c| c.is_ascii_hexdigit())
    } else {
        bytes.iter().all(|c| {
            c.is_ascii_digit()
                || matches!(
                    c,
                    b'.' | b'_' | b'+' | b'-' | b':' | b'e' | b'E' | b'T' | b'Z' | b' '
                )
        })
    };
    numeric.then_some(Token::Number)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (text, token) pairs of one line.
    fn tokens(lang: Lang, state: LineState, line: &str) -> (Vec<(&str, Token)>, LineState) {
        let (tokens, next) = highlight_line(lang, state, line);
        (
            tokens
                .into_iter()
                .map(|(range, token)| (&line[range], token))
                .collect(),
            next,
        )
    }

    fn line(lang: Lang, text: &str) -> Vec<(&str, Token)> {
        let (tokens, next) = tokens(lang, LineState::Normal, text);
        assert_eq!(next, LineState::Normal, "{text}");
        tokens
    }

    #[test]
    fn rust_keywords_types_functions_macros_and_literals() {
        use Token::*;
        assert_eq!(
            line(
                Lang::Rust,
                r#"pub fn main() -> Result<u8, Error> { println!("hi {}", 0x1F); }"#
            ),
            vec![
                ("pub", Keyword),
                ("fn", Keyword),
                ("main", Function),
                ("Result", Type),
                ("u8", Type),
                ("Error", Type),
                ("println!", Macro),
                ("\"hi {}\"", String),
                ("0x1F", Number),
            ]
        );
        assert_eq!(
            line(Lang::Rust, "#[derive(Debug)] let x: &'a str = 'c'; // done"),
            vec![
                ("#[derive(Debug)]", Macro),
                ("let", Keyword),
                ("'a", Keyword),
                ("str", Type),
                ("'c'", String),
                ("// done", Comment),
            ]
        );
        assert_eq!(
            line(Lang::Rust, "for i in 0..10 { MAX_LEN; self.go::<T>() }"),
            vec![
                ("for", Keyword),
                ("in", Keyword),
                ("0", Number),
                ("10", Number),
                ("MAX_LEN", Constant),
                ("self", Constant),
                ("go", Function),
                ("T", Type),
            ]
        );
    }

    #[test]
    fn rust_multi_line_comments_and_strings_carry_over() {
        let (first, state) = tokens(
            Lang::Rust,
            LineState::Normal,
            "let a = 1; /* start /* nested */",
        );
        assert_eq!(
            first.last(),
            Some(&("/* start /* nested */", Token::Comment))
        );
        assert_eq!(state, LineState::BlockComment { depth: 1 });
        let (second, state) = tokens(Lang::Rust, state, "still */ let b");
        assert_eq!(
            second,
            vec![("still */", Token::Comment), ("let", Token::Keyword)]
        );
        assert_eq!(state, LineState::Normal);

        let (_, state) = tokens(Lang::Rust, LineState::Normal, "let s = r#\"raw \"quoted\"");
        assert_eq!(state, LineState::RawString { hashes: 1 });
        let (rest, state) = tokens(Lang::Rust, state, "end\"# ; fn");
        assert_eq!(
            rest,
            vec![("end\"#", Token::String), ("fn", Token::Keyword)]
        );
        assert_eq!(state, LineState::Normal);

        let (_, state) = tokens(Lang::Rust, LineState::Normal, "let s = \"two");
        assert_eq!(state, LineState::String);
        let (rest, state) = tokens(Lang::Rust, state, "lines\\\" still\" + x");
        assert_eq!(rest, vec![("lines\\\" still\"", Token::String)]);
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn python_strings_comments_and_decorators() {
        use Token::*;
        assert_eq!(
            line(Lang::Python, "@app.route('/x')"),
            vec![("@app.route", Macro), ("'/x'", String)]
        );
        assert_eq!(
            line(Lang::Python, "def go(self, n: int = 3) -> None:  # why"),
            vec![
                ("def", Keyword),
                ("go", Function),
                ("self", Constant),
                ("int", Type),
                ("3", Number),
                ("None", Constant),
                ("# why", Comment),
            ]
        );
        assert_eq!(
            line(Lang::Python, "x = f\"{a}\" + rb'\\x00'"),
            vec![
                ("f", String),
                ("\"{a}\"", String),
                ("rb", String),
                ("'\\x00'", String)
            ]
        );
        let (_, state) = tokens(Lang::Python, LineState::Normal, "doc = \"\"\"Starts here");
        assert_eq!(state, LineState::TripleString { quote: b'"' });
        let (rest, state) = tokens(Lang::Python, state, "and ends\"\"\" if True");
        assert_eq!(
            rest,
            vec![
                ("and ends\"\"\"", String),
                ("if", Keyword),
                ("True", Constant)
            ]
        );
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn javascript_and_typescript() {
        use Token::*;
        assert_eq!(
            line(
                Lang::JavaScript,
                "const el = document.querySelector('#id'); // x"
            ),
            vec![
                ("const", Keyword),
                ("querySelector", Function),
                ("'#id'", String),
                ("// x", Comment),
            ]
        );
        assert_eq!(
            line(
                Lang::TypeScript,
                "export interface User { name: string; age?: number }"
            ),
            vec![
                ("export", Keyword),
                ("interface", Keyword),
                ("User", Type),
                ("string", Type),
                ("number", Type),
            ]
        );
        assert_eq!(
            line(Lang::JavaScript, "interface"),
            vec![],
            "TypeScript-only words are plain in JavaScript"
        );
        let (_, state) = tokens(Lang::TypeScript, LineState::Normal, "const s = `multi ${x}");
        assert_eq!(state, LineState::Template);
        let (rest, state) = tokens(Lang::TypeScript, state, "line` + null");
        assert_eq!(rest, vec![("line`", String), ("null", Constant)]);
        assert_eq!(state, LineState::Normal);
        let (_, state) = tokens(Lang::JavaScript, LineState::Normal, "/* a /* b");
        let (rest, state) = tokens(Lang::JavaScript, state, "c */ d */");
        assert_eq!(
            rest,
            vec![("c */", Comment)],
            "JS block comments do not nest"
        );
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn json_keys_values_and_comments() {
        use Token::*;
        assert_eq!(
            line(
                Lang::Json,
                r#"{"name": "slate", "size": -1.5e3, "ok": true, "none": null},"#
            ),
            vec![
                ("\"name\"", Type),
                ("\"slate\"", String),
                ("\"size\"", Type),
                ("-1.5e3", Number),
                ("\"ok\"", Type),
                ("true", Constant),
                ("\"none\"", Type),
                ("null", Constant),
            ]
        );
        assert_eq!(
            line(Lang::Json, "  bare: 'x', // jsonc"),
            vec![("bare", Type), ("'x'", String), ("// jsonc", Comment)]
        );
        let (_, state) = tokens(Lang::Json, LineState::Normal, "[1, /* two");
        assert_eq!(state, LineState::BlockComment { depth: 1 });
        let (rest, state) = tokens(Lang::Json, state, "lines */ 2]");
        assert_eq!(rest, vec![("lines */", Comment), ("2", Number)]);
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn toml_tables_keys_strings_and_dates() {
        use Token::*;
        assert_eq!(
            line(Lang::Toml, "[package] # the crate"),
            vec![("[package]", Keyword), ("# the crate", Comment)]
        );
        assert_eq!(line(Lang::Toml, "[[bin]]"), vec![("[[bin]]", Keyword)]);
        assert_eq!(
            line(Lang::Toml, r#"name = "slate"  # quoted"#),
            vec![("name", Type), ("\"slate\"", String), ("# quoted", Comment)]
        );
        assert_eq!(
            line(
                Lang::Toml,
                r#"site."google.com" = { strip = true, level = 3 }"#
            ),
            vec![
                ("site.\"google.com\"", Type),
                ("strip", Type),
                ("true", Constant),
                ("level", Type),
                ("3", Number),
            ]
        );
        assert_eq!(
            line(Lang::Toml, "when = 1979-05-27T07:32:00-08:00"),
            vec![("when", Type), ("1979-05-27T07:32:00-08:00", Number)]
        );
        assert_eq!(
            line(Lang::Toml, "path = 'C:\\x\\'"),
            vec![("path", Type), ("'C:\\x\\'", String)],
            "literal strings have no escapes"
        );
        assert_eq!(
            line(Lang::Toml, "  [1, 2],"),
            vec![("1", Number), ("2", Number)],
            "an array line is not a table header"
        );
        let (first, state) = tokens(Lang::Toml, LineState::Normal, "text = '''");
        assert_eq!(first, vec![("text", Type), ("'''", String)]);
        assert_eq!(state, LineState::TripleString { quote: b'\'' });
        let (rest, state) = tokens(Lang::Toml, state, "key = 1 '''  # done");
        assert_eq!(rest, vec![("key = 1 '''", String), ("# done", Comment)]);
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn yaml_keys_scalars_anchors_and_block_scalars() {
        use Token::*;
        assert_eq!(
            line(Lang::Yaml, "--- # doc"),
            vec![("---", Keyword), ("# doc", Comment)]
        );
        assert_eq!(
            line(Lang::Yaml, "  - name: Slate editor  # why"),
            vec![("name", Type), ("# why", Comment)]
        );
        assert_eq!(
            line(Lang::Yaml, "port: 8080"),
            vec![("port", Type), ("8080", Number)]
        );
        assert_eq!(
            line(Lang::Yaml, "enabled: yes"),
            vec![("enabled", Type), ("yes", Constant)]
        );
        assert_eq!(line(Lang::Yaml, "url: http://x.y/z:1"), vec![("url", Type)]);
        assert_eq!(
            line(Lang::Yaml, r#""quoted key": 'it''s'"#),
            vec![("\"quoted key\"", Type), ("'it'", String), ("'s'", String)]
        );
        assert_eq!(
            line(Lang::Yaml, "base: &base !!map {a: 1, b: [x, null]}"),
            vec![
                ("base", Type),
                ("&base", Macro),
                ("!!map", Macro),
                ("a", Type),
                ("1", Number),
                ("b", Type),
                ("null", Constant),
            ]
        );
        assert_eq!(
            line(Lang::Yaml, "<<: *base"),
            vec![("<<", Type), ("*base", Macro)]
        );
        let (first, state) = tokens(Lang::Yaml, LineState::Normal, "  script: |-");
        assert_eq!(first, vec![("script", Type), ("|-", Keyword)]);
        assert_eq!(state, LineState::YamlBlock { indent: 2 });
        let (body, state) = tokens(Lang::Yaml, state, "    echo: not a key # nor a comment");
        assert_eq!(body, vec![("echo: not a key # nor a comment", String)]);
        let (blank, state) = tokens(Lang::Yaml, state, "");
        assert_eq!(blank, vec![]);
        assert_eq!(state, LineState::YamlBlock { indent: 2 });
        let (after, state) = tokens(Lang::Yaml, state, "  next: 1");
        assert_eq!(after, vec![("next", Type), ("1", Number)]);
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn shell_words_variables_and_strings() {
        use Token::*;
        assert_eq!(
            line(
                Lang::Shell,
                r#"if [ -f "$HOME/.rc" ]; then echo "hi ${USER}!" $1 2>&1; fi # done"#
            ),
            vec![
                ("if", Keyword),
                ("\"", String),
                ("$HOME", Macro),
                ("/.rc\"", String),
                ("then", Keyword),
                ("echo", Function),
                ("\"hi ", String),
                ("${USER}", Macro),
                ("!\"", String),
                ("$1", Macro),
                ("2", Number),
                ("1", Number),
                ("fi", Keyword),
                ("# done", Comment),
            ]
        );
        assert_eq!(
            line(
                Lang::Shell,
                "apt-get install --if x#y 'lit $x' && build() { true; }"
            ),
            vec![
                ("'lit $x'", String),
                ("build", Function),
                ("true", Constant),
            ]
        );
        let (_, state) = tokens(Lang::Shell, LineState::Normal, "msg=\"two");
        assert_eq!(state, LineState::ShellString { quote: b'"' });
        let (rest, state) = tokens(Lang::Shell, state, "lines $x\" done");
        assert_eq!(
            rest,
            vec![
                ("lines ", String),
                ("$x", Macro),
                ("\"", String),
                ("done", Keyword)
            ]
        );
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn powershell_cmdlets_variables_and_comments() {
        use Token::*;
        assert_eq!(
            line(
                Lang::PowerShell,
                r#"foreach ($f in Get-ChildItem -Path $env:TEMP) { if ($f.Length -gt 10kb) { Write-Host "big: $($f.Name)" } } # x"#
            ),
            vec![
                ("foreach", Keyword),
                ("$f", Macro),
                ("in", Keyword),
                ("Get-ChildItem", Function),
                ("$env:TEMP", Macro),
                ("if", Keyword),
                ("$f", Macro),
                ("-gt", Keyword),
                ("10kb", Number),
                ("Write-Host", Function),
                ("\"big: ", String),
                ("$(", Macro),
                ("$f", Macro),
                (".Name)\"", String),
                ("# x", Comment),
            ]
        );
        assert_eq!(
            line(Lang::PowerShell, "[string]$s = $null; [Math]::Round(2.5)"),
            vec![
                ("string", Type),
                ("$s", Macro),
                ("$null", Constant),
                ("Math", Type),
                ("Round", Function),
                ("2.5", Number),
            ]
        );
        let (first, state) = tokens(Lang::PowerShell, LineState::Normal, "$a = 1 <# block");
        assert_eq!(
            first,
            vec![("$a", Macro), ("1", Number), ("<# block", Comment)]
        );
        assert_eq!(state, LineState::PsBlockComment);
        let (rest, state) = tokens(Lang::PowerShell, state, "comment #> $b");
        assert_eq!(rest, vec![("comment #>", Comment), ("$b", Macro)]);
        assert_eq!(state, LineState::Normal);
        let (first, state) = tokens(Lang::PowerShell, LineState::Normal, "$t = @\"");
        assert_eq!(first, vec![("$t", Macro), ("@\"", String)]);
        assert_eq!(state, LineState::HereString { quote: b'"' });
        let (body, state) = tokens(Lang::PowerShell, state, "Hi $name \"quoted\"");
        assert_eq!(
            body,
            vec![("Hi ", String), ("$name", Macro), (" \"quoted\"", String)]
        );
        assert_eq!(state, LineState::HereString { quote: b'"' });
        let (end, state) = tokens(Lang::PowerShell, state, "\"@ | Out-File x");
        assert_eq!(end, vec![("\"@", String), ("Out-File", Function)]);
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn c_and_cpp() {
        use Token::*;
        assert_eq!(
            line(Lang::C, "#include <stdio.h> // io"),
            vec![
                ("#include", Macro),
                ("<stdio.h>", String),
                ("// io", Comment)
            ]
        );
        assert_eq!(
            line(Lang::C, "#define MAX_LEN 10UL"),
            vec![("#define", Macro), ("MAX_LEN", Constant), ("10UL", Number)]
        );
        assert_eq!(
            line(
                Lang::C,
                r#"static size_t count(const char *s) { return s ? strlen(s) : '\0' + 1.5f; }"#
            ),
            vec![
                ("static", Keyword),
                ("size_t", Type),
                ("count", Function),
                ("const", Keyword),
                ("char", Type),
                ("return", Keyword),
                ("strlen", Function),
                ("'\\0'", String),
                ("1.5f", Number),
            ]
        );
        assert_eq!(
            line(
                Lang::Cpp,
                r#"auto v = std::make_unique<Widget>(L"x", nullptr);"#
            ),
            vec![
                ("auto", Keyword),
                ("std", Type),
                ("Widget", Type),
                ("L", String),
                ("\"x\"", String),
                ("nullptr", Constant),
            ]
        );
        assert_eq!(
            line(Lang::C, "class"),
            vec![],
            "C++-only words are plain in C"
        );
        let (_, state) = tokens(Lang::Cpp, LineState::Normal, "int x; /* a");
        assert_eq!(state, LineState::BlockComment { depth: 1 });
    }

    #[test]
    fn go_raw_strings_and_calls() {
        use Token::*;
        assert_eq!(
            line(
                Lang::Go,
                "func main() { fmt.Println(\"hi\", 'x', nil); var n int64 = 0x1F }"
            ),
            vec![
                ("func", Keyword),
                ("main", Function),
                ("Println", Function),
                ("\"hi\"", String),
                ("'x'", String),
                ("nil", Constant),
                ("var", Keyword),
                ("int64", Type),
                ("0x1F", Number),
            ]
        );
        let (first, state) = tokens(Lang::Go, LineState::Normal, "q := `select \\");
        assert_eq!(first, vec![("`select \\", String)]);
        assert_eq!(state, LineState::RawBacktick);
        let (rest, state) = tokens(Lang::Go, state, "from t` // go");
        assert_eq!(rest, vec![("from t`", String), ("// go", Comment)]);
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn sql_case_insensitive_keywords() {
        use Token::*;
        assert_eq!(
            line(
                Lang::Sql,
                "SELECT count(*), \"Order\" FROM users u WHERE u.name = 'it''s' AND id > 10 -- x"
            ),
            vec![
                ("SELECT", Keyword),
                ("count", Function),
                ("FROM", Keyword),
                ("WHERE", Keyword),
                ("'it'", String),
                ("'s'", String),
                ("AND", Keyword),
                ("10", Number),
                ("-- x", Comment),
            ]
        );
        assert_eq!(
            line(
                Lang::Sql,
                "create table t (id integer primary key, ok boolean default NULL);"
            ),
            vec![
                ("create", Keyword),
                ("table", Keyword),
                ("integer", Type),
                ("primary", Keyword),
                ("key", Keyword),
                ("boolean", Type),
                ("default", Keyword),
                ("NULL", Constant),
            ]
        );
        let (_, state) = tokens(Lang::Sql, LineState::Normal, "/* multi");
        let (rest, state) = tokens(Lang::Sql, state, "line */ select");
        assert_eq!(rest, vec![("line */", Comment), ("select", Keyword)]);
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn html_tags_attributes_entities_and_comments() {
        use Token::*;
        assert_eq!(
            line(
                Lang::Html,
                r#"<a href="/x" class='y' hidden data-n=3>Tom &amp; Jerry</a><br/>"#
            ),
            vec![
                ("a", Keyword),
                ("href", Type),
                ("\"/x\"", String),
                ("class", Type),
                ("'y'", String),
                ("hidden", Type),
                ("data-n", Type),
                ("3", String),
                ("&amp;", Constant),
                ("a", Keyword),
                ("br", Keyword),
            ]
        );
        assert_eq!(
            line(Lang::Html, "<!DOCTYPE html> a < b && c"),
            vec![("!DOCTYPE", Keyword), ("html", Type)]
        );
        let (first, state) = tokens(Lang::Html, LineState::Normal, "<p>x <!-- note");
        assert_eq!(first, vec![("p", Keyword), ("<!-- note", Comment)]);
        assert_eq!(state, LineState::HtmlComment);
        let (rest, state) = tokens(Lang::Html, state, "<b> --> &#x1F600;");
        assert_eq!(rest, vec![("<b> -->", Comment), ("&#x1F600;", Constant)]);
        assert_eq!(state, LineState::Normal);
        let (first, state) = tokens(Lang::Html, LineState::Normal, "<svg width=\"10\"");
        assert_eq!(
            first,
            vec![("svg", Keyword), ("width", Type), ("\"10\"", String)]
        );
        assert_eq!(state, LineState::HtmlTag);
        let (second, state) = tokens(Lang::Html, state, "  d=\"M 0 0");
        assert_eq!(second, vec![("d", Type), ("\"M 0 0", String)]);
        assert_eq!(state, LineState::HtmlAttr { quote: b'"' });
        let (third, state) = tokens(Lang::Html, state, "L 1 1\" fill=red>text");
        assert_eq!(
            third,
            vec![("L 1 1\"", String), ("fill", Type), ("red", String)]
        );
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn css_selectors_properties_and_values() {
        use Token::*;
        assert_eq!(
            line(Lang::Css, "a.btn:hover, #main > li::before {"),
            vec![
                ("a", Keyword),
                (".btn", Function),
                (":hover", Function),
                ("#main", Function),
                ("li", Keyword),
                ("::before", Function),
            ]
        );
        assert_eq!(
            line(
                Lang::Css,
                "  margin: -0.5em 10px; color: #fff !important; width: calc(100% - 2rem);"
            ),
            vec![
                ("margin", Type),
                ("-0.5em", Number),
                ("10px", Number),
                ("color", Type),
                ("#fff", Constant),
                ("!important", Keyword),
                ("width", Type),
                ("calc", Function),
                ("100%", Number),
                ("2rem", Number),
            ]
        );
        assert_eq!(
            line(Lang::Css, "@media (max-width: 600px) { .x{color:red} }"),
            vec![
                ("@media", Macro),
                ("max-width", Type),
                ("600px", Number),
                (".x", Function),
                ("color", Type),
            ]
        );
        assert_eq!(
            line(Lang::Css, "$gap: 4px; // scss"),
            vec![("$gap", Macro), ("4px", Number), ("// scss", Comment)]
        );
        assert_eq!(
            line(Lang::Css, "  background: url(//cdn/x.png);"),
            vec![("background", Type), ("url", Function)]
        );
        let (_, state) = tokens(Lang::Css, LineState::Normal, "/* a");
        let (rest, state) = tokens(Lang::Css, state, "b */ p { }");
        assert_eq!(rest, vec![("b */", Comment), ("p", Keyword)]);
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn languages_from_fence_tags_and_extensions() {
        assert_eq!(Lang::from_tag("rust"), Some(Lang::Rust));
        assert_eq!(Lang::from_tag("Py"), Some(Lang::Python));
        assert_eq!(Lang::from_tag("tsx"), Some(Lang::TypeScript));
        assert_eq!(Lang::from_tag("mjs"), Some(Lang::JavaScript));
        assert_eq!(Lang::from_tag("jsonc"), Some(Lang::Json));
        assert_eq!(Lang::from_tag("TOML"), Some(Lang::Toml));
        assert_eq!(Lang::from_tag("yml"), Some(Lang::Yaml));
        assert_eq!(Lang::from_tag("zsh"), Some(Lang::Shell));
        assert_eq!(Lang::from_tag("pwsh"), Some(Lang::PowerShell));
        assert_eq!(Lang::from_tag("c"), Some(Lang::C));
        assert_eq!(Lang::from_tag("c++"), Some(Lang::Cpp));
        assert_eq!(Lang::from_tag("go"), Some(Lang::Go));
        assert_eq!(Lang::from_tag("SQL"), Some(Lang::Sql));
        assert_eq!(Lang::from_tag("svg"), Some(Lang::Html));
        assert_eq!(Lang::from_tag("scss"), Some(Lang::Css));
        assert_eq!(Lang::from_tag("brainfuck"), None);
        assert_eq!(Lang::from_tag("md"), None);
        for (path, lang) in [
            ("C:/x/main.rs", Lang::Rust),
            ("Cargo.toml", Lang::Toml),
            ("ci.yaml", Lang::Yaml),
            ("build.sh", Lang::Shell),
            ("setup.ps1", Lang::PowerShell),
            ("x.h", Lang::Cpp),
            ("x.hxx", Lang::Cpp),
            ("main.c", Lang::C),
            ("main.go", Lang::Go),
            ("index.htm", Lang::Html),
            ("site.css", Lang::Css),
            ("package.json", Lang::Json),
        ] {
            assert_eq!(
                Lang::from_path(std::path::Path::new(path)),
                Some(lang),
                "{path}"
            );
        }
    }

    #[test]
    fn unicode_and_unfinished_fragments_never_panic() {
        for lang in [
            Lang::Rust,
            Lang::Python,
            Lang::JavaScript,
            Lang::TypeScript,
            Lang::Json,
            Lang::Toml,
            Lang::Yaml,
            Lang::Shell,
            Lang::PowerShell,
            Lang::C,
            Lang::Cpp,
            Lang::Go,
            Lang::Sql,
            Lang::Html,
            Lang::Css,
        ] {
            for text in [
                "let café = \"ñandú\"; // 雪 😀",
                "\"unterminated \\",
                "'",
                "r#",
                "/*",
                "\"\"\"",
                "'''",
                "`",
                "#[",
                "@",
                "x = 'a",
                "'é",
                "<!--",
                "<",
                "</",
                "<a b=\"é",
                "&#",
                "&é;",
                "<#",
                "#>",
                "@\"",
                "\"@",
                "\"$",
                "\"${é",
                "$",
                "$env:",
                "[",
                "[é]",
                "-",
                "--",
                "key: |",
                "- é: é # é",
                "{a: [b, c",
                "&é *é !é",
                "é = 'é'",
                "a.\"é = 1",
                "#include <é",
                "#",
                "\\",
                "\"\\é\"",
                "'\\é'",
                "1e",
                "1e-",
                ".5",
                "#é {",
                "a:é",
                "!",
                "é",
            ] {
                let mut state = LineState::Normal;
                for _ in 0..2 {
                    let (tokens, next) = highlight_line(lang, state, text);
                    let mut previous_end = 0;
                    for (range, _) in tokens {
                        assert!(range.end <= text.len(), "{lang:?} {text}");
                        assert!(range.start >= previous_end, "{lang:?} {text}: overlap");
                        previous_end = range.end;
                        assert!(
                            text.is_char_boundary(range.start) && text.is_char_boundary(range.end)
                        );
                    }
                    state = next;
                }
            }
        }
    }

    #[test]
    fn every_carried_state_makes_progress() {
        // Each state on each language's tricky lines terminates (no endless
        // re-entry of `run` without consuming input).
        let states = [
            LineState::Normal,
            LineState::BlockComment { depth: 1 },
            LineState::TripleString { quote: b'"' },
            LineState::RawBacktick,
            LineState::ShellString { quote: b'"' },
            LineState::HereString { quote: b'\'' },
            LineState::PsBlockComment,
            LineState::HtmlComment,
            LineState::HtmlTag,
            LineState::HtmlAttr { quote: b'"' },
            LineState::YamlBlock { indent: 0 },
        ];
        for lang in [
            Lang::Json,
            Lang::Toml,
            Lang::Yaml,
            Lang::Shell,
            Lang::PowerShell,
            Lang::Go,
            Lang::Html,
            Lang::Css,
        ] {
            for state in states {
                for text in [
                    "", " ", "x", "\"@", "#>", "-->", "`", "*/", "a\" b>", "  a: |",
                ] {
                    let _ = highlight_line(lang, state, text);
                }
            }
        }
    }
}
