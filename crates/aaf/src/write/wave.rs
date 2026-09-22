//! WAV files, read as pyaaf2's `audio.WaveReader` reads them for
//! `import_audio_essence`.
//!
//! pyaaf2's reader is Python's `wave.Wave_read` with its format chunk parsing
//! replaced, to take `WAVE_FORMAT_EXTENSIBLE` as well as plain PCM and to
//! keep the block alignment. Everything else is `wave`'s: the chunk walk,
//! which stops at the first `data` chunk, and reads that stop at the end of
//! the chunk or of the file, whichever comes first.

use crate::error::{Error, Result};

const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_EXTENSIBLE_PCM: u16 = 0xFFFE;

/// A WAV file's format and samples.
#[derive(Debug, Clone)]
pub(crate) struct Wave<'a> {
    pub(crate) channels: u16,
    pub(crate) sample_rate: u32,
    /// Bytes per sample.
    pub(crate) sample_width: u16,
    pub(crate) block_align: u16,
    /// Frames the `data` chunk says it holds: `getnframes()`.
    pub(crate) frames: u64,
    /// The samples, as far as the file has them.
    data: &'a [u8],
    frame_size: usize,
    pos: usize,
}

fn error(reason: &str) -> Error {
    Error::InvalidMedia {
        reason: reason.to_owned(),
    }
}

/// One chunk: `wave._Chunk`, over a region of the file.
struct Chunk<'a> {
    name: &'a [u8],
    size: usize,
    /// The chunk's bytes, cut short where the region it is in ends.
    body: &'a [u8],
}

/// Reads the chunk at the start of `region`, or `None` where `wave` raises
/// `EOFError`, which ends a chunk walk.
fn chunk(region: &[u8]) -> Option<Chunk<'_>> {
    if region.len() < 8 {
        return None;
    }
    let size = u32::from_le_bytes([region[4], region[5], region[6], region[7]]) as usize;
    let body = &region[8..];
    Some(Chunk {
        name: &region[..4],
        size,
        body: &body[..size.min(body.len())],
    })
}

impl<'a> Wave<'a> {
    /// Reads a WAV file's header: `WaveReader(path)`.
    ///
    /// # Errors
    ///
    /// Returns an error, with the message Python's `wave.Error` carries, for
    /// a file that is not a WAV file, has no format or data chunk, or is in
    /// a format other than PCM at 16 or 24 bits a sample.
    pub(crate) fn parse(file: &'a [u8]) -> Result<Self> {
        let riff = chunk(file).ok_or_else(|| error("file does not start with RIFF id"))?;
        if riff.name != b"RIFF" {
            return Err(error("file does not start with RIFF id"));
        }
        if riff.body.get(..4) != Some(b"WAVE") {
            return Err(error("not a WAVE file"));
        }
        let mut region = &riff.body[4..];
        let mut format = None;
        while let Some(c) = chunk(region) {
            if c.name == b"fmt " {
                format = Some(Self::format(c.body)?);
            } else if c.name == b"data" {
                let Some((channels, sample_rate, sample_width, block_align)) = format else {
                    return Err(error("data chunk before fmt chunk"));
                };
                let frame_size = usize::from(channels) * usize::from(sample_width);
                if frame_size == 0 {
                    return Err(error("bad # of channels"));
                }
                return Ok(Self {
                    channels,
                    sample_rate,
                    sample_width,
                    block_align,
                    frames: (c.size / frame_size) as u64,
                    data: c.body,
                    frame_size,
                    pos: 0,
                });
            }
            // Skip the rest of the chunk, and its pad byte if it is odd.
            let skip = 8 + c.size + (c.size & 1);
            region = region.get(skip..).unwrap_or_default();
        }
        Err(error("fmt chunk and/or data chunk missing"))
    }

    /// pyaaf2's `WaveReader._read_fmt_chunk`: channels, sample rate, bytes a
    /// sample and block alignment.
    fn format(body: &[u8]) -> Result<(u16, u32, u16, u16)> {
        let u16_at = |i: usize| body.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
        let (Some(tag), Some(channels), Some(rate), Some(align)) = (
            u16_at(0),
            u16_at(2),
            body.get(4..8)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            u16_at(12),
        ) else {
            return Err(error("fmt chunk is too short"));
        };
        if tag != WAVE_FORMAT_PCM && tag != WAVE_EXTENSIBLE_PCM {
            return Err(Error::InvalidMedia {
                reason: format!("unknown format: {tag}"),
            });
        }
        let bits = u16_at(14).ok_or_else(|| error("fmt chunk is too short"))?;
        let sample_width = bits.div_ceil(8);
        if !matches!(sample_width, 2 | 3) {
            return Err(Error::InvalidMedia {
                reason: format!("unsupported sample width: {sample_width}"),
            });
        }
        Ok((channels, rate, sample_width, align))
    }

    /// The next `frames` frames, or as many as are left: `readframes`.
    pub(crate) fn read_frames(&mut self, frames: usize) -> &'a [u8] {
        let want = frames.saturating_mul(self.frame_size);
        let end = self.pos.saturating_add(want).min(self.data.len());
        let out = &self.data[self.pos..end];
        self.pos = end;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(tag: u16, channels: u16, bits: u16, samples: &[u8]) -> Vec<u8> {
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&tag.to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&48000u32.to_le_bytes());
        let align = channels * bits.div_ceil(8);
        fmt.extend_from_slice(&(48000 * u32::from(align)).to_le_bytes());
        fmt.extend_from_slice(&align.to_le_bytes());
        fmt.extend_from_slice(&bits.to_le_bytes());
        let mut body = b"WAVE".to_vec();
        body.extend_from_slice(b"LIST\x03\0\0\0abc\0");
        body.extend_from_slice(b"fmt ");
        body.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        body.extend_from_slice(&fmt);
        body.extend_from_slice(b"data");
        body.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        body.extend_from_slice(samples);
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file
    }

    #[test]
    fn reads_the_format_past_an_odd_chunk_and_the_samples_in_frames() {
        let file = wav(1, 2, 16, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let mut w = Wave::parse(&file).unwrap();
        assert_eq!((w.channels, w.sample_rate, w.sample_width), (2, 48000, 2));
        assert_eq!((w.block_align, w.frames), (4, 2));
        assert_eq!(w.read_frames(1), [1, 2, 3, 4]);
        // A read runs to the end of the chunk, a partial frame and all, as
        // `wave`'s does.
        assert_eq!(w.read_frames(5), [5, 6, 7, 8, 9, 10]);
        assert!(w.read_frames(5).is_empty());
    }

    #[test]
    fn takes_extensible_pcm_and_refuses_the_rest_as_pyaaf2_does() {
        assert!(Wave::parse(&wav(0xFFFE, 1, 24, &[0; 6])).is_ok());
        let error = |file: &[u8]| Wave::parse(file).unwrap_err().to_string();
        assert_eq!(error(&wav(3, 1, 32, &[])), "unknown format: 3");
        assert_eq!(error(&wav(1, 1, 8, &[])), "unsupported sample width: 1");
        assert_eq!(error(b"RIFX\0\0\0\0"), "file does not start with RIFF id");
        assert_eq!(error(b"RIFF\x04\0\0\0AVI "), "not a WAVE file");
        assert_eq!(
            error(b"RIFF\x04\0\0\0WAVE"),
            "fmt chunk and/or data chunk missing"
        );
    }
}
