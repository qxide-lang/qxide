// Hand-written lexer: `//` comments, decimal integers and doubles, string literals, identifiers, punctuation.

use super::diag::{Code, Diagnostic, Span};
use super::msg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    // `9223372036854775808`, one past `i64::MAX`. The parser accepts it only right after a unary
    // minus, which makes `i64::MIN` (as in Java); anywhere else it is `Q0104`.
    IntMinAbs,
    // f64 bit pattern, so `Tok` can stay `Eq`.
    Double(u64),
    Str(String),
    // Keywords
    Final,
    Var,
    Struct,
    Enum,
    Match,
    Consume,
    Import,
    Public,
    If,
    Else,
    For,
    In,
    While,
    Break,
    Continue,
    Return,
    True,
    False,
    // Punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Colon,
    Arrow,
    // `=>` in `match` arms.
    FatArrow,
    // Postfix `?`: early return of an `Err` or `None`.
    Question,
    DotDot,
    Dot,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    // Compound assignment: `+=`, `-=`, `*=`, `/=`, `%=`.
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    // Bitwise operators. `>>` and `>>>` are adjacent `>` tokens (they also close nested type
    // arguments); the parser joins them. `<<` is one token.
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    // `&=`, `|=`, `^=`, `<<=`, `>>=`, `>>>=`.
    AmpEq,
    PipeEq,
    CaretEq,
    ShlEq,
    ShrEq,
    UShrEq,
    Bang,
    Eq,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Eof,
}

impl Tok {
    /// Source spelling used in diagnostics; `None` for tokens described by category.
    pub fn symbol(&self) -> Option<&'static str> {
        use Tok::*;
        Some(match self {
            Final => "final",
            Var => "var",
            Struct => "struct",
            Enum => "enum",
            Match => "match",
            Consume => "consume",
            Import => "import",
            Public => "public",
            If => "if",
            Else => "else",
            For => "for",
            In => "in",
            While => "while",
            Break => "break",
            Continue => "continue",
            Return => "return",
            True => "true",
            False => "false",
            LParen => "(",
            RParen => ")",
            LBrace => "{",
            RBrace => "}",
            LBracket => "[",
            RBracket => "]",
            Comma => ",",
            Semi => ";",
            Colon => ":",
            Arrow => "->",
            FatArrow => "=>",
            Question => "?",
            DotDot => "..",
            Dot => ".",
            Plus => "+",
            Minus => "-",
            Star => "*",
            Slash => "/",
            Percent => "%",
            PlusEq => "+=",
            MinusEq => "-=",
            StarEq => "*=",
            SlashEq => "/=",
            PercentEq => "%=",
            Amp => "&",
            Pipe => "|",
            Caret => "^",
            Tilde => "~",
            Shl => "<<",
            AmpEq => "&=",
            PipeEq => "|=",
            CaretEq => "^=",
            ShlEq => "<<=",
            ShrEq => ">>=",
            UShrEq => ">>>=",
            Bang => "!",
            Eq => "=",
            EqEq => "==",
            NotEq => "!=",
            Lt => "<",
            Le => "<=",
            Gt => ">",
            Ge => ">=",
            AndAnd => "&&",
            OrOr => "||",
            Ident(_) | Int(_) | IntMinAbs | Double(_) | Str(_) | Eof => return None,
        })
    }

    /// How a found token is shown in "expected X, found Y" messages.
    pub fn describe(&self) -> String {
        match self {
            Tok::Ident(s) => format!("`{s}`"),
            Tok::Int(n) => format!("`{n}`"),
            Tok::IntMinAbs => "`9223372036854775808`".to_string(),
            Tok::Double(b) => format!("`{:?}`", f64::from_bits(*b)),
            Tok::Str(_) => msg::desc_string(),
            Tok::Eof => msg::desc_eof(),
            t => format!("`{}`", t.symbol().unwrap_or_default()),
        }
    }
}

