// Bundled executables (`qxide build`): a copy of this `qxide` binary with a checked program
// appended. At startup `main` looks for the trailer at the very end of its own file; with one,
// the binary runs the embedded program instead of the command-line interface.
//
// Layout of the appended bytes: `payload | trailer`, where the 32-byte trailer is
// `payload length (u64 LE) | FNV-1a 64 checksum of the payload (u64 LE) | format (u32 LE) | MAGIC`.
// The checksum detects truncated or corrupted files; it is not a signature (see qx-build.md).

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::lang::{PackageRoot, Project};

pub const MAGIC: &[u8; 12] = b"QXIDE-BUNDLE";
const FORMAT: u32 = 1;
pub const TRAILER_LEN: usize = 32;
// Sources only: far above any real program, and small enough to read into memory at startup.
pub const MAX_PAYLOAD: u64 = 64 << 20;

/// The embedded program and the `qxide run` options fixed when it was built.
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq))]
pub struct Bundle {
    /// Version of the `qxide` that built it (always this binary's own version).
    pub version: String,
    /// Display name of the entry file (as given to `qxide build`).
    pub name: String,
    pub src: String,
    pub engine: String,
    pub shots: u64,
    pub seed: Option<u64>,
    pub allow_fs: bool,
    /// `--allow ffi`: the program may call native functions (`extern`).
    pub allow_ffi: bool,
    pub project: Option<Project>,
    /// Every module file the linker read while checking, by the path it asked for.
    pub files: Vec<(String, String)>,
}

#[cfg(test)]
impl PartialEq for Project {
    fn eq(&self, o: &Self) -> bool {
        self.entry_package == o.entry_package
            && self.entry_module == o.entry_module
            && self.packages.len() == o.packages.len()
            && self
                .packages
                .iter()
                .zip(&o.packages)
                .all(|(a, b)| a.name == b.name && a.src_dir == b.src_dir && a.deps == b.deps)
    }
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn len(&mut self, n: usize) -> Result<(), String> {
        let n = u32::try_from(n).map_err(|_| "program too large to bundle".to_string())?;
        self.u32(n);
        Ok(())
    }
    fn str(&mut self, s: &str) -> Result<(), String> {
        self.len(s.len())?;
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
}

const DAMAGED: &str = "payload is damaged";

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(n).ok_or(DAMAGED)?;
        let s = self.buf.get(self.at..end).ok_or(DAMAGED)?;
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Result<u64, String> {
        let mut a = [0u8; 8];
        a.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(a))
    }
    // A count of items that each take at least `min` bytes: never more than the bytes left.
    fn count(&mut self, min: usize) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n.saturating_mul(min) > self.buf.len() - self.at {
            return Err(DAMAGED.into());
        }
        Ok(n)
    }
    fn str(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        let b = self.take(n)?;
        String::from_utf8(b.to_vec()).map_err(|_| DAMAGED.into())
    }
    fn flag(&mut self) -> Result<bool, String> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DAMAGED.into()),
        }
    }
}

fn path_str(p: &Path) -> Result<&str, String> {
    p.to_str()
        .ok_or_else(|| format!("path `{}` is not valid UTF-8", p.display()))
}

