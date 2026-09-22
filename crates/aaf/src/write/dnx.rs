//! Raw DNxHD and DNxHR streams, read frame by frame as pyaaf2's `video.py`
//! reads them for `import_dnxhd_essence`.
//!
//! A raw stream is its frames one after another, each starting with a
//! 640-byte header that says which compression it is (its CID), how big the
//! picture is and at what bit depth. pyaaf2 reads the header, works out from
//! the CID how long the frame is, and reads the rest; the frames are embedded
//! as they are, and the first one's header describes the essence.

use crate::Auid;
use crate::builtin::auid;
use crate::error::{Error, Result};

/// The size of a frame's header.
const HEADER_SIZE: usize = 640;

/// pyaaf2's `dnxhd_frame_sizes`: the size of a DNxHD frame, by CID.
const DNXHD_FRAME_SIZES: [(u32, usize); 15] = [
    (1235, 917_504),
    (1237, 606_208),
    (1238, 917_504),
    (1241, 917_504),
    (1242, 606_208),
    (1243, 917_504),
    (1244, 606_208),
    (1250, 458_752),
    (1251, 458_752),
    (1252, 303_104),
    (1253, 188_416),
    (1256, 1_835_008),
    (1258, 212_992),
    (1259, 417_792),
    (1260, 835_584),
];

/// pyaaf2's `dnxhr_compression_ratio`: the compression of a DNxHR frame, by
/// CID, from which its size follows.
const DNXHR_COMPRESSION_RATIO: [(u32, usize, usize); 5] = [
    (1270, 57344, 255), // dnxhr_444
    (1271, 28672, 255), // dnxhr_hqx
    (1272, 28672, 255), // dnxhr_hq
    (1273, 18944, 255), // dnxhr_sq
    (1274, 5888, 255),  // dnxhr_lb
];

/// pyaaf2's `dnx_compression_auids`: the `Compression` a descriptor records
/// for each CID.
const DNX_COMPRESSION_AUIDS: [(u32, Auid); 22] = [
    (1235, auid("04010202-7101-0000-060e-2b340401010a")),
    (1236, auid("04010202-7102-0000-060e-2b340401010a")),
    (1237, auid("04010202-7103-0000-060e-2b340401010a")),
    (1238, auid("04010202-7104-0000-060e-2b340401010a")),
    (1241, auid("04010202-7107-0000-060e-2b340401010a")),
    (1242, auid("04010202-7108-0000-060e-2b340401010a")),
    (1243, auid("04010202-7109-0000-060e-2b340401010a")),
    (1244, auid("04010202-710a-0000-060e-2b340401010a")),
    (1250, auid("04010202-7110-0000-060e-2b340401010a")),
    (1251, auid("04010202-7111-0000-060e-2b340401010a")),
    (1252, auid("04010202-7112-0000-060e-2b340401010a")),
    (1253, auid("04010202-7113-0000-060e-2b340401010a")),
    (1256, auid("04010202-7116-0000-060e-2b340401010a")),
    (1257, auid("04010202-7117-0000-060e-2b340401010a")),
    (1258, auid("04010202-7118-0000-060e-2b340401010a")),
    (1259, auid("04010202-7119-0000-060e-2b340401010a")),
    (1260, auid("04010202-711a-0000-060e-2b340401010a")),
    (1270, auid("04010202-7124-0000-060e-2b340401010d")),
    (1271, auid("04010202-7125-0000-060e-2b340401010d")),
    (1272, auid("04010202-7126-0000-060e-2b340401010d")),
    (1273, auid("04010202-7127-0000-060e-2b340401010d")),
    (1274, auid("04010202-7128-0000-060e-2b340401010d")),
];

/// What a frame's header says: pyaaf2's `read_dnx_frame_header`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameHeader {
    pub(crate) cid: u32,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) bitdepth: u8,
    pub(crate) interlaced: bool,
}

impl FrameHeader {
    /// The `Compression` identifier pyaaf2 records for this frame's CID.
    ///
    /// # Errors
    ///
    /// Returns an error for a CID pyaaf2 has none for, where it raises
    /// `KeyError`.
    pub(crate) fn compression(&self) -> Result<Auid> {
        DNX_COMPRESSION_AUIDS
            .iter()
            .find(|(cid, _)| *cid == self.cid)
            .map(|(_, auid)| *auid)
            .ok_or_else(|| invalid(format!("no compression is known for DNx CID {}", self.cid)))
    }
}

fn invalid(reason: String) -> Error {
    Error::InvalidMedia { reason }
}

/// pyaaf2's `valid_dnx_prefix`.
const fn valid_prefix(prefix: u64) -> bool {
    // DNxHD.
    if prefix == 0x0000_0280_0100 {
        return true;
    }
    // DNxHR.
    let data_offset = prefix >> 16;
    (prefix & 0xFFFF_0000_FFFF) == 0x0300
        && data_offset >= 0x0280
        && data_offset <= 0x2170
        && (data_offset & 3) == 0
}