// Rust-style syntax removed in 0.5.0; reported with a migration hint.
pub const REMOVED: &[&str] = &["let", "fn", "mut"];

pub const RESERVED: &[&str] = &[
    "loop",
    "impl",
    "use",
    "mod",
    "const",
    "with_ancilla",
    "apply",
];

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub fn lex(src: &str) -> Result<Vec<Token>, Diagnostic> {
    lex_with_comments(src).map(|(toks, _)| toks)
}

/// Like `lex`, but also returns the span of every `//` comment (up to the line break).
pub fn lex_with_comments(src: &str) -> Result<(Vec<Token>, Vec<Span>), Diagnostic> {
    let mut comments = Vec::new();
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            comments.push(Span::new(start, i));
            continue;
        }
        let start = i;
        let tok = if c.is_ascii_alphabetic() || c == b'_' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &src[start..i];
            if REMOVED.contains(&word) {
                return Err(Diagnostic::new(
                    Code::RemovedSyntax,
                    Span::new(start, i),
                    msg::removed_syntax(word),
                ));
            }
            if RESERVED.contains(&word) {
                return Err(Diagnostic::new(
                    Code::ReservedKeyword,
                    Span::new(start, i),
                    msg::reserved_keyword(word),
                ));
            }
            match word {
                "final" => Tok::Final,
                "struct" => Tok::Struct,
                "enum" => Tok::Enum,
                "match" => Tok::Match,
                "var" => Tok::Var,
                "consume" => Tok::Consume,
                "import" => Tok::Import,
                "public" => Tok::Public,
                "if" => Tok::If,
                "else" => Tok::Else,
                "for" => Tok::For,
                "in" => Tok::In,
                "while" => Tok::While,
                "break" => Tok::Break,
                "continue" => Tok::Continue,
                "return" => Tok::Return,
                "true" => Tok::True,
                "false" => Tok::False,
                _ => Tok::Ident(word.to_string()),
            }
        } else if c.is_ascii_digit() {
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
            // A double needs digits on both sides of the dot, so `0..n` stays a range.
            let mut is_double = false;
            if bytes.get(i) == Some(&b'.') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
                is_double = true;
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                    i += 1;
                }
            }
            if matches!(bytes.get(i), Some(b'e' | b'E')) {
                let sign = usize::from(matches!(bytes.get(i + 1), Some(b'+' | b'-')));
                if bytes.get(i + 1 + sign).is_some_and(u8::is_ascii_digit) {
                    is_double = true;
                    i += 1 + sign;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let digits: String = src[start..i].chars().filter(|&c| c != '_').collect();
            if is_double {
                match digits.parse::<f64>() {
                    Ok(v) if v.is_finite() => Tok::Double(v.to_bits()),
                    _ => {
                        return Err(Diagnostic::new(
                            Code::IntTooLarge,
                            Span::new(start, i),
                            msg::double_out_of_range(),
                        ))
                    }
                }
            } else {
                match digits.parse::<i64>() {
                    Ok(n) => Tok::Int(n),
                    Err(_) if digits.trim_start_matches('0') == "9223372036854775808" => {
                        Tok::IntMinAbs
                    }
                    Err(_) => {
                        return Err(Diagnostic::new(
                            Code::IntTooLarge,
                            Span::new(start, i),
                            msg::int_too_large(),
                        ))
                    }
                }
            }
        } else if c == b'"' {
            i += 1;
            let mut s = String::new();
            loop {
                let Some(ch) = src[i..].chars().next() else {
                    return Err(Diagnostic::new(
                        Code::UnterminatedString,
                        Span::new(start, i),
                        msg::unterminated_string(),
                    ));
                };
                match ch {
                    '"' => {
                        i += 1;
                        break;
                    }
                    '\\' => {
                        let esc = src[i + 1..].chars().next();
                        // `\u001B` (Java): exactly four hex digits naming one Unicode scalar
                        // value, so terminal control sequences can be written in a string.
                        if esc == Some('u') {
                            let hex = src.get(i + 2..i + 6).unwrap_or("");
                            let ch = (hex.len() == 4 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
                                .then(|| u32::from_str_radix(hex, 16).ok())
                                .flatten()
                                .and_then(char::from_u32);
                            let Some(ch) = ch else {
                                let end = src[i + 2..]
                                    .char_indices()
                                    .take(4)
                                    .last()
                                    .map_or(i + 2, |(k, c)| i + 2 + k + c.len_utf8());
                                return Err(Diagnostic::new(
                                    Code::InvalidEscape,
                                    Span::new(i, end),
                                    msg::invalid_unicode_escape(),
                                ));
                            };
                            s.push(ch);
                            i += 6;
                            continue;
                        }
                        let decoded = match esc {
                            Some('n') => '\n',
                            Some('t') => '\t',
                            Some('r') => '\r',
                            Some('\\') => '\\',
                            Some('"') => '"',
                            Some(other) => {
                                return Err(Diagnostic::new(
                                    Code::InvalidEscape,
                                    Span::new(i, i + 1 + other.len_utf8()),
                                    msg::invalid_escape(other),
                                ))
                            }
                            None => {
                                return Err(Diagnostic::new(
                                    Code::UnterminatedString,
                                    Span::new(start, i + 1),
                                    msg::unterminated_string(),
                                ))
                            }
                        };
                        s.push(decoded);
                        i += 1 + esc.map_or(0, char::len_utf8);
                    }
                    _ => {
                        s.push(ch);
                        i += ch.len_utf8();
                    }
                }
            }
            Tok::Str(s)
        } else {
            let two = bytes.get(i + 1).copied();
            let rest = &bytes[i..];
            let (tok, len) = match (c, two) {
                _ if rest.starts_with(b">>>=") => (Tok::UShrEq, 4),
                _ if rest.starts_with(b">>=") => (Tok::ShrEq, 3),
                _ if rest.starts_with(b"<<=") => (Tok::ShlEq, 3),
                (b'<', Some(b'<')) => (Tok::Shl, 2),
                (b'&', Some(b'=')) => (Tok::AmpEq, 2),
                (b'|', Some(b'=')) => (Tok::PipeEq, 2),
                (b'^', Some(b'=')) => (Tok::CaretEq, 2),
                (b'-', Some(b'>')) => (Tok::Arrow, 2),
                (b'.', Some(b'.')) => (Tok::DotDot, 2),
                (b'.', _) => (Tok::Dot, 1),
                (b'=', Some(b'=')) => (Tok::EqEq, 2),
                (b'=', Some(b'>')) => (Tok::FatArrow, 2),
                (b'!', Some(b'=')) => (Tok::NotEq, 2),
                (b'<', Some(b'=')) => (Tok::Le, 2),
                (b'>', Some(b'=')) => (Tok::Ge, 2),
                (b'&', Some(b'&')) => (Tok::AndAnd, 2),
                (b'|', Some(b'|')) => (Tok::OrOr, 2),
                (b'+', Some(b'=')) => (Tok::PlusEq, 2),
                (b'-', Some(b'=')) => (Tok::MinusEq, 2),
                (b'*', Some(b'=')) => (Tok::StarEq, 2),
                (b'/', Some(b'=')) => (Tok::SlashEq, 2),
                (b'%', Some(b'=')) => (Tok::PercentEq, 2),
                (b'(', _) => (Tok::LParen, 1),
                (b')', _) => (Tok::RParen, 1),
                (b'{', _) => (Tok::LBrace, 1),
                (b'}', _) => (Tok::RBrace, 1),
                (b'[', _) => (Tok::LBracket, 1),
                (b']', _) => (Tok::RBracket, 1),
                (b',', _) => (Tok::Comma, 1),
                (b';', _) => (Tok::Semi, 1),
                (b':', _) => (Tok::Colon, 1),
                (b'?', _) => (Tok::Question, 1),
                (b'+', _) => (Tok::Plus, 1),
                (b'-', _) => (Tok::Minus, 1),
                (b'*', _) => (Tok::Star, 1),
                (b'/', _) => (Tok::Slash, 1),
                (b'%', _) => (Tok::Percent, 1),
                (b'&', _) => (Tok::Amp, 1),
                (b'|', _) => (Tok::Pipe, 1),
                (b'^', _) => (Tok::Caret, 1),
                (b'~', _) => (Tok::Tilde, 1),
                (b'!', _) => (Tok::Bang, 1),
                (b'=', _) => (Tok::Eq, 1),
                (b'<', _) => (Tok::Lt, 1),
                (b'>', _) => (Tok::Gt, 1),
                _ => {
                    let ch = src[i..].chars().next().unwrap_or('?');
                    return Err(Diagnostic::new(
                        Code::UnexpectedChar,
                        Span::new(i, i + ch.len_utf8()),
                        msg::unexpected_char(ch),
                    ));
                }
            };
            i += len;
            tok
        };
        out.push(Token {
            tok,
            span: Span::new(start, i),
        });
    }
    out.push(Token {
        tok: Tok::Eof,
        span: Span::new(src.len(), src.len()),
    });
    Ok((out, comments))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        lex(src).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn lexes_keywords_operators_and_literals() {
        assert_eq!(
            toks("final int f() { 1_000 <= x && !y } // c"),
            vec![
                Tok::Final,
                Tok::Ident("int".into()),
                Tok::Ident("f".into()),
                Tok::LParen,
                Tok::RParen,
                Tok::LBrace,
                Tok::Int(1000),
                Tok::Le,
                Tok::Ident("x".into()),
                Tok::AndAnd,
                Tok::Bang,
                Tok::Ident("y".into()),
                Tok::RBrace,
                Tok::Eof,
            ]
        );
        assert_eq!(
            toks(r#""a\n\"{}\"" 0..3"#),
            vec![
                Tok::Str("a\n\"{}\"".into()),
                Tok::Int(0),
                Tok::DotDot,
                Tok::Int(3),
                Tok::Eof
            ]
        );
        assert_eq!(
            toks("var consume variable"),
            vec![
                Tok::Var,
                Tok::Consume,
                Tok::Ident("variable".into()),
                Tok::Eof
            ]
        );
        let d = |v: f64| Tok::Double(v.to_bits());
        assert_eq!(
            toks("0.5 1e3 2.5E-2 1..2 m.asInt"),
            vec![
                d(0.5),
                d(1000.0),
                d(0.025),
                Tok::Int(1),
                Tok::DotDot,
                Tok::Int(2),
                Tok::Ident("m".into()),
                Tok::Dot,
                Tok::Ident("asInt".into()),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn lex_errors_have_codes_and_spans() {
        let e = lex("int x = 1 # 2;").unwrap_err();
        assert_eq!((e.code, e.span), (Code::UnexpectedChar, Span::new(10, 11)));
        assert_eq!(lex("\"abc").unwrap_err().code, Code::UnterminatedString);
        assert_eq!(lex(r#""\q""#).unwrap_err().code, Code::InvalidEscape);
        assert_eq!(
            lex("99999999999999999999").unwrap_err().code,
            Code::IntTooLarge
        );
        assert_eq!(lex("loop x").unwrap_err().code, Code::ReservedKeyword);
        // Rust-style syntax removed in 0.5.0 gets a migration hint.
        for word in ["let x = 1;", "fn main() {}", "mut"] {
            assert_eq!(lex(word).unwrap_err().code, Code::RemovedSyntax);
        }
        assert_eq!(lex("1e999").unwrap_err().code, Code::IntTooLarge);
        // Non-ASCII characters outside strings are rejected without panicking on char boundaries.
        assert_eq!(lex("큐").unwrap_err().code, Code::UnexpectedChar);
    }
}
