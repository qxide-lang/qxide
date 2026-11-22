// Diagnostics: stable codes, byte spans, rustc-style rendering with line/column.

use std::borrow::Cow;
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
    LoopControl => "Q0224",
    BadArrayElement => "Q0225",
    UnknownField => "Q0226",
    BadFieldType => "Q0227",
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
    StepLimit => "Q0413",
    OpLimit => "Q0414",
    OutputLimit => "Q0415",
    CondDepth => "Q0416",
    SimWorkLimit => "Q0417",
    TooManyDiagnostics => "Q0418",
    ParseNumber => "Q0419",
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
    StdUnknown => "Q0701",
    StdUnavailable => "Q0702",
    StdDenied => "Q0703",
    StdIo => "Q0704",
    StdPath => "Q0705",
    StdArgument => "Q0706",
    StdHttp => "Q0707",
    StdUrl => "Q0708",
    UnknownCommand => "Q0711",
    BadArguments => "Q0712",
    Cancelled => "Q0713",
    BadCommand => "Q0714",
    BadAppConfig => "Q0715",
    ResultTooLarge => "Q0716",
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

    // `max_chars` caps the echoed line: a longer line is shown as a window around `col` with `...`
    // on the cut sides, so one diagnostic never repeats a whole huge line (M-01).
    fn snippet(&self, out: &mut String, span: Span, max_chars: usize) {
        let f = self.file(span.start);
        let (line, col) = self.line_col(span.start);
        let text = Self::line_text(f, line);
        let len = text.chars().count();
        // Carets stop at the end of the first line for multi-line spans.
        let (end_line, end_col) = self.line_col(span.end.max(span.start));
        let width = if end_line == line && span.end <= f.base + f.text.len() {
            end_col.saturating_sub(col).max(1)
        } else {
            (len + 1).saturating_sub(col).max(1)
        };
        // Escape first, then window: positions count displayed characters, an escaped control
        // character counts as its whole `\u{..}` escape, and the window never splits an escape.
        let shown_width = |c: char| {
            if hidden(c) {
                c.escape_unicode().len()
            } else {
                1
            }
        };
        // Displayed width of the first `chars` characters; columns past the end count one each.
        let display = |chars: usize| -> usize {
            text.chars().take(chars).map(shown_width).sum::<usize>() + chars.saturating_sub(len)
        };
        let max = max_chars.max(1);
        let total = display(len);
        let start = display(col - 1);
        let end = display(col - 1 + width);
        // Characters `skip..skip + take` of the line are shown; `from` is where they start on screen.
        let (skip, take, from) = if total <= max {
            (0, len, 0)
        } else {
            let target = start.saturating_sub(max / 2).min(total - max);
            let (mut skip, mut from) = (0, 0);
            for w in text.chars().map(shown_width) {
                if from >= target {
                    break;
                }
                from += w;
                skip += 1;
            }
            let (mut take, mut used) = (0, 0);
            for w in text.chars().skip(skip).map(shown_width) {
                if used + w > max {
                    break;
                }
                used += w;
                take += 1;
            }
            (skip, take, from)
        };
        let before = if skip > 0 { "..." } else { "" };
        let after = if skip + take < len { "..." } else { "" };
        let raw: String = text.chars().skip(skip).take(take).collect();
        let shown = printable(&raw);
        let shown_len = shown.chars().count();
        // The caret column is at most one past the window (a span at the end of the line).
        let at = start.saturating_sub(from).min(shown_len);
        let width = end
            .saturating_sub(start)
            .max(1)
            .min((shown_len + 1).saturating_sub(at))
            .max(1);
        let line = f.labels.as_ref().map_or(line, |l| l[line - 1]);
        let gutter = line.to_string().len();
        let pad = " ".repeat(gutter);
        let _ = writeln!(out, "{pad}--> {}:{line}:{col}", f.name);
        let _ = writeln!(out, "{pad} |");
        let _ = writeln!(out, "{line} | {before}{shown}{after}");
        let lead: String = " ".repeat(before.len())
            + &shown
                .chars()
                .take(at)
                .map(|c| if c == '\t' { '\t' } else { ' ' })
                .collect::<String>();
        let _ = writeln!(out, "{pad} | {lead}{}", "^".repeat(width));
    }

    pub fn render(&self, d: &Diagnostic, limits: &super::Limits) -> String {
        let max_chars = limits.max_line_chars;
        let mut out = String::new();
        let _ = writeln!(out, "error[{}]: {}", d.code.as_str(), printable(&d.message));
        if !d.no_snippet {
            self.snippet(&mut out, d.span, max_chars);
        }
        for n in &d.notes {
            let _ = writeln!(out, "note: {}", printable(&n.message));
            if let Some(s) = n.span {
                self.snippet(&mut out, s, max_chars);
            }
        }
        if let Some(h) = &d.help {
            let _ = writeln!(out, "help: {}", printable(h));
        }
        out
    }
}

