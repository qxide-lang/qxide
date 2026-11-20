// Diagnostics: stable codes, byte spans, rustc-style rendering with line/column.

use std::fmt::Write as _;

/// Byte range in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }
}

/// Stable error codes; tests match on these, never on message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    UnexpectedChar,
    UnterminatedString,
    InvalidEscape,
    IntTooLarge,
    RemovedSyntax,
    UnknownMethod,
    AsIntTooWide,
    UnexpectedToken,
    TooDeep,
    ReservedKeyword,
    UnknownType,
    BadFormat,
    UnknownVar,
    UnknownFn,
    Arity,
    TypeMismatch,
    Duplicate,
    BadMain,
    ImmutableAssign,
    NotIndexable,
    FormatArgCount,
    PrintlnFormat,
    NotPrintable,
    MissingReturn,
    StringNotAllowed,
    BadCondition,
    BuiltinRedefined,
    BitAtBuildTime,
    QuantumSignature,
    UseAfterConsume,
    NeverConsumed,
    DuplicateQubit,
    BranchMismatch,
    ConsumedInLoop,
    PartialMove,
    QuantumAssign,
    ForbiddenInBitIf,
    OuterAssignInBitIf,
    ShadowLive,
    PlaceRequired,
    TempDropped,
    Overflow,
    DivByZero,
    IndexOutOfBounds,
    RecursionLimit,
    Simulation,
    NoReturn,
    NegativeSize,
    TooManyQubits,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        use Code::*;
        match self {
            UnexpectedChar => "Q0101",
            UnterminatedString => "Q0102",
            InvalidEscape => "Q0103",
            IntTooLarge => "Q0104",
            RemovedSyntax => "Q0110",
            UnknownMethod => "Q0220",
            AsIntTooWide => "Q0410",
            UnexpectedToken => "Q0105",
            TooDeep => "Q0106",
            ReservedKeyword => "Q0107",
            UnknownType => "Q0108",
            BadFormat => "Q0109",
            UnknownVar => "Q0201",
            UnknownFn => "Q0202",
            Arity => "Q0203",
            TypeMismatch => "Q0204",
            Duplicate => "Q0205",
            BadMain => "Q0206",
            ImmutableAssign => "Q0207",
            NotIndexable => "Q0209",
            FormatArgCount => "Q0210",
            PrintlnFormat => "Q0211",
            NotPrintable => "Q0212",
            MissingReturn => "Q0213",
            StringNotAllowed => "Q0214",
            BadCondition => "Q0215",
            BuiltinRedefined => "Q0217",
            BitAtBuildTime => "Q0218",
            QuantumSignature => "Q0219",
            UseAfterConsume => "Q0301",
            NeverConsumed => "Q0302",
            DuplicateQubit => "Q0303",
            BranchMismatch => "Q0304",
            ConsumedInLoop => "Q0305",
            PartialMove => "Q0306",
            QuantumAssign => "Q0307",
            ForbiddenInBitIf => "Q0308",
            OuterAssignInBitIf => "Q0309",
            ShadowLive => "Q0310",
            PlaceRequired => "Q0311",
            TempDropped => "Q0312",
            Overflow => "Q0401",
            DivByZero => "Q0402",
            IndexOutOfBounds => "Q0403",
            RecursionLimit => "Q0405",
            Simulation => "Q0406",
            NoReturn => "Q0407",
            NegativeSize => "Q0408",
            TooManyQubits => "Q0409",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Note {
    pub span: Option<Span>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub code: Code,
    pub span: Span,
    pub message: String,
    pub notes: Vec<Note>,
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn new(code: Code, span: Span, message: String) -> Self {
        Self {
            code,
            span,
            message,
            notes: Vec::new(),
            help: super::msg::help(code),
        }
    }

    pub fn with_help(mut self, help: String) -> Self {
        self.help = Some(help);
        self
    }
}

/// Source text with precomputed line starts for offset -> (line, column) lookups.
pub struct SourceMap<'a> {
    pub name: &'a str,
    pub text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> SourceMap<'a> {
    pub fn new(name: &'a str, text: &'a str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Self {
            name,
            text,
            line_starts,
        }
    }

    /// 1-based line and column; columns count chars, not bytes.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.text.len());
        let line = self.line_starts.partition_point(|&s| s <= offset) - 1;
        let col = self.text[self.line_starts[line]..offset].chars().count() + 1;
        (line + 1, col)
    }

    fn line_text(&self, line: usize) -> &str {
        let start = self.line_starts[line - 1];
        let end = self
            .line_starts
            .get(line)
            .map_or(self.text.len(), |&e| e - 1);
        self.text[start..end].trim_end_matches('\r')
    }

    fn snippet(&self, out: &mut String, span: Span) {
        let (line, col) = self.line_col(span.start);
        let text = self.line_text(line);
        // Carets stop at the end of the first line for multi-line spans.
        let (end_line, end_col) = self.line_col(span.end);
        let width = if end_line == line {
            end_col.saturating_sub(col).max(1)
        } else {
            (text.chars().count() + 1).saturating_sub(col).max(1)
        };
        let gutter = line.to_string().len();
        let pad = " ".repeat(gutter);
        let _ = writeln!(out, "{pad}--> {}:{line}:{col}", self.name);
        let _ = writeln!(out, "{pad} |");
        let _ = writeln!(out, "{line} | {text}");
        let lead: String = text
            .chars()
            .take(col - 1)
            .map(|c| if c == '\t' { '\t' } else { ' ' })
            .collect();
        let _ = writeln!(out, "{pad} | {lead}{}", "^".repeat(width));
    }

    pub fn render(&self, d: &Diagnostic) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "error[{}]: {}", d.code.as_str(), d.message);
        self.snippet(&mut out, d.span);
        for n in &d.notes {
            let _ = writeln!(out, "note: {}", n.message);
            if let Some(s) = n.span {
                self.snippet(&mut out, s);
            }
        }
        if let Some(h) = &d.help {
            let _ = writeln!(out, "help: {h}");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_counts_chars_and_accepts_crlf() {
        let src = "ab\r\n큐비트 x\r\nz";
        let map = SourceMap::new("t.qx", src);
        assert_eq!(map.line_col(0), (1, 1));
        let x = src.find('x').unwrap();
        assert_eq!(map.line_col(x), (2, 5));
        assert_eq!(map.line_col(src.len()), (3, 2));
        assert_eq!(map.line_text(2), "큐비트 x");
    }
}