/// Reads a frame's header, as pyaaf2's `read_dnx_frame_header` does, with
/// its messages.
///
/// # Errors
///
/// Returns an error, with the message pyaaf2's `ValueError` carries, for a
/// header that is too short, has an unknown prefix or an unknown bit depth,
/// and for 4:4:4 sampling, which pyaaf2 refuses as untested.
pub(crate) fn read_frame_header(header: &[u8]) -> Result<FrameHeader> {
    if header.len() < HEADER_SIZE {
        return Err(invalid("Invalid DNxHD frame: header to Short".to_owned()));
    }
    let prefix = header[..6]
        .iter()
        .fold(0u64, |n, b| (n << 8) | u64::from(*b))
        & 0xFFFF_FFFF_FF00;
    if !valid_prefix(prefix) {
        return Err(invalid(format!(
            "Invalid DNxHD frame: unknown prefix: 0x{prefix:012X}"
        )));
    }
    // Stored height, then width, as signed shorts; pyaaf2 reads them so.
    let height = u16::from_be_bytes([header[24], header[25]]);
    let width = u16::from_be_bytes([header[26], header[27]]);
    let cid = u32::from_be_bytes([header[40], header[41], header[42], header[43]]);
    let interlaced = header[5] & 2 != 0;
    let bitdepth = match header[33] >> 5 {
        1 => 8,
        2 => 10,
        3 => 12,
        other => {
            return Err(invalid(format!(
                "Invalid DNxHD frame: unknown bitdepth: {other}"
            )));
        }
    };
    if (header[44] >> 6) & 1 != 0 {
        return Err(invalid("444 not tested".to_owned()));
    }
    Ok(FrameHeader {
        cid,
        width,
        height,
        bitdepth,
        interlaced,
    })
}

/// pyaaf2's `dnx_frame_size`: how long a frame of this CID and size is.
fn frame_size(header: &FrameHeader) -> Result<usize> {
    if let Some((_, size)) = DNXHD_FRAME_SIZES.iter().find(|(c, _)| *c == header.cid) {
        return Ok(*size);
    }
    let (_, num, den) = DNXHR_COMPRESSION_RATIO
        .iter()
        .find(|(c, _, _)| *c == header.cid)
        .ok_or_else(|| invalid(format!("unknown DNx CID {}", header.cid)))?;
    let (width, height) = (usize::from(header.width), usize::from(header.height));
    let size = height.div_ceil(16) * width.div_ceil(16) * num / den;
    let size = (size + 2048) / 4096 * 4096;
    Ok(size.max(8192))
}

/// The frames of a raw stream: pyaaf2's `iter_dnx_stream`.
///
/// Each frame is its header and as much of the rest as the stream has; the
/// last may be short, as pyaaf2 reads it. The stream ends at the first read
/// that does not return a whole header.
pub(crate) struct Frames<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Frames<'a> {
    pub(crate) const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl<'a> Iterator for Frames<'a> {
    type Item = Result<(FrameHeader, &'a [u8])>;

    fn next(&mut self) -> Option<Self::Item> {
        let rest = &self.data[self.pos..];
        if rest.len() < HEADER_SIZE {
            return None;
        }
        let header = match read_frame_header(&rest[..HEADER_SIZE]) {
            Ok(header) => header,
            Err(error) => {
                self.pos = self.data.len();
                return Some(Err(error));
            }
        };
        let size = match frame_size(&header) {
            Ok(size) => size.max(HEADER_SIZE),
            Err(error) => {
                self.pos = self.data.len();
                return Some(Err(error));
            }
        };
        let frame = &rest[..size.min(rest.len())];
        self.pos += frame.len();
        Some(Ok((header, frame)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(cid: u32) -> Vec<u8> {
        let mut h = vec![0u8; HEADER_SIZE];
        h[..6].copy_from_slice(&[0x00, 0x00, 0x02, 0x80, 0x01, 0x00]);
        h[24..26].copy_from_slice(&1080u16.to_be_bytes());
        h[26..28].copy_from_slice(&1920u16.to_be_bytes());
        h[33] = 1 << 5;
        h[40..44].copy_from_slice(&cid.to_be_bytes());
        h
    }

    #[test]
    fn reads_a_dnxhd_header() {
        let h = read_frame_header(&header(1253)).unwrap();
        assert_eq!(
            h,
            FrameHeader {
                cid: 1253,
                width: 1920,
                height: 1080,
                bitdepth: 8,
                interlaced: false
            }
        );
        assert_eq!(frame_size(&h).unwrap(), 188_416);
    }

    #[test]
    fn refuses_what_pyaaf2_refuses() {
        let riff = b"RIFF4\0\0\0WAVE";
        let mut data = riff.to_vec();
        data.resize(HEADER_SIZE, 0);
        assert_eq!(
            read_frame_header(&data).unwrap_err().to_string(),
            "Invalid DNxHD frame: unknown prefix: 0x524946463400"
        );
        assert_eq!(
            read_frame_header(&data[..10]).unwrap_err().to_string(),
            "Invalid DNxHD frame: header to Short"
        );
        let mut deep = header(1253);
        deep[33] = 0;
        assert_eq!(
            read_frame_header(&deep).unwrap_err().to_string(),
            "Invalid DNxHD frame: unknown bitdepth: 0"
        );
    }

    #[test]
    fn sizes_dnxhr_frames_by_their_picture() {
        let mut h = read_frame_header(&header(1253)).unwrap();
        h.cid = 1274;
        // 68 * 120 blocks at 5888/255 each, rounded to 4096.
        assert_eq!(frame_size(&h).unwrap(), 188_416);
    }

    #[test]
    fn a_short_last_frame_is_kept_and_a_short_header_ends_the_stream() {
        let frame = |len: usize| {
            let mut f = header(1253);
            f.resize(len, 0);
            f
        };
        // A whole frame, then one cut short, which takes what is left.
        let mut data = frame(188_416);
        data.extend(frame(1000));
        data.extend(frame(600));
        let frames: Vec<_> = Frames::new(&data).map(|f| f.unwrap().1.len()).collect();
        assert_eq!(frames, [188_416, 1600]);
        // Less than a header is no frame at all.
        assert_eq!(Frames::new(&frame(600)).count(), 0);
    }
}
