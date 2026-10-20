//! Compressing messages, as negotiated in the wire profile.
//!
//! Each side lists the encodings it accepts when it joins: the client in
//! its hello, the server in its welcome. A side compresses a message only
//! with an encoding the other side listed, only when the message is at least
//! [`THRESHOLD`] bytes, and only when compressing makes it smaller. Plain
//! JSON travels as a text frame, as it always has; a compressed message
//! travels as a binary frame whose first byte names the encoding
//! ([`Encoding::tag`]) and whose rest is the compressed JSON.
//!
//! A side that lists nothing, as every Furnace before this one, gets only
//! text frames, so negotiating is safe against old peers.
//!
//! ```text
//! text frame:    {"v":2,"type":"snapshot",…}
//! binary frame:  [0x02][brotli( {"v":2,"type":"snapshot",…} )]
//! ```

use std::io::{Read, Write};

use crate::{Error, Frame};

/// Messages shorter than this many bytes are never compressed: the saving
/// is too small to pay for the work.
pub const THRESHOLD: usize = 1024;

/// The most a compressed frame may expand to, so a small frame cannot
/// exhaust memory. Furnace's largest snapshot is 32 MiB.
pub const MAX_EXPANDED: usize = 64 << 20;

/// How a message is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Encoding {
    /// Not compressed.
    Identity,
    /// gzip (RFC 1952).
    Gzip,
    /// Brotli (RFC 7932).
    Brotli,
}

impl Encoding {
    /// Every encoding, best first: the order a side lists and picks them in.
    pub const ALL: [Self; 3] = [Self::Brotli, Self::Gzip, Self::Identity];

    /// The encoding's name in a hello or welcome: `identity`, `gzip` or
    /// `br`, as HTTP names them.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Gzip => "gzip",
            Self::Brotli => "br",
        }
    }

    /// The encoding called `name`, if it is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "identity" => Some(Self::Identity),
            "gzip" => Some(Self::Gzip),
            "br" => Some(Self::Brotli),
            _ => None,
        }
    }

    /// The first byte of a binary frame encoded this way.
    #[must_use]
    pub const fn tag(self) -> u8 {
        match self {
            Self::Identity => 0,
            Self::Gzip => 1,
            Self::Brotli => 2,
        }
    }

    /// The encoding whose tag is `tag`, if any.
    #[must_use]
    pub const fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::Identity),
            1 => Some(Self::Gzip),
            2 => Some(Self::Brotli),
            _ => None,
        }
    }

    /// Compresses `data`.
    ///
    /// # Errors
    ///
    /// [`Error::Compression`] if the encoder fails, which it does not on
    /// data held in memory.
    pub fn compress(self, data: &[u8]) -> Result<Vec<u8>, Error> {
        match self {
            Self::Identity => Ok(data.to_vec()),
            Self::Gzip => {
                let mut encoder =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(data).map_err(compression_error)?;
                encoder.finish().map_err(compression_error)
            }
            Self::Brotli => {
                // Quality 5 compresses timelines nearly as well as the
                // default 11 at a small fraction of the time, which matters
                // for a host sending every edit.
                let mut out = Vec::new();
                let mut encoder = brotli::CompressorWriter::new(&mut out, 4096, 5, 22);
                encoder.write_all(data).map_err(compression_error)?;
                drop(encoder);
                Ok(out)
            }
        }
    }

    /// Expands what [`Encoding::compress`] made.
    ///
    /// # Errors
    ///
    /// [`Error::Compression`] if `data` is not valid for the encoding or
    /// expands past [`MAX_EXPANDED`].
    pub fn expand(self, data: &[u8]) -> Result<Vec<u8>, Error> {
        let limit = MAX_EXPANDED as u64 + 1;
        let mut out = Vec::new();
        match self {
            Self::Identity => out.extend_from_slice(data),
            Self::Gzip => {
                flate2::read::GzDecoder::new(data)
                    .take(limit)
                    .read_to_end(&mut out)
                    .map_err(compression_error)?;
            }
            Self::Brotli => {
                brotli::Decompressor::new(data, 4096)
                    .take(limit)
                    .read_to_end(&mut out)
                    .map_err(compression_error)?;
            }
        }
        if out.len() > MAX_EXPANDED {
            return Err(Error::Compression(format!(
                "a frame expands past {MAX_EXPANDED} bytes"
            )));
        }
        Ok(out)
    }
}

fn compression_error(error: std::io::Error) -> Error {
    Error::Compression(error.to_string())
}

/// The names of `encodings`, for a hello or welcome.
#[must_use]
pub fn names(encodings: &[Encoding]) -> Vec<String> {
    encodings
        .iter()
        .map(|encoding| encoding.name().to_string())
        .collect()
}

