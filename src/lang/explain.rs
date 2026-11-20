// `qxide explain`: one entry per error code, read from docs/error-codes.md. That catalog is the
// single source; tests keep it in sync with `Code::ALL` and the design document's code tables.

const CATALOG: &str = include_str!("../../docs/error-codes.md");

/// Codes that are no longer reported but keep their entry; numbers are never reused.
#[cfg(test)]
pub const RETIRED: &[&str] = &["Q0219"];

/// `q0302` and `Q0302` name the same code; anything else that is not `Q` plus four digits is rejected.
pub fn normalize(arg: &str) -> Option<String> {
    let code = arg.trim().to_ascii_uppercase();
    let digits = code.strip_prefix('Q')?;
    (digits.len() == 4 && digits.bytes().all(|b| b.is_ascii_digit())).then_some(code)
}

/// Every `## Qdddd: title` section of the catalog as (code, text), with `\n` line endings.
pub fn entries() -> Vec<(String, String)> {
    let text = CATALOG.replace("\r\n", "\n");
    let mut out = Vec::new();
    for section in text.split("\n## ").skip(1) {
        let Some((code, _)) = section.split_once(':') else {
            continue;
        };
        if normalize(code).as_deref() == Some(code) {
            out.push((code.to_string(), section.trim_end().to_string()));
        }
    }
    out
}

/// The catalog entry for `code` (already normalized), ready to print.
pub fn explain(code: &str) -> Option<String> {
    entries()
        .into_iter()
        .find(|(c, _)| c == code)
        .map(|(_, text)| format!("{text}\n"))
}
