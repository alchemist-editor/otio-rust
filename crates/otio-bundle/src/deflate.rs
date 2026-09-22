//! Raw DEFLATE (RFC 1951), the compression a zip archive uses.
//!
//! Upstream writes `.otioz` archives with minizip-ng, which deflates the two
//! text entries and stores the media uncompressed. Reading has to accept any
//! stream another tool produced, so [`inflate`] handles all three block types.
//! Writing only has to produce a valid stream, so [`deflate`] keeps to one
//! block of fixed Huffman codes over a greedy LZ77 match finder: JSON is
//! repetitive enough that this does most of what a full encoder would.
//!
//! The decoder follows the structure of Mark Adler's `puff.c`, the reference
//! decoder that ships with zlib, which trades speed for being obviously
//! correct.

use std::fmt;

/// Why a DEFLATE stream could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InflateError(&'static str);

impl fmt::Display for InflateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid deflate stream: {}", self.0)
    }
}

impl std::error::Error for InflateError {}

const MAX_BITS: usize = 15;

/// The base length for each length symbol from 257, and its extra bits.
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

/// The base distance for each distance symbol, and its extra bits.
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// The order code length code lengths arrive in, in a dynamic block header.
const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// A canonical Huffman code, as counts per length and symbols in code order.
struct Huffman {
    count: [u16; MAX_BITS + 1],
    symbol: Vec<u16>,
}

impl Huffman {
    /// Builds the code from each symbol's code length (zero for unused).
    fn new(lengths: &[u8]) -> Result<Self, InflateError> {
        let mut count = [0u16; MAX_BITS + 1];
        for &length in lengths {
            count[usize::from(length)] += 1;
        }
        // An over-subscribed set of lengths describes no code at all. An
        // incomplete one is allowed: a stream that uses only one distance
        // code has to be able to say so.
        let mut left: i32 = 1;
        for &n in &count[1..] {
            left <<= 1;
            left -= i32::from(n);
            if left < 0 {
                return Err(InflateError("over-subscribed Huffman code"));
            }
        }
        let mut offsets = [0u16; MAX_BITS + 1];
        for length in 1..MAX_BITS {
            offsets[length + 1] = offsets[length] + count[length];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (value, &length) in lengths.iter().enumerate() {
            if length != 0 {
                let slot = &mut offsets[usize::from(length)];
                symbol[usize::from(*slot)] = value as u16;
                *slot += 1;
            }
        }
        Ok(Self { count, symbol })
    }
}

/// Reads a stream least significant bit first, as DEFLATE packs it.
struct BitReader<'a> {
    input: &'a [u8],
    position: usize,
    buffer: u64,
    held: u32,
}

