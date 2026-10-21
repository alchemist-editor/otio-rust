//! zlib and base64, as ORI's v1 proposal compresses large snapshots.

use std::io::{Read, Write};

use crate::Error;
use crate::compression::MAX_EXPANDED;

/// zlib-compresses `data` (RFC 1950).
pub(super) fn deflate(data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(data)
        .and_then(|()| encoder.finish())
        .map_err(|error| Error::Compression(error.to_string()))
}

/// Expands zlib data, refusing more than [`MAX_EXPANDED`] bytes.
pub(super) fn inflate(data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data)
        .take(MAX_EXPANDED as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|error| Error::Compression(error.to_string()))?;
    if out.len() > MAX_EXPANDED {
        return Err(Error::Compression(format!(
            "expands past {MAX_EXPANDED} bytes"
        )));
    }
    Ok(out)
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding (RFC 4648), as Python's `base64.b64encode`.
pub(super) fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let bytes = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let bits = u32::from(bytes[0]) << 16 | u32::from(bytes[1]) << 8 | u32::from(bytes[2]);
        for (place, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if place <= chunk.len() {
                out.push(char::from(ALPHABET[(bits >> shift & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Reads standard base64, with or without padding; whitespace is skipped.
pub(super) fn base64_decode(text: &str) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut bits = 0_u32;
    let mut count = 0;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            other => {
                return Err(Error::Protocol(format!(
                    "the snapshot is not base64: byte {other:#04x}"
                )));
            }
        };
        bits = bits << 6 | u32::from(value);
        count += 1;
        if count == 4 {
            out.extend_from_slice(&[(bits >> 16) as u8, (bits >> 8) as u8, bits as u8]);
            bits = 0;
            count = 0;
        }
    }
    match count {
        0 => {}
        2 => out.push((bits >> 4) as u8),
        3 => out.extend_from_slice(&[(bits >> 10) as u8, (bits >> 2) as u8]),
        _ => return Err(Error::Protocol("the snapshot's base64 is cut short".into())),
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_4648() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(plain.as_bytes()), encoded);
            assert_eq!(base64_decode(encoded).unwrap(), plain.as_bytes());
        }
    }

    #[test]
    fn zlib_round_trips() {
        let text = "timeline ".repeat(1000);
        let packed = deflate(text.as_bytes()).unwrap();
        assert!(packed.len() < text.len());
        assert_eq!(packed[0], 0x78, "a zlib header");
        assert_eq!(inflate(&packed).unwrap(), text.as_bytes());
    }
}
