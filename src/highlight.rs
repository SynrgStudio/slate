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
}

impl Lang {
    /// From a fence tag (` ```rust `) or a file extension.
    pub(crate) fn from_tag(tag: &str) -> Option<Self> {
        match tag.trim().to_ascii_lowercase().as_str() {
            "rust" | "rs" => Some(Self::Rust),
            "python" | "py" | "python3" | "pyw" | "pyi" => Some(Self::Python),
            "javascript" | "js" | "jsx" | "mjs" | "cjs" | "node" => Some(Self::JavaScript),
            "typescript" | "ts" | "tsx" | "mts" | "cts" => Some(Self::TypeScript),
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
    /// Python `'''…'''` / `"""…"""`.
    TripleString { quote: u8 },
    /// JS/TS `` `…` ``.
    Template,
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "static", "struct", "super", "trait", "type", "unsafe", "use", "where", "while",
    "yield", "macro_rules", "union",
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
    "async", "await", "break", "case", "catch", "class", "const", "continue", "debugger",
    "default", "delete", "do", "else", "export", "extends", "finally", "for", "from", "function",
    "get", "if", "import", "in", "instanceof", "let", "new", "of", "return", "set", "static",
    "super", "switch", "throw", "try", "typeof", "var", "void", "while", "with", "yield", "as",
];
const TS_KEYWORDS: &[&str] = &[
    "abstract", "declare", "enum", "implements", "interface", "keyof", "namespace", "private",
    "protected", "public", "readonly", "type", "satisfies", "is", "infer", "override",
];
const JS_CONSTANTS: &[&str] = &["true", "false", "null", "undefined", "this", "NaN", "Infinity"];
const TS_TYPES: &[&str] = &[
    "string", "number", "boolean", "any", "unknown", "never", "object", "bigint", "symbol",
];

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
            };
            if self.at >= self.bytes.len() {
                return state;
            }
        }
    }

    /// Lexes until the line ends (`None`) or a multi-line construct opens.
    fn normal(&mut self) -> Option<LineState> {
        while self.at < self.bytes.len() {
            let c = self.bytes[self.at];
            let start = self.at;
            match (self.lang, c) {
                (Lang::Python, b'#') => {
                    self.push(start..self.bytes.len(), Token::Comment);
                    self.at = self.bytes.len();
                }
                (_, b'/') if self.lang != Lang::Python && self.peek(1) == Some(b'/') => {
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
                (Lang::Python | Lang::TypeScript | Lang::JavaScript, b'@')
                    if self.peek(1).is_some_and(is_ident_start) =>
                {
                    self.at += 1;
                    self.skip_ident_and_dots();
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
                (Lang::JavaScript | Lang::TypeScript, b'"' | b'\'') => {
                    self.at += 1;
                    self.single_line_string(start, c);
                }
                (Lang::JavaScript | Lang::TypeScript, b'`') => {
                    self.push(start..start + 1, Token::String);
                    self.at += 1;
                    return Some(LineState::Template);
                }
                (_, b'0'..=b'9') => {
                    self.at += 1;
                    while self
                        .peek(0)
                        .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.')
                    {
                        // `1..2` is a range, not a float.
                        if self.peek(0) == Some(b'.') && self.peek(1) == Some(b'.') {
                            break;
                        }
                        self.at += 1;
                    }
                    self.push(start..self.at, Token::Number);
                }
                _ if is_ident_start(c) => self.identifier(),
                _ => {
                    // Skip one whole character (it may be multi-byte).
                    self.at += self.line[self.at..].chars().next().map_or(1, char::len_utf8);
                }
            }
        }
        None
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
        let (keywords, constants, types): (&[&str], &[&str], &[&str]) = match self.lang {
            Lang::Rust => (RUST_KEYWORDS, RUST_CONSTANTS, RUST_TYPES),
            Lang::Python => (PYTHON_KEYWORDS, PYTHON_CONSTANTS, PYTHON_TYPES),
            Lang::JavaScript => (JS_KEYWORDS, JS_CONSTANTS, &[]),
            Lang::TypeScript => (JS_KEYWORDS, JS_CONSTANTS, TS_TYPES),
        };
        let token = if keywords.contains(&word)
            || (self.lang == Lang::TypeScript && TS_KEYWORDS.contains(&word))
        {
            Some(Token::Keyword)
        } else if constants.contains(&word) {
            Some(Token::Constant)
        } else if types.contains(&word) {
            Some(Token::Type)
        } else if self.lang == Lang::Rust && self.peek(0) == Some(b'!') {
            self.at += 1;
            Some(Token::Macro)
        } else if word.len() > 1
            && word.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
            && word.bytes().any(|c| c.is_ascii_uppercase())
        {
            Some(Token::Constant)
        } else if word.as_bytes()[0].is_ascii_uppercase() {
            Some(Token::Type)
        } else if self.next_non_space() == Some(b'(')
            || (self.lang == Lang::Rust && self.starts_with("::<"))
        {
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
        while self.at < self.bytes.len() {
            match self.bytes[self.at] {
                b'\\' => self.at += 2,
                c if c == quote => {
                    self.at += 1;
                    break;
                }
                _ => self.at += 1,
            }
        }
        self.at = self.at.min(self.bytes.len());
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
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c >= 0x80
}

fn is_ident_continue(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80
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
        let (first, state) =
            tokens(Lang::Rust, LineState::Normal, "let a = 1; /* start /* nested */");
        assert_eq!(first.last(), Some(&("/* start /* nested */", Token::Comment)));
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
        assert_eq!(rest, vec![("c */", Comment)], "JS block comments do not nest");
        assert_eq!(state, LineState::Normal);
    }

    #[test]
    fn languages_from_fence_tags_and_extensions() {
        assert_eq!(Lang::from_tag("rust"), Some(Lang::Rust));
        assert_eq!(Lang::from_tag("Py"), Some(Lang::Python));
        assert_eq!(Lang::from_tag("tsx"), Some(Lang::TypeScript));
        assert_eq!(Lang::from_tag("mjs"), Some(Lang::JavaScript));
        assert_eq!(Lang::from_tag("toml"), None);
        assert_eq!(
            Lang::from_path(std::path::Path::new("C:/x/main.rs")),
            Some(Lang::Rust)
        );
    }

    #[test]
    fn unicode_and_unfinished_fragments_never_panic() {
        for lang in [Lang::Rust, Lang::Python, Lang::JavaScript, Lang::TypeScript] {
            for text in [
                "let café = \"ñandú\"; // 雪 😀",
                "\"unterminated \\",
                "'",
                "r#",
                "/*",
                "\"\"\"",
                "`",
                "#[",
                "@",
                "x = 'a",
                "'é",
            ] {
                let mut state = LineState::Normal;
                for _ in 0..2 {
                    let (tokens, next) = highlight_line(lang, state, text);
                    for (range, _) in tokens {
                        assert!(range.end <= text.len());
                        assert!(
                            text.is_char_boundary(range.start) && text.is_char_boundary(range.end)
                        );
                    }
                    state = next;
                }
            }
        }
    }
}
