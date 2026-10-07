//! Reading the compiler version out of rustc metadata files.
//!
//! rustc refuses to load metadata produced by a different compiler (it compares the version
//! string stored in the header). `obol-driver` *is* a rustc, so it can only read `.rmeta` files
//! made by the exact same compiler build. Checking this up front gives a much clearer error than
//! rustc's `E0514: found crate compiled by an incompatible version of rustc`.

use std::io::Read;
use std::path::Path;

/// Extract the `rustc X.Y.Z (hash date)` version string from the header of an `.rmeta`/`.rlib`
/// metadata blob. The header is `b"rust"`, a 4-byte format version, an 8-byte offset, then the
/// version string prefixed by its length (one byte).
pub fn version_from_metadata(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 17 || &bytes[..4] != b"rust" {
        return None;
    }
    // Look for the length-prefixed `rustc ` string in the first bytes of the header rather than
    // hard-coding the offset, which has changed across metadata versions.
    let window = &bytes[..bytes.len().min(256)];
    let pos = window.windows(6).position(|w| w == b"rustc ")?;
    if pos == 0 {
        return None;
    }
    let len = window[pos - 1] as usize;
    let s = bytes.get(pos..pos + len)?;
    let s = std::str::from_utf8(s).ok()?;
    s.ends_with(')').then(|| s.to_owned())
}

/// Read the compiler version from an `.rmeta` file, or from the metadata member of an `.rlib`
/// archive. Returns `None` for other files (e.g. proc-macro `.so` files).
pub fn version_from_rmeta_file(path: &Path) -> std::io::Result<Option<String>> {
    let mut f = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    // rustc puts the metadata member first in rlibs, so it is near the start of the file.
    f.by_ref().take(1 << 20).read_to_end(&mut buf)?;
    Ok(version_from_bytes(&buf))
}

/// Like [`version_from_metadata`], but also accepts an `ar` archive (`.rlib`) containing the
/// metadata.
pub fn version_from_bytes(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(b"!<arch>\n") {
        let pos = bytes.windows(7).position(|w| w == b"rust\0\0\0")?;
        version_from_metadata(&bytes[pos..])
    } else {
        version_from_metadata(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_header() {
        let mut header = b"rust\0\0\0\x0a\x9e\x04\x87\0\0\0\0\0".to_vec();
        let v = "rustc 1.98.1 (48a229cea 2026-09-01)";
        header.push(v.len() as u8);
        header.extend_from_slice(v.as_bytes());
        header.extend_from_slice(b"\xc1\x02\x96 trailing");
        assert_eq!(version_from_metadata(&header).as_deref(), Some(v));
        assert_eq!(version_from_metadata(b"!<arch>\nfoo"), None);
        let mut rlib =
            b"!<arch>\nlib.rmeta/      0           0     0     644     1234      `\n".to_vec();
        rlib.extend_from_slice(&header);
        assert_eq!(version_from_bytes(&rlib).as_deref(), Some(v));
    }
}