/// `s` with every control character except tab (C0, DEL, and C1 such as CSI) shown as a
/// `\u{..}` escape, so text from a source file, an archive, or a registry cannot drive the
/// terminal. Used for everything printed that did not come from this binary.
pub fn printable(s: &str) -> Cow<'_, str> {
    if !s.chars().any(hidden) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if hidden(c) {
            let _ = write!(out, "{}", c.escape_unicode());
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// Control characters other than tab, which `printable` shows as escapes.
fn hidden(c: char) -> bool {
    c.is_control() && c != '\t'
}

/// Renders `errors` one after another, separated by blank lines. At most `max_diagnostics` are
/// rendered and the report stops before it would pass `max_output_bytes`; a closing `Q0418`
/// diagnostic then says how many were left out (M-01).
pub fn render_all(
    errors: &[Diagnostic],
    limits: &super::Limits,
    render: impl Fn(&Diagnostic) -> String,
) -> String {
    let max_bytes = usize::try_from(limits.max_output_bytes).unwrap_or(usize::MAX);
    let mut out = String::new();
    let (mut shown, mut cut) = (0, false);
    for d in errors.iter().take(limits.max_diagnostics) {
        let text = render(d);
        let sep = usize::from(shown > 0);
        if out.len() + sep + text.len() > max_bytes {
            // Keep the start of a first diagnostic that alone is too long, so the report says something.
            if shown == 0 {
                let mut end = max_bytes.min(text.len());
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                out.push_str(&text[..end]);
                out.push('\n');
                shown = 1;
            }
            cut = true;
            break;
        }
        if sep == 1 {
            out.push('\n');
        }
        out.push_str(&text);
        shown += 1;
    }
    let omitted = errors.len() - shown;
    if cut || omitted > 0 {
        let message = if cut {
            super::msg::diagnostics_too_long(omitted, limits.max_output_bytes)
        } else {
            super::msg::too_many_diagnostics(omitted, limits.max_diagnostics)
        };
        let mut summary = Diagnostic::new(Code::TooManyDiagnostics, Span::default(), message);
        summary.no_snippet = true;
        out.push('\n');
        out.push_str(&render(&summary));
    }
    out
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

    // M-04 in docs/security/PKG_CLIENT_FIXES.md: control characters from a source file are shown
    // escaped, and the carets stay under the span.
    #[test]
    fn rendered_diagnostics_hide_terminal_escapes() {
        let src = "x\u{1b}[2K\u{7}\u{9b}1A\ty;\n";
        let map = SourceMap::new("t.qx", src);
        let y = src.find('y').unwrap();
        let d = Diagnostic::new(
            Code::UnknownVar,
            Span::new(y, y + 1),
            "m\u{1b}]0;t\u{7}".into(),
        );
        let out = map.render(&d, &crate::lang::Limits::CLI);
        assert!(
            !out.chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t'),
            "{out:?}"
        );
        let echoed = out.lines().find(|l| l.starts_with("1 | ")).unwrap();
        let carets = out.lines().find(|l| l.contains('^')).unwrap();
        assert_eq!(echoed.find('y'), carets.find('^'), "{out}");
    }

    // A long line is escaped first and then windowed: the echoed text stays within
    // `max_line_chars` displayed characters and never ends inside a `\u{..}` escape.
    #[test]
    fn long_lines_are_windowed_after_escaping() {
        let limits = crate::lang::Limits {
            max_line_chars: 20,
            ..crate::lang::Limits::CLI
        };
        for gap in 0..12 {
            let src = format!(
                "{}{}y{}\n",
                "\u{1b}".repeat(30),
                "a".repeat(gap),
                "\u{9b}".repeat(30)
            );
            let map = SourceMap::new("t.qx", &src);
            let y = src.find('y').unwrap();
            let d = Diagnostic::new(Code::UnknownVar, Span::new(y, y + 1), "m".into());
            let out = map.render(&d, &limits);
            let echoed = out.lines().find(|l| l.starts_with("1 | ")).unwrap();
            let body = echoed["1 | ".len()..]
                .trim_start_matches("...")
                .trim_end_matches("...");
            assert!(body.chars().count() <= 20, "{out}");
            assert!(body.contains('y'), "{out}");
            // Every escape is whole: the text splits into complete escapes, `a`s, and `y`.
            let rest = body.replace("\\u{1b}", "").replace("\\u{9b}", "");
            assert!(rest.chars().all(|c| c == 'a' || c == 'y'), "{out}");
            let carets = out.lines().find(|l| l.contains('^')).unwrap();
            assert_eq!(echoed.find('y'), carets.find('^'), "{out}");
            assert_eq!(carets.matches('^').count(), 1, "{out}");
        }
    }
}