impl Bundle {
    /// The payload followed by its trailer, ready to append to the executable.
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let mut w = Writer::default();
        w.str(&self.version)?;
        w.str(&self.name)?;
        w.str(&self.src)?;
        w.str(&self.engine)?;
        w.u64(self.shots);
        match self.seed {
            Some(s) => {
                w.u8(1);
                w.u64(s);
            }
            None => w.u8(0),
        }
        // Permission bits: 1 = `--allow fs`, 2 = `--allow ffi`.
        w.u8(u8::from(self.allow_fs) | (u8::from(self.allow_ffi) << 1));
        match &self.project {
            None => w.u8(0),
            Some(p) => {
                w.u8(1);
                w.len(p.packages.len())?;
                for pkg in &p.packages {
                    w.str(&pkg.name)?;
                    w.str(path_str(&pkg.src_dir)?)?;
                    w.len(pkg.deps.len())?;
                    for &d in &pkg.deps {
                        w.len(d)?;
                    }
                }
                w.len(p.entry_package)?;
                match &p.entry_module {
                    None => w.u8(0),
                    Some(parts) => {
                        w.u8(1);
                        w.len(parts.len())?;
                        for s in parts {
                            w.str(s)?;
                        }
                    }
                }
            }
        }
        w.len(self.files.len())?;
        for (path, text) in &self.files {
            w.str(path)?;
            w.str(text)?;
        }
        let mut out = w.0;
        let len = out.len() as u64;
        if len > MAX_PAYLOAD {
            return Err(format!(
                "program too large to bundle ({len} bytes; the limit is {MAX_PAYLOAD})"
            ));
        }
        let sum = fnv1a(&out);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&sum.to_le_bytes());
        out.extend_from_slice(&FORMAT.to_le_bytes());
        out.extend_from_slice(MAGIC);
        Ok(out)
    }

    /// Decodes a payload whose checksum the trailer gave.
    pub fn decode(payload: &[u8], sum: u64) -> Result<Bundle, String> {
        if fnv1a(payload) != sum {
            return Err("checksum mismatch".into());
        }
        let mut r = Reader {
            buf: payload,
            at: 0,
        };
        let version = r.str()?;
        let name = r.str()?;
        let src = r.str()?;
        let engine = r.str()?;
        let shots = r.u64()?;
        let seed = if r.flag()? { Some(r.u64()?) } else { None };
        let perms = r.u8()?;
        if perms > 3 {
            return Err(DAMAGED.into());
        }
        let (allow_fs, allow_ffi) = (perms & 1 != 0, perms & 2 != 0);
        let project = if r.flag()? {
            let n = r.count(12)?;
            let mut packages = Vec::with_capacity(n);
            for _ in 0..n {
                let name = r.str()?;
                let src_dir = PathBuf::from(r.str()?);
                let k = r.count(4)?;
                let mut deps = Vec::with_capacity(k);
                for _ in 0..k {
                    let d = r.u32()? as usize;
                    if d >= n {
                        return Err(DAMAGED.into());
                    }
                    deps.push(d);
                }
                packages.push(PackageRoot {
                    name,
                    src_dir,
                    deps,
                });
            }
            let entry_package = r.u32()? as usize;
            if entry_package >= n {
                return Err(DAMAGED.into());
            }
            let entry_module = if r.flag()? {
                let k = r.count(4)?;
                let mut parts = Vec::with_capacity(k);
                for _ in 0..k {
                    parts.push(r.str()?);
                }
                Some(parts)
            } else {
                None
            };
            Some(Project {
                packages,
                entry_package,
                entry_module,
            })
        } else {
            None
        };
        let n = r.count(8)?;
        let mut files = Vec::with_capacity(n);
        for _ in 0..n {
            files.push((r.str()?, r.str()?));
        }
        if r.at != payload.len() {
            return Err(DAMAGED.into());
        }
        Ok(Bundle {
            version,
            name,
            src,
            engine,
            shots,
            seed,
            allow_fs,
            allow_ffi,
            project,
            files,
        })
    }
}

/// What the last `TRAILER_LEN` bytes of a file of `file_len` bytes say: `None` without the magic
/// (a plain `qxide`), else the payload's length and checksum after bounds checks.
pub fn parse_trailer(t: &[u8; TRAILER_LEN], file_len: u64) -> Result<Option<(u64, u64)>, String> {
    if &t[20..32] != MAGIC {
        return Ok(None);
    }
    let le = |b: &[u8]| {
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        u64::from_le_bytes(a)
    };
    let (len, sum) = (le(&t[0..8]), le(&t[8..16]));
    let format = u32::from_le_bytes([t[16], t[17], t[18], t[19]]);
    if format != FORMAT {
        return Err(format!("unsupported bundle format {format}"));
    }
    if len > MAX_PAYLOAD || len > file_len - TRAILER_LEN as u64 {
        return Err("truncated or damaged file".into());
    }
    Ok(Some((len, sum)))
}

