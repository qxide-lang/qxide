// Package manager behind `qxide add/remove/install/publish/search` (docs/design/qx-packages.md).
// Binary-only like `lang`: manifests, lock files, semver resolution, the registry index, package
// archives with sha256 checksums, and the local cache under `~/.qxide`.

pub mod archive;
pub mod cache;
pub mod cli;
pub mod index;
pub mod lock;
pub mod manifest;
mod msg;
pub mod project;
pub mod resolve;

#[cfg(test)]
mod tests;

use std::fmt::Write as _;

/// Stable codes of package errors (Q06xx); tests match on these, never on message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    ManifestNotFound,
    InvalidManifest,
    InvalidLock,
    PackageNotFound,
    NoMatchingVersion,
    VersionConflict,
    ChecksumMismatch,
    RegistryUnavailable,
    InvalidIndex,
    BadArchive,
    NotInstalled,
    LockOutdated,
    LanguageVersion,
    PublishRejected,
    Offline,
    Io,
    NotADependency,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        use Code::*;
        match self {
            ManifestNotFound => "Q0601",
            InvalidManifest => "Q0602",
            InvalidLock => "Q0603",
            PackageNotFound => "Q0604",
            NoMatchingVersion => "Q0605",
            VersionConflict => "Q0606",
            ChecksumMismatch => "Q0607",
            RegistryUnavailable => "Q0608",
            InvalidIndex => "Q0609",
            BadArchive => "Q0610",
            NotInstalled => "Q0611",
            LockOutdated => "Q0612",
            LanguageVersion => "Q0613",
            PublishRejected => "Q0614",
            Offline => "Q0615",
            Io => "Q0616",
            NotADependency => "Q0617",
        }
    }
}

/// A package error: no source span, just a message with optional notes and help.
#[derive(Debug, Clone)]
pub struct Error {
    pub code: Code,
    pub message: String,
    pub notes: Vec<String>,
    pub help: Option<String>,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(code: Code, message: String) -> Self {
        Self {
            code,
            message,
            notes: Vec::new(),
            help: msg::help(code),
        }
    }

    pub fn note(mut self, note: String) -> Self {
        self.notes.push(note);
        self
    }

    /// Filesystem failure at `path`.
    pub fn io(path: &std::path::Path, err: &std::io::Error) -> Self {
        Self::new(
            Code::Io,
            msg::io_error(&path.display().to_string(), &err.to_string()),
        )
    }

    /// Messages quote registry, archive, and file text, so control characters are escaped.
    pub fn render(&self) -> String {
        use crate::lang::printable;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "error[{}]: {}",
            self.code.as_str(),
            printable(&self.message)
        );
        for n in &self.notes {
            let _ = writeln!(out, "note: {}", printable(n));
        }
        if let Some(h) = &self.help {
            let _ = writeln!(out, "help: {}", printable(h));
        }
        out
    }
}

/// Lowercase hex of a sha256 digest.
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(data);
    let mut s = String::with_capacity(64);
    for b in digest.iter() {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Package names: lowercase ASCII letters, digits and single `-`, starting with a letter, at
/// most 64 characters. `_` is not allowed, so the import name (`-` -> `_`) is unambiguous.
pub fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && !name.ends_with('-')
        && !name.contains("--")
}

/// The name a package is imported by: `bell-utils` -> `bell_utils`.
pub fn import_name(name: &str) -> String {
    name.replace('-', "_")
}

/// Version of this `qxide` binary, checked against `qxide = "..."` requirements.
pub fn language_version() -> semver::Version {
    semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is semver")
}
