// The `.qxpak` container of a built app (docs/design/qx-app.md §9): written by `qxide app build`,
// read by the `qxide-app` runtime from the end of its own executable. Standard library only, so
// the runtime can include this file as is.
//
// Layout: entries `[u32 path length][path, UTF-8][u64 data length][data]`, then a 24-byte footer
// `[u64 offset of the first entry][u32 entry count][u32 format version][8-byte magic]`, all
// little-endian. The offset is absolute within the file, so the container can follow any prefix
// (the runtime executable).

use std::collections::BTreeMap;

pub const MAGIC: &[u8; 8] = b"QXAPPK01";
pub const FORMAT: u32 = 1;
const FOOTER: usize = 24;
/// The manifest entry every bundle has.
pub const MANIFEST: &str = "app.json";

/// Appends a container holding `files` to `out`, whose current length is where it starts.
pub fn write(out: &mut Vec<u8>, files: &BTreeMap<String, Vec<u8>>) {
    let start = out.len() as u64;
    for (path, data) in files {
        out.extend_from_slice(&(path.len() as u32).to_le_bytes());
        out.extend_from_slice(path.as_bytes());
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
        out.extend_from_slice(data);
    }
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&(files.len() as u32).to_le_bytes());
    out.extend_from_slice(&FORMAT.to_le_bytes());
    out.extend_from_slice(MAGIC);
}

/// Why a container could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleError {
    /// No footer: the executable has no app attached.
    Missing,
    /// A footer from another container format version.
    Format(u32),
    /// Truncated or inconsistent data.
    Corrupt,
}

/// Reads the container at the end of `bytes`.
pub fn read(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, BundleError> {
    if bytes.len() < FOOTER || &bytes[bytes.len() - 8..] != MAGIC {
        return Err(BundleError::Missing);
    }
    let footer = &bytes[bytes.len() - FOOTER..];
    let start = u64::from_le_bytes(footer[0..8].try_into().unwrap());
    let count = u32::from_le_bytes(footer[8..12].try_into().unwrap());
    let format = u32::from_le_bytes(footer[12..16].try_into().unwrap());
    if format != FORMAT {
        return Err(BundleError::Format(format));
    }
    let end = bytes.len() - FOOTER;
    let mut pos = usize::try_from(start).map_err(|_| BundleError::Corrupt)?;
    let mut files = BTreeMap::new();
    let take = |pos: &mut usize, n: usize| -> Result<&[u8], BundleError> {
        let stop = pos
            .checked_add(n)
            .filter(|&s| s <= end)
            .ok_or(BundleError::Corrupt)?;
        let s = &bytes[*pos..stop];
        *pos = stop;
        Ok(s)
    };
    for _ in 0..count {
        let n = u32::from_le_bytes(take(&mut pos, 4)?.try_into().unwrap()) as usize;
        let path = std::str::from_utf8(take(&mut pos, n)?)
            .map_err(|_| BundleError::Corrupt)?
            .to_string();
        let len = u64::from_le_bytes(take(&mut pos, 8)?.try_into().unwrap());
        let len = usize::try_from(len).map_err(|_| BundleError::Corrupt)?;
        let data = take(&mut pos, len)?.to_vec();
        files.insert(path, data);
    }
    if pos != end {
        return Err(BundleError::Corrupt);
    }
    Ok(files)
}

/// Bundle paths use `/`; module files are looked up with OS paths, so normalise those.
pub fn key(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_after_a_prefix() {
        let mut files = BTreeMap::new();
        files.insert(MANIFEST.to_string(), b"{}".to_vec());
        files.insert("ui/index.html".to_string(), "<p>é</p>".as_bytes().to_vec());
        files.insert("empty".to_string(), Vec::new());
        let mut out = b"MZ fake runtime".to_vec();
        write(&mut out, &files);
        assert_eq!(read(&out), Ok(files));
    }

    #[test]
    fn rejects_missing_foreign_and_truncated_containers() {
        assert_eq!(read(b"just an exe"), Err(BundleError::Missing));
        let mut files = BTreeMap::new();
        files.insert("a".to_string(), vec![1, 2, 3]);
        let mut out = Vec::new();
        write(&mut out, &files);
        let mut other = out.clone();
        let n = other.len();
        other[n - 12..n - 8].copy_from_slice(&9u32.to_le_bytes());
        assert_eq!(read(&other), Err(BundleError::Format(9)));
        let mut cut = out[..4].to_vec();
        cut.extend_from_slice(&out[out.len() - FOOTER..]);
        assert_eq!(read(&cut), Err(BundleError::Corrupt));
    }
}