/// The bundle appended to the file at `path`, if any.
pub fn read_from(path: &Path) -> Result<Option<Bundle>, String> {
    let Ok(mut f) = std::fs::File::open(path) else {
        return Ok(None);
    };
    let file_len = f.metadata().map_err(|e| e.to_string())?.len();
    if file_len < TRAILER_LEN as u64 {
        return Ok(None);
    }
    let mut t = [0u8; TRAILER_LEN];
    f.seek(SeekFrom::End(-(TRAILER_LEN as i64)))
        .and_then(|_| f.read_exact(&mut t))
        .map_err(|e| e.to_string())?;
    let Some((len, sum)) = parse_trailer(&t, file_len)? else {
        return Ok(None);
    };
    let mut payload = vec![0u8; len as usize];
    f.seek(SeekFrom::Start(file_len - TRAILER_LEN as u64 - len))
        .and_then(|_| f.read_exact(&mut payload))
        .map_err(|e| e.to_string())?;
    Bundle::decode(&payload, sum).map(Some)
}

/// The bundle of the running executable; `Ok(None)` for a plain `qxide`.
pub fn current() -> Result<Option<Bundle>, String> {
    match std::env::current_exe() {
        Ok(exe) => read_from(&exe),
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Bundle {
        Bundle {
            version: "0.8.0".into(),
            name: "app.qx".into(),
            src: "void main() {}\n".into(),
            engine: "ir".into(),
            shots: 3,
            seed: Some(7),
            allow_fs: true,
            allow_ffi: true,
            project: Some(Project {
                packages: vec![
                    PackageRoot {
                        name: "app".into(),
                        src_dir: PathBuf::from("app/src"),
                        deps: vec![1],
                    },
                    PackageRoot {
                        name: "util".into(),
                        src_dir: PathBuf::from("util/src"),
                        deps: vec![],
                    },
                ],
                entry_package: 0,
                entry_module: Some(vec!["a".into(), "b".into()]),
            }),
            files: vec![(
                "util/src/lib.qx".into(),
                "public int one() { return 1; }".into(),
            )],
        }
    }

    fn split(bytes: &[u8]) -> Result<Option<Bundle>, String> {
        let n = bytes.len();
        if n < TRAILER_LEN {
            return Ok(None);
        }
        let t: [u8; TRAILER_LEN] = bytes[n - TRAILER_LEN..].try_into().unwrap();
        let Some((len, sum)) = parse_trailer(&t, n as u64)? else {
            return Ok(None);
        };
        let start = n - TRAILER_LEN - len as usize;
        Bundle::decode(&bytes[start..n - TRAILER_LEN], sum).map(Some)
    }

    #[test]
    fn round_trips_after_any_prefix() {
        let b = sample();
        let mut file = b"MZ some executable bytes".to_vec();
        file.extend(b.encode().unwrap());
        assert_eq!(split(&file), Ok(Some(b.clone())));
        let mut plain = sample();
        plain.project = None;
        plain.seed = None;
        let enc = plain.encode().unwrap();
        assert_eq!(split(&enc), Ok(Some(plain)));
    }

    #[test]
    fn files_without_the_magic_are_plain() {
        assert_eq!(split(b"short"), Ok(None));
        assert_eq!(split(&[0u8; 100]), Ok(None));
    }

    #[test]
    fn every_corruption_is_an_error_not_a_panic() {
        let mut file = b"prefix".to_vec();
        file.extend(sample().encode().unwrap());
        let n = file.len();
        // Flipping any byte of the payload or trailer (except inside the magic) is detected.
        for i in 6..n - MAGIC.len() {
            let mut bad = file.clone();
            bad[i] ^= 0x41;
            assert!(split(&bad).is_err(), "byte {i} flipped went unnoticed");
        }
        // Truncations that keep the trailer: the payload no longer fits.
        let tail = file[n - TRAILER_LEN..].to_vec();
        for cut in 0..n - TRAILER_LEN {
            let mut bad = file[cut + 1..n - TRAILER_LEN].to_vec();
            bad.extend(&tail);
            let r = split(&bad);
            assert!(r.is_err() || cut < 6, "cut {cut}: {r:?}");
        }
        // A length beyond the limit.
        let mut huge = file.clone();
        huge[n - TRAILER_LEN..n - TRAILER_LEN + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(split(&huge).is_err());
    }

    #[test]
    fn damaged_payloads_with_a_matching_checksum_are_rejected() {
        // Counts larger than the payload, an entry package out of range, trailing bytes.
        for payload in [vec![0xff, 0xff, 0xff, 0xff], {
            let mut p = sample().encode().unwrap();
            p.truncate(p.len() - TRAILER_LEN);
            p.push(0);
            p
        }] {
            assert!(Bundle::decode(&payload, fnv1a(&payload)).is_err());
        }
    }
}
