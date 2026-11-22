//! Decodes the result of a Qiskit Runtime `sampler` job: the JSON that `qiskit_ibm_runtime`'s
//! `RuntimeEncoder` writes and its `RuntimeDecoder` reads (`PrimitiveResult` → `SamplerPubResult`
//! → `DataBin` → `BitArray`). A `BitArray` holds a NumPy `uint8` array, saved with `np.save`,
//! compressed with zlib and base64-encoded; each row is one shot with its bits big-endian
//! (bit 0 is the lowest bit of the last byte).

use serde_json::Value;
use std::io::Read as _;

/// One record per shot: `c[0..bits]` of that shot.
pub fn decode(body: &[u8], register: &str, bits: usize) -> Result<Vec<Vec<bool>>, String> {
    let root: Value = serde_json::from_slice(body).map_err(|e| format!("not JSON: {e}"))?;
    let result = typed(&root, "PrimitiveResult")?;
    let pubs = result
        .get("pub_results")
        .and_then(Value::as_array)
        .ok_or("no `pub_results`")?;
    let [first] = pubs.as_slice() else {
        return Err(format!("expected one pub result, found {}", pubs.len()));
    };
    let pub_result = typed(first, "SamplerPubResult")?;
    let data = typed(pub_result.get("data").ok_or("no `data`")?, "DataBin")?;
    let field = data
        .get("fields")
        .and_then(|f| f.get(register))
        .ok_or_else(|| format!("no register `{register}` in the result"))?;
    let bit_array = typed(field, "BitArray")?;
    let num_bits = bit_array
        .get("num_bits")
        .and_then(Value::as_u64)
        .ok_or("no `num_bits`")?;
    if num_bits != bits as u64 {
        return Err(format!("expected {bits} bits, the result has {num_bits}"));
    }
    let array = typed(bit_array.get("array").ok_or("no `array`")?, "ndarray")?;
    let encoded = array.as_str().ok_or("`array` is not encoded data")?;
    let compressed = base64(encoded)?;
    let bytes_per_shot = bits.div_ceil(8);
    let npy = inflate(&compressed, MAX_RESULT)?;
    let (shape, data) = npy_u8(&npy)?;
    let (shots, width) = match shape.as_slice() {
        [shots, width] => (*shots, *width),
        _ => return Err(format!("unexpected array shape {shape:?}")),
    };
    if width != bytes_per_shot || shots == 0 {
        return Err(format!("unexpected array shape {shape:?} for {bits} bits"));
    }
    Ok(data
        .chunks_exact(width)
        .map(|row| {
            (0..bits)
                .map(|i| row[width - 1 - i / 8] >> (i % 8) & 1 == 1)
                .collect()
        })
        .collect())
}

// Decompressed results are at most a few hundred MB (millions of shots of wide registers).
const MAX_RESULT: u64 = 512 << 20;

fn typed<'a>(v: &'a Value, ty: &str) -> Result<&'a Value, String> {
    match (
        v.get("__type__").and_then(Value::as_str),
        v.get("__value__"),
    ) {
        (Some(t), Some(value)) if t == ty => Ok(value),
        (t, _) => Err(format!("expected `{ty}`, found `{}`", t.unwrap_or("?"))),
    }
}

fn base64(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut n) = (0u32, 0);
    for &c in s.as_bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return Err("bad base64".into()),
        };
        acc = acc << 6 | u32::from(v);
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((acc >> n) as u8);
        }
    }
    Ok(out)
}

fn inflate(data: &[u8], limit: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data)
        .take(limit + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("bad zlib data: {e}"))?;
    if out.len() as u64 > limit {
        return Err("result too large".into());
    }
    Ok(out)
}

// A `.npy` file holding a C-ordered `uint8` array: (shape, data).
fn npy_u8(b: &[u8]) -> Result<(Vec<usize>, &[u8]), String> {
    let bad = || "not a NumPy uint8 array".to_string();
    if b.len() < 10 || &b[..6] != b"\x93NUMPY" {
        return Err(bad());
    }
    let (len, start) = match b[6] {
        1 => (usize::from(u16::from_le_bytes([b[8], b[9]])), 10),
        2 | 3 if b.len() >= 12 => (u32::from_le_bytes([b[8], b[9], b[10], b[11]]) as usize, 12),
        _ => return Err(bad()),
    };
    let header = b
        .get(start..start + len)
        .and_then(|h| std::str::from_utf8(h).ok())
        .ok_or_else(bad)?;
    let compact: String = header.chars().filter(|c| !c.is_whitespace()).collect();
    if !compact.contains("'descr':'|u1'") || !compact.contains("'fortran_order':False") {
        return Err(bad());
    }
    let shape = compact
        .split("'shape':(")
        .nth(1)
        .and_then(|s| s.split(')').next())
        .ok_or_else(bad)?;
    let shape: Vec<usize> = shape
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().map_err(|_| bad()))
        .collect::<Result<_, _>>()?;
    let data = &b[start + len..];
    let count = shape
        .iter()
        .try_fold(1usize, |acc, &d| acc.checked_mul(d))
        .ok_or_else(bad)?;
    if data.len() != count {
        return Err(bad());
    }
    Ok((shape, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Written by `qiskit_ibm_runtime.json.RuntimeEncoder` (0.52.1) for a `PrimitiveResult` with
    // one `SamplerPubResult` whose register `c` has 10 bits and the shots
    // 0x001, 0x001, 0x200, 0x3ff, 0x001 (regenerate with ci/qpu_fixtures.py).
    const TEN_BITS: &str = include_str!("../../tests/data/qpu/results_10bit.json");

    #[test]
    fn decodes_the_official_encoding() {
        let records = decode(TEN_BITS.as_bytes(), "c", 10).unwrap();
        let ints: Vec<u32> = records
            .iter()
            .map(|r| r.iter().enumerate().map(|(i, &b)| u32::from(b) << i).sum())
            .collect();
        assert_eq!(ints, [0x001, 0x001, 0x200, 0x3ff, 0x001]);
        assert!(records[0][0] && !records[0][9]);
    }

    #[test]
    fn rejects_mismatches_and_damage() {
        assert!(decode(TEN_BITS.as_bytes(), "c", 9).is_err());
        assert!(decode(TEN_BITS.as_bytes(), "meas", 10).is_err());
        assert!(decode(b"{}", "c", 10).is_err());
        assert!(decode(b"not json", "c", 10).is_err());
        let damaged = TEN_BITS.replace("eJ", "eK");
        assert!(decode(damaged.as_bytes(), "c", 10).is_err());
    }

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64("aGk=").unwrap(), b"hi");
        assert!(base64("a$b").is_err());
    }

    #[test]
    fn inflate_stops_at_the_limit() {
        let mut big = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut big, &vec![0u8; 1 << 20]).unwrap();
        let big = big.finish().unwrap();
        assert!(inflate(&big, 1000).is_err());
        assert_eq!(inflate(&big, 1 << 20).unwrap().len(), 1 << 20);
    }
}