/// The encodings `names` lists that this crate knows, in the order listed.
#[must_use]
pub fn parse_names(names: &[String]) -> Vec<Encoding> {
    names
        .iter()
        .filter_map(|name| Encoding::from_name(name))
        .collect()
}

/// Makes the frame for the JSON message `text`, compressed with the best
/// encoding in `accepted` when that is worth it.
///
/// # Errors
///
/// [`Error::Compression`] if an encoder fails.
pub fn encode(text: String, accepted: &[Encoding]) -> Result<Frame, Error> {
    if text.len() < THRESHOLD {
        return Ok(Frame::text(text));
    }
    let Some(encoding) = Encoding::ALL
        .into_iter()
        .find(|encoding| *encoding != Encoding::Identity && accepted.contains(encoding))
    else {
        return Ok(Frame::text(text));
    };
    let compressed = encoding.compress(text.as_bytes())?;
    if compressed.len() + 1 >= text.len() {
        return Ok(Frame::text(text));
    }
    let mut data = Vec::with_capacity(compressed.len() + 1);
    data.push(encoding.tag());
    data.extend_from_slice(&compressed);
    Ok(Frame::binary(data))
}

/// The JSON message in `frame`, expanded if it was compressed.
///
/// # Errors
///
/// [`Error::Compression`] for a binary frame that is empty, names an
/// unknown encoding or does not expand, and [`Error::Protocol`] if the
/// message is not UTF-8.
pub fn decode(frame: &Frame) -> Result<String, Error> {
    let bytes = if frame.binary {
        let (&tag, rest) = frame
            .data
            .split_first()
            .ok_or_else(|| Error::Compression("an empty binary frame".to_string()))?;
        let encoding = Encoding::from_tag(tag)
            .ok_or_else(|| Error::Compression(format!("unknown encoding tag {tag}")))?;
        encoding.expand(rest)?
    } else {
        frame.data.clone()
    };
    String::from_utf8(bytes).map_err(|_| Error::Protocol("a message is not UTF-8".to_string()))
}

#[cfg(test)]
mod tests {
    use super::{Encoding, MAX_EXPANDED, THRESHOLD, decode, encode};
    use crate::{Error, Frame};

    fn message(size: usize) -> String {
        let filler = "{\"OTIO_SCHEMA\":\"Gap.1\",\"name\":\"\"},".repeat(size / 32 + 1);
        format!(
            "{{\"v\":2,\"type\":\"snapshot\",\"payload\":[{}null]}}",
            filler
        )
    }

    #[test]
    fn every_encoding_round_trips() {
        let text = message(10_000);
        for encoding in Encoding::ALL {
            let compressed = encoding.compress(text.as_bytes()).expect("compresses");
            assert_eq!(
                encoding.expand(&compressed).expect("expands"),
                text.as_bytes()
            );
            assert_eq!(Encoding::from_tag(encoding.tag()), Some(encoding));
            assert_eq!(Encoding::from_name(encoding.name()), Some(encoding));
        }
    }

    #[test]
    fn small_messages_and_unaccepted_encodings_stay_text() {
        let small = message(10);
        assert!(small.len() < THRESHOLD);
        assert!(!encode(small, &Encoding::ALL).expect("ok").binary);
        let large = message(10_000);
        assert!(!encode(large.clone(), &[]).expect("ok").binary);
        assert!(
            !encode(large.clone(), &[Encoding::Identity])
                .expect("ok")
                .binary
        );
        let frame = encode(large.clone(), &[Encoding::Gzip]).expect("ok");
        assert_eq!((frame.binary, frame.data[0]), (true, Encoding::Gzip.tag()));
        assert!(frame.data.len() < large.len() / 10);
        assert_eq!(decode(&frame).expect("expands"), large);
        let frame = encode(large.clone(), &[Encoding::Gzip, Encoding::Brotli]).expect("ok");
        assert_eq!(frame.data[0], Encoding::Brotli.tag(), "Brotli is preferred");
        assert_eq!(decode(&frame).expect("expands"), large);
    }

    #[test]
    fn bad_binary_frames_are_refused() {
        assert!(matches!(
            decode(&Frame::binary(Vec::new())),
            Err(Error::Compression(_))
        ));
        assert!(matches!(
            decode(&Frame::binary(vec![9, 1, 2])),
            Err(Error::Compression(_))
        ));
        assert!(matches!(
            decode(&Frame::binary(vec![1, 1, 2])),
            Err(Error::Compression(_))
        ));
        let bomb = Encoding::Gzip
            .compress(&vec![b' '; MAX_EXPANDED + 1])
            .expect("compresses");
        let mut frame = vec![Encoding::Gzip.tag()];
        frame.extend(bomb);
        assert!(matches!(
            decode(&Frame::binary(frame)),
            Err(Error::Compression(_))
        ));
    }
}