impl<'a> BitReader<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            position: 0,
            buffer: 0,
            held: 0,
        }
    }

    fn bits(&mut self, need: u32) -> Result<u32, InflateError> {
        while self.held < need {
            let byte = *self
                .input
                .get(self.position)
                .ok_or(InflateError("unexpected end of stream"))?;
            self.position += 1;
            self.buffer |= u64::from(byte) << self.held;
            self.held += 8;
        }
        let value = (self.buffer & ((1u64 << need) - 1)) as u32;
        self.buffer >>= need;
        self.held -= need;
        Ok(value)
    }

    /// Drops the bits left in the current byte.
    fn align(&mut self) {
        self.buffer = 0;
        self.held = 0;
    }

    fn decode(&mut self, code: &Huffman) -> Result<u16, InflateError> {
        let mut bits: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for length in 1..=MAX_BITS {
            bits |= self.bits(1)? as i32;
            let count = i32::from(code.count[length]);
            if bits - count < first {
                return Ok(code.symbol[(index + (bits - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            bits <<= 1;
        }
        Err(InflateError("invalid Huffman code"))
    }
}

/// Decodes a raw DEFLATE stream.
///
/// # Errors
///
/// Returns an [`InflateError`] if the stream is truncated or malformed.
pub fn inflate(input: &[u8]) -> Result<Vec<u8>, InflateError> {
    let mut reader = BitReader::new(input);
    let mut out = Vec::new();
    loop {
        let last = reader.bits(1)? == 1;
        match reader.bits(2)? {
            0 => stored(&mut reader, &mut out)?,
            1 => {
                let (lengths, distances) = fixed_codes()?;
                codes(&mut reader, &mut out, &lengths, &distances)?;
            }
            2 => {
                let (lengths, distances) = dynamic_codes(&mut reader)?;
                codes(&mut reader, &mut out, &lengths, &distances)?;
            }
            _ => return Err(InflateError("invalid block type")),
        }
        if last {
            return Ok(out);
        }
    }
}

fn stored(reader: &mut BitReader<'_>, out: &mut Vec<u8>) -> Result<(), InflateError> {
    reader.align();
    let header = reader
        .input
        .get(reader.position..reader.position + 4)
        .ok_or(InflateError("unexpected end of stream"))?;
    let length = u16::from_le_bytes([header[0], header[1]]);
    let complement = u16::from_le_bytes([header[2], header[3]]);
    if length != !complement {
        return Err(InflateError(
            "stored block length does not match its complement",
        ));
    }
    reader.position += 4;
    let data = reader
        .input
        .get(reader.position..reader.position + usize::from(length))
        .ok_or(InflateError("unexpected end of stream"))?;
    out.extend_from_slice(data);
    reader.position += usize::from(length);
    Ok(())
}

fn fixed_codes() -> Result<(Huffman, Huffman), InflateError> {
    let mut lengths = [0u8; 288];
    for (symbol, length) in lengths.iter_mut().enumerate() {
        *length = match symbol {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    Ok((Huffman::new(&lengths)?, Huffman::new(&[5u8; 30])?))
}

fn dynamic_codes(reader: &mut BitReader<'_>) -> Result<(Huffman, Huffman), InflateError> {
    let literal_count = reader.bits(5)? as usize + 257;
    let distance_count = reader.bits(5)? as usize + 1;
    let code_length_count = reader.bits(4)? as usize + 4;
    if literal_count > 286 || distance_count > 30 {
        return Err(InflateError("too many length or distance codes"));
    }

    let mut code_lengths = [0u8; 19];
    for &slot in &CODE_LENGTH_ORDER[..code_length_count] {
        code_lengths[slot] = reader.bits(3)? as u8;
    }
    let code_length_code = Huffman::new(&code_lengths)?;

    let mut lengths = vec![0u8; literal_count + distance_count];
    let mut index = 0;
    while index < lengths.len() {
        let symbol = reader.decode(&code_length_code)?;
        if symbol < 16 {
            lengths[index] = symbol as u8;
            index += 1;
            continue;
        }
        let (value, repeat) = match symbol {
            16 => {
                let previous = *index
                    .checked_sub(1)
                    .and_then(|i| lengths.get(i))
                    .ok_or(InflateError("repeat with no previous length"))?;
                (previous, 3 + reader.bits(2)? as usize)
            }
            17 => (0, 3 + reader.bits(3)? as usize),
            _ => (0, 11 + reader.bits(7)? as usize),
        };
        if index + repeat > lengths.len() {
            return Err(InflateError("too many code lengths"));
        }
        lengths[index..index + repeat].fill(value);
        index += repeat;
    }
    if lengths[256] == 0 {
        return Err(InflateError("no end-of-block code"));
    }
    Ok((
        Huffman::new(&lengths[..literal_count])?,
        Huffman::new(&lengths[literal_count..])?,
    ))
}

fn codes(
    reader: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    lengths: &Huffman,
    distances: &Huffman,
) -> Result<(), InflateError> {
    loop {
        let symbol = reader.decode(lengths)?;
        match symbol {
            0..=255 => out.push(symbol as u8),
            256 => return Ok(()),
            _ => {
                let slot = usize::from(symbol - 257);
                if slot >= LENGTH_BASE.len() {
                    return Err(InflateError("invalid length symbol"));
                }
                let length = usize::from(LENGTH_BASE[slot])
                    + reader.bits(u32::from(LENGTH_EXTRA[slot]))? as usize;
                let slot = usize::from(reader.decode(distances)?);
                if slot >= DIST_BASE.len() {
                    return Err(InflateError("invalid distance symbol"));
                }
                let distance = usize::from(DIST_BASE[slot])
                    + reader.bits(u32::from(DIST_EXTRA[slot]))? as usize;
                if distance > out.len() {
                    return Err(InflateError("distance too far back"));
                }
                // Byte by byte: a match may overlap the bytes it produces.
                let start = out.len() - distance;
                for i in 0..length {
                    let byte = out[start + i];
                    out.push(byte);
                }
            }
        }
    }
}

/// Writes bits least significant first, as DEFLATE packs them.
struct BitWriter {
    out: Vec<u8>,
    buffer: u64,
    held: u32,
}

impl BitWriter {
    fn put(&mut self, value: u32, count: u32) {
        self.buffer |= u64::from(value) << self.held;
        self.held += count;
        while self.held >= 8 {
            self.out.push(self.buffer as u8);
            self.buffer >>= 8;
            self.held -= 8;
        }
    }

    /// Writes a Huffman code, which DEFLATE stores most significant bit first.
    fn put_code(&mut self, code: u32, length: u32) {
        let reversed = code.reverse_bits() >> (32 - length);
        self.put(reversed, length);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.held > 0 {
            self.out.push(self.buffer as u8);
        }
        self.out
    }
}

/// The fixed Huffman code for a literal or length symbol, and its length.
fn fixed_literal(symbol: u16) -> (u32, u32) {
    let symbol = u32::from(symbol);
    match symbol {
        0..=143 => (0x30 + symbol, 8),
        144..=255 => (0x190 + symbol - 144, 9),
        256..=279 => (symbol - 256, 7),
        _ => (0xC0 + symbol - 280, 8),
    }
}

fn put_literal(writer: &mut BitWriter, symbol: u16) {
    let (code, length) = fixed_literal(symbol);
    writer.put_code(code, length);
}

fn put_match(writer: &mut BitWriter, length: usize, distance: usize) {
    let slot = LENGTH_BASE
        .iter()
        .rposition(|&base| usize::from(base) <= length)
        .expect("a match is at least three bytes long");
    put_literal(writer, 257 + slot as u16);
    writer.put(
        (length - usize::from(LENGTH_BASE[slot])) as u32,
        u32::from(LENGTH_EXTRA[slot]),
    );
    let slot = DIST_BASE
        .iter()
        .rposition(|&base| usize::from(base) <= distance)
        .expect("a distance is at least one");
    writer.put_code(slot as u32, 5);
    writer.put(
        (distance - usize::from(DIST_BASE[slot])) as u32,
        u32::from(DIST_EXTRA[slot]),
    );
}

const WINDOW: usize = 32 * 1024;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const HASH_BITS: u32 = 15;
const MAX_CHAIN: usize = 64;

fn hash(bytes: &[u8]) -> usize {
    let value = u32::from(bytes[0]) << 16 | u32::from(bytes[1]) << 8 | u32::from(bytes[2]);
    (value.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

/// Compresses `input` as a raw DEFLATE stream.
///
/// The stream is one final block of fixed Huffman codes, which any inflater
/// reads; see the module documentation for why that is enough here.
pub fn deflate(input: &[u8]) -> Vec<u8> {
    let mut writer = BitWriter {
        out: Vec::with_capacity(input.len() / 2 + 16),
        buffer: 0,
        held: 0,
    };
    // BFINAL = 1, BTYPE = 01 (fixed codes).
    writer.put(1, 1);
    writer.put(1, 2);

    // `head` holds the latest position with each hash, plus one (zero means
    // none), and `previous` chains each position to the one before it with
    // the same hash.
    let mut head = vec![0usize; 1 << HASH_BITS];
    let mut previous = vec![0usize; input.len()];
    let insert = |position: usize, head: &mut Vec<usize>, previous: &mut Vec<usize>| {
        if position + MIN_MATCH <= input.len() {
            let h = hash(&input[position..]);
            previous[position] = head[h];
            head[h] = position + 1;
        }
    };

    let mut position = 0;
    while position < input.len() {
        let mut best_length = 0;
        let mut best_distance = 0;
        if position + MIN_MATCH <= input.len() {
            let limit = (input.len() - position).min(MAX_MATCH);
            let mut candidate = head[hash(&input[position..])];
            let mut chain = 0;
            while candidate != 0 && chain < MAX_CHAIN {
                let start = candidate - 1;
                let distance = position - start;
                if distance > WINDOW {
                    break;
                }
                let length = input[start..]
                    .iter()
                    .zip(&input[position..position + limit])
                    .take_while(|(a, b)| a == b)
                    .count();
                if length > best_length {
                    best_length = length;
                    best_distance = distance;
                    if length == limit {
                        break;
                    }
                }
                candidate = previous[start];
                chain += 1;
            }
        }

        if best_length >= MIN_MATCH {
            put_match(&mut writer, best_length, best_distance);
            for offset in 0..best_length {
                insert(position + offset, &mut head, &mut previous);
            }
            position += best_length;
        } else {
            put_literal(&mut writer, u16::from(input[position]));
            insert(position, &mut head, &mut previous);
            position += 1;
        }
    }
    put_literal(&mut writer, 256);
    writer.finish()
}

#[cfg(test)]
mod tests {
    use super::{deflate, inflate};

    fn round_trip(input: &[u8]) {
        let packed = deflate(input);
        assert_eq!(inflate(&packed).expect("our own stream inflates"), input);
    }

    #[test]
    fn empty_input_round_trips() {
        round_trip(b"");
    }

    #[test]
    fn text_round_trips_and_shrinks() {
        let text = r#"{"OTIO_SCHEMA": "Clip.2", "name": "a", "metadata": {}}"#.repeat(200);
        round_trip(text.as_bytes());
        assert!(deflate(text.as_bytes()).len() < text.len() / 10);
    }

    #[test]
    fn every_byte_value_and_long_runs_round_trip() {
        let mut input: Vec<u8> = (0..=255u8).collect();
        input.extend(std::iter::repeat_n(7u8, 1000));
        input.extend((0..70_000u32).map(|n| (n.wrapping_mul(2_654_435_761) >> 24) as u8));
        round_trip(&input);
    }

    #[test]
    fn a_stored_block_inflates() {
        // BFINAL=1, BTYPE=00, LEN=5, NLEN=!5, "hello"
        let stream = [0x01, 0x05, 0x00, 0xFA, 0xFF, b'h', b'e', b'l', b'l', b'o'];
        assert_eq!(inflate(&stream).unwrap(), b"hello");
    }

    #[test]
    fn a_dynamic_block_from_zlib_inflates() {
        // zlib.compressobj(9, zlib.DEFLATED, -15) over the text below, which
        // is long and varied enough that zlib picks dynamic Huffman codes.
        let text = "The quick brown fox jumps over the lazy dog. ".repeat(4)
            + "Pack my box with five dozen liquor jugs! 0123456789";
        let stream: &[u8] = &[
            0xcd, 0xcb, 0xd9, 0x15, 0x40, 0x30, 0x14, 0x45, 0xd1, 0x56, 0xae, 0x06, 0x2c, 0xf3,
            0xd0, 0x85, 0x0f, 0x0d, 0x04, 0x41, 0x4c, 0x8f, 0x90, 0x20, 0xd5, 0x7b, 0x65, 0xf8,
            0x3e, 0xfb, 0xd4, 0xa3, 0xc4, 0x61, 0x54, 0x3b, 0xa3, 0xd1, 0x74, 0x6f, 0xe8, 0xe9,
            0xc1, 0x64, 0xd6, 0xfd, 0x04, 0x59, 0xa9, 0x71, 0x71, 0x5e, 0x84, 0x7b, 0xd1, 0xd1,
            0xe0, 0xa3, 0xfe, 0x07, 0xae, 0x04, 0xbb, 0xf5, 0x45, 0xc3, 0xe8, 0x56, 0xd7, 0x88,
            0x5e, 0x59, 0xc9, 0xc9, 0xc9, 0x0d, 0x8b, 0x3a, 0x0c, 0x69, 0x7e, 0x87, 0xd3, 0x43,
            0x10, 0x46, 0x71, 0x92, 0x66, 0x79, 0x51, 0x7e,
        ];
        assert_eq!(stream[0] >> 1 & 3, 2, "the vector is a dynamic block");
        assert_eq!(inflate(stream).unwrap(), text.as_bytes());
    }

    #[test]
    fn a_truncated_stream_is_refused() {
        let packed = deflate(b"some text that is long enough to matter, some text");
        assert!(inflate(&packed[..packed.len() / 2]).is_err());
    }
}
