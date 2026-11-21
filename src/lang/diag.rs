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

macro_rules! codes {
    ($($name:ident => $code:literal,)*) => {
        /// Stable error codes; tests match on these, never on message text.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Code {
            $($name,)*
        }

        impl Code {
            /// Every code the compiler can report, in numeric order.
            #[allow(dead_code)]
            pub const ALL: &'static [Code] = &[$(Code::$name,)*];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Code::$name => $code,)*
                }
            }
        }
    };
}

codes! {
    UnexpectedChar => "Q0101",
    UnterminatedString => "Q0102",
    InvalidEscape => "Q0103",
    IntTooLarge => "Q0104",
    UnexpectedToken => "Q0105",
    TooDeep => "Q0106",
    ReservedKeyword => "Q0107",
    UnknownType => "Q0108",
    BadFormat => "Q0109",
    RemovedSyntax => "Q0110",
    VarNotAllowed => "Q0111",
    TypeNameAsIdent => "Q0112",
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
    UnknownMethod => "Q0220",
    ConsumeNonQuantum => "Q0221",
    VarInfer => "Q0222",
    InvalidCast => "Q0223",
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
    BorrowedConsume => "Q0313",
    ConditionalCall => "Q0314",
    ShortCircuitConsume => "Q0315",
    Overflow => "Q0401",
    DivByZero => "Q0402",
    IndexOutOfBounds => "Q0403",
    RecursionLimit => "Q0405",
    Simulation => "Q0406",
    NoReturn => "Q0407",
    NegativeSize => "Q0408",
    TooManyQubits => "Q0409",
    AsIntTooWide => "Q0410",
    NonFiniteAngle => "Q0411",
    Internal => "Q0412",
    UnknownPackage => "Q0501",
    ModuleNotFound => "Q0502",
    UnknownImport => "Q0503",
    PrivateFunction => "Q0504",
    ImportCycle => "Q0505",
    ImportConflict => "Q0506",
    AmbiguousName => "Q0507",
    ImportOutsideProject => "Q0508",
    BadImportPath => "Q0509",
    MisplacedImport => "Q0510",
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
    // Errors about a file as a whole (for example a missing module) have no source line to show.
    pub no_snippet: bool,
}

impl Diagnostic {
    pub fn new(code: Code, span: Span, message: String) -> Self {
        Self {
            code,
            span,
            message,
            notes: Vec::new(),
            help: super::msg::help(code),
            no_snippet: false,
        }
    }

    pub fn with_help(mut self, help: String) -> Self {
        self.help = Some(help);
        self
    }
}

struct File {
    name: String,
    text: String,
    // Offset of the file's first byte in the shared span space.
    base: usize,
    line_starts: Vec<usize>,
    // Line numbers to display instead of 1, 2, 3, ... (the REPL numbers lines as they were typed).
    labels: Option<Vec<usize>>,
}

/// Every loaded source file in one offset space, so a `Span` needs no file id: the entry file
/// starts at 0 and each imported file follows the previous one with a one-byte gap.
pub struct SourceMap {
    files: Vec<File>,
}

impl SourceMap {
    pub fn new(name: &str, text: &str) -> Self {
        let mut map = Self { files: Vec::new() };
        map.add(name, text);
        map
    }

    /// Adds a file and returns the base offset to add to its local spans.
    pub fn add(&mut self, name: &str, text: &str) -> usize {
        let base = self.files.last().map_or(0, |f| f.base + f.text.len() + 1);
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        self.files.push(File {
            name: name.to_string(),
            text: text.to_string(),
            base,
            line_starts,
            labels: None,
        });
        base
    }

    /// Displays line `i + 1` of the entry file as `labels[i]`; needs one label per line.
    pub fn with_labels(mut self, labels: Vec<usize>) -> Self {
        let f = &mut self.files[0];
        assert_eq!(labels.len(), f.line_starts.len(), "one label per line");
        f.labels = Some(labels);
        self
    }

    fn file(&self, offset: usize) -> &File {
        let i = self.files.partition_point(|f| f.base <= offset).max(1) - 1;
        &self.files[i]
    }

    /// Display name of the file that holds `offset`.
    pub fn file_name(&self, offset: usize) -> &str {
        &self.file(offset).name
    }

    /// Name, text and base offset of the file holding `offset` (editor support maps spans back
    /// to file-local positions with these).
    #[allow(dead_code)]
    pub fn file_at(&self, offset: usize) -> (&str, &str, usize) {
        let f = self.file(offset);
        (&f.name, &f.text, f.base)
    }

    /// 1-based line and column within the file holding `offset`; columns count chars, not bytes.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let f = self.file(offset);
        let local = (offset - f.base).min(f.text.len());
        let line = f.line_starts.partition_point(|&s| s <= local) - 1;
        let col = f.text[f.line_starts[line]..local].chars().count() + 1;
        (line + 1, col)
    }

    fn line_text(f: &File, line: usize) -> &str {
        let start = f.line_starts[line - 1];
        let end = f.line_starts.get(line).map_or(f.text.len(), |&e| e - 1);
        f.text[start..end].trim_end_matches('\r')
    }

    fn snippet(&self, out: &mut String, span: Span) {
        let f = self.file(span.start);
        let (line, col) = self.line_col(span.start);
        let text = Self::line_text(f, line);
        // Carets stop at the end of the first line for multi-line spans.
        let (end_line, end_col) = self.line_col(span.end.max(span.start));
        let width = if end_line == line && span.end <= f.base + f.text.len() {
            end_col.saturating_sub(col).max(1)
        } else {
            (text.chars().count() + 1).saturating_sub(col).max(1)
        };
        let line = f.labels.as_ref().map_or(line, |l| l[line - 1]);
        let gutter = line.to_string().len();
        let pad = " ".repeat(gutter);
        let _ = writeln!(out, "{pad}--> {}:{line}:{col}", f.name);
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
        if !d.no_snippet {
            self.snippet(&mut out, d.span);
        }
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
        assert_eq!(SourceMap::line_text(&map.files[0], 2), "큐비트 x");
    }

    #[test]
    fn later_files_have_their_own_lines() {
        let mut map = SourceMap::new("a.qx", "ab\ncd");
        let base = map.add("b.qx", "x\ny");
        assert_eq!(base, 6);
        assert_eq!(map.line_col(base + 2), (2, 1));
        assert_eq!(map.file_name(base), "b.qx");
        assert_eq!(map.file_name(5), "a.qx");
    }
}
