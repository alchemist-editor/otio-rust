//! Decoding the UTF-16LE strings AAF and its container store.

/// Decodes a NUL-terminated UTF-16LE string.
///
/// The string ends at the first NUL, so a buffer that includes the terminator
/// and a buffer that does not both decode the same. A trailing odd byte is
/// ignored, and unpaired surrogates become the replacement character rather
/// than an error: a name a Windows application wrote is worth reading even if
/// it is not valid Unicode.
pub(crate) fn decode_le(data: &[u8]) -> String {
    let units = data
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0);
    char::decode_utf16(units)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}
