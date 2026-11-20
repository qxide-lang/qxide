// Hand-written lexer: `//` comments, decimal integers, string literals, identifiers, punctuation.

use super::diag::{Code, Diagnostic, Span};
use super::msg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Str(String),
    // Keywords
    Fn,
    Let,
    Mut,
    If,
    Else,
    For,
    In,
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
    DotDot,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
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
            Fn => "fn",
            Let => "let",
            Mut => "mut",
            If => "if",
            Else => "else",
            For => "for",
            In => "in",
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
            DotDot => "..",
            Plus => "+",
            Minus => "-",
            Star => "*",
            Slash => "/",
            Percent => "%",
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
            Ident(_) | Int(_) | Str(_) | Eof => return None,
        })
    }

    /// How a found token is shown in "expected X, found Y" messages.
    pub fn describe(&self) -> String {
        match self {
            Tok::Ident(s) => format!("`{s}`"),
            Tok::Int(n) => format!("`{n}`"),
            Tok::Str(_) => msg::desc_string(),
            Tok::Eof => msg::desc_eof(),
            t => format!("`{}`", t.symbol().unwrap_or_default()),
        }
    }
}

pub const RESERVED: &[&str] = &[
    "while",
    "loop",
    "break",
    "continue",
    "struct",
    "enum",
    "impl",
    "use",
    "mod",
    "const",
    "match",
    "with_ancilla",
    "apply",
];

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub fn lex(src: &str) -> Result<Vec<Token>, Diagnostic> {
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
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        let tok = if c.is_ascii_alphabetic() || c == b'_' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &src[start..i];
            if RESERVED.contains(&word) {
                return Err(Diagnostic::new(
                    Code::ReservedKeyword,
                    Span::new(start, i),
                    msg::reserved_keyword(word),
                ));
            }
            match word {
                "fn" => Tok::Fn,
                "let" => Tok::Let,
                "mut" => Tok::Mut,
                "if" => Tok::If,
                "else" => Tok::Else,
                "for" => Tok::For,
                "in" => Tok::In,
                "return" => Tok::Return,
                "true" => Tok::True,
                "false" => Tok::False,
                _ => Tok::Ident(word.to_string()),
            }
        } else if c.is_ascii_digit() {
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
            let digits: String = src[start..i].chars().filter(|&c| c != '_').collect();
            match digits.parse::<i64>() {
                Ok(n) => Tok::Int(n),
                Err(_) => {
                    return Err(Diagnostic::new(
                        Code::IntTooLarge,
                        Span::new(start, i),
                        msg::int_too_large(),
                    ))
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
                        let decoded = match esc {
                            Some('n') => '\n',
                            Some('t') => '\t',
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
            let (tok, len) = match (c, two) {
                (b'-', Some(b'>')) => (Tok::Arrow, 2),
                (b'.', Some(b'.')) => (Tok::DotDot, 2),
                (b'=', Some(b'=')) => (Tok::EqEq, 2),
                (b'!', Some(b'=')) => (Tok::NotEq, 2),
                (b'<', Some(b'=')) => (Tok::Le, 2),
                (b'>', Some(b'=')) => (Tok::Ge, 2),
                (b'&', Some(b'&')) => (Tok::AndAnd, 2),
                (b'|', Some(b'|')) => (Tok::OrOr, 2),
                (b'(', _) => (Tok::LParen, 1),
                (b')', _) => (Tok::RParen, 1),
                (b'{', _) => (Tok::LBrace, 1),
                (b'}', _) => (Tok::RBrace, 1),
                (b'[', _) => (Tok::LBracket, 1),
                (b']', _) => (Tok::RBracket, 1),
                (b',', _) => (Tok::Comma, 1),
                (b';', _) => (Tok::Semi, 1),
                (b':', _) => (Tok::Colon, 1),
                (b'+', _) => (Tok::Plus, 1),
                (b'-', _) => (Tok::Minus, 1),
                (b'*', _) => (Tok::Star, 1),
                (b'/', _) => (Tok::Slash, 1),
                (b'%', _) => (Tok::Percent, 1),
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
    Ok(out)
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
            toks("fn f() -> i64 { 1_000 <= x && !y } // c"),
            vec![
                Tok::Fn,
                Tok::Ident("f".into()),
                Tok::LParen,
                Tok::RParen,
                Tok::Arrow,
                Tok::Ident("i64".into()),
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
    }

    #[test]
    fn lex_errors_have_codes_and_spans() {
        let e = lex("let x = 1 # 2;").unwrap_err();
        assert_eq!((e.code, e.span), (Code::UnexpectedChar, Span::new(10, 11)));
        assert_eq!(lex("\"abc").unwrap_err().code, Code::UnterminatedString);
        assert_eq!(lex(r#""\q""#).unwrap_err().code, Code::InvalidEscape);
        assert_eq!(
            lex("99999999999999999999").unwrap_err().code,
            Code::IntTooLarge
        );
        assert_eq!(lex("while x").unwrap_err().code, Code::ReservedKeyword);
        // Non-ASCII characters outside strings are rejected without panicking on char boundaries.
        assert_eq!(lex("큐").unwrap_err().code, Code::UnexpectedChar);
    }
}
