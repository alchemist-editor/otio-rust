//! Just enough of the zip format for `.otioz` bundles.
//!
//! Upstream reads and writes bundles with minizip-ng. What a bundle needs of
//! it is small: two deflated text entries and any number of stored media
//! files on the way out, and on the way in one entry read into memory and the
//! rest extracted to disk. Media files are routinely larger than 4 GiB, and a
//! long image sequence can put more than 65,535 entries in one archive, so
//! both directions speak ZIP64 wherever a field overflows, as minizip-ng does.
//!
//! Entries are never encrypted, spanned or compressed with anything but
//! "stored" and "deflate"; an archive that uses anything else is refused with
//! an error rather than misread.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::crc32::{self, Crc32};
use crate::deflate;

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
const ZIP64_END_OF_CENTRAL_DIRECTORY: u32 = 0x0606_4b50;
const ZIP64_LOCATOR: u32 = 0x0706_4b50;
const ZIP64_EXTRA: u16 = 0x0001;

/// Compression method 0: the bytes as they are.
const STORED: u16 = 0;
/// Compression method 8: raw DEFLATE.
const DEFLATED: u16 = 8;

/// General purpose flag bit 11: the name is UTF-8.
const FLAG_UTF8: u16 = 0x0800;
/// General purpose flag bit 0: the entry is encrypted.
const FLAG_ENCRYPTED: u16 = 0x0001;

/// "Version made by": Unix, specification 4.5.
const MADE_BY: u16 = 0x0300 | 45;
const NEEDED_DEFAULT: u16 = 20;
const NEEDED_ZIP64: u16 = 45;

const U32_OVERFLOW: u64 = 0xFFFF_FFFF;
const U16_OVERFLOW: u64 = 0xFFFF;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// An entry written so far, as its central directory record needs it.
struct Written {
    name: Vec<u8>,
    method: u16,
    crc: u32,
    compressed: u64,
    uncompressed: u64,
    offset: u64,
    time: u16,
    date: u16,
}

/// Writes a zip archive, one entry at a time.
pub struct ZipWriter<W: Write + Seek> {
    out: W,
    written: Vec<Written>,
    time: u16,
    date: u16,
}

impl ZipWriter<BufWriter<File>> {
    /// Creates the archive at `path`, which must not be open elsewhere.
    ///
    /// # Errors
    ///
    /// Returns the error from creating the file.
    pub fn create(path: &Path) -> io::Result<Self> {
        Ok(Self::new(BufWriter::new(File::create(path)?)))
    }
}

impl<W: Write + Seek> ZipWriter<W> {
    /// Writes an archive to `out`, stamping every entry with the time now.
    pub fn new(out: W) -> Self {
        let (time, date) = dos_date_time(SystemTime::now());
        Self {
            out,
            written: Vec::new(),
            time,
            date,
        }
    }

    /// Adds an entry holding `data`, deflated.
    ///
    /// # Errors
    ///
    /// Returns any error from writing.
    pub fn add_deflated(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        let packed = deflate::deflate(data);
        let crc = crc32::checksum(data);
        let offset = self.out.stream_position()?;
        let zip64 = packed.len() as u64 >= U32_OVERFLOW || data.len() as u64 >= U32_OVERFLOW;
        self.local_header(
            name,
            DEFLATED,
            crc,
            packed.len() as u64,
            data.len() as u64,
            zip64,
        )?;
        self.out.write_all(&packed)?;
        self.written.push(Written {
            name: name.as_bytes().to_vec(),
            method: DEFLATED,
            crc,
            compressed: packed.len() as u64,
            uncompressed: data.len() as u64,
            offset,
            time: self.time,
            date: self.date,
        });
        Ok(())
    }

    /// Adds an entry holding the file at `source`, stored uncompressed.
    ///
    /// The file is streamed rather than read into memory, since media files
    /// are often larger than memory; its checksum is patched into the local
    /// header once it has been read.
    ///
    /// # Errors
    ///
    /// Returns any error from reading `source` or writing the archive.
    pub fn add_stored_file(&mut self, name: &str, source: &Path) -> io::Result<()> {
        let mut input = BufReader::new(File::open(source)?);
        let expected = input.get_ref().metadata()?.len();
        let zip64 = expected >= U32_OVERFLOW;
        let offset = self.out.stream_position()?;
        let crc_at = self.local_header(name, STORED, 0, expected, expected, zip64)?;

        let mut crc = Crc32::new();
        let mut size: u64 = 0;
        let mut buffer = vec![0u8; 1 << 16];
        loop {
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            crc.update(&buffer[..read]);
            self.out.write_all(&buffer[..read])?;
            size += read as u64;
        }
        if size != expected {
            return Err(invalid(format!(
                "'{}' changed size while it was being added",
                source.display()
            )));
        }
        let crc = crc.finish();

        let end = self.out.stream_position()?;
        self.out.seek(SeekFrom::Start(crc_at))?;
        self.out.write_all(&crc.to_le_bytes())?;
        self.out.seek(SeekFrom::Start(end))?;

        self.written.push(Written {
            name: name.as_bytes().to_vec(),
            method: STORED,
            crc,
            compressed: size,
            uncompressed: size,
            offset,
            time: self.time,
            date: self.date,
        });
        Ok(())
    }

    /// Writes a local file header, returning where its checksum field is.
    fn local_header(
        &mut self,
        name: &str,
        method: u16,
        crc: u32,
        compressed: u64,
        uncompressed: u64,
        zip64: bool,
    ) -> io::Result<u64> {
        let mut header = Vec::with_capacity(30 + name.len() + 20);
        put32(&mut header, LOCAL_HEADER);
        put16(
            &mut header,
            if zip64 { NEEDED_ZIP64 } else { NEEDED_DEFAULT },
        );
        put16(&mut header, FLAG_UTF8);
        put16(&mut header, method);
        put16(&mut header, self.time);
        put16(&mut header, self.date);
        let crc_offset = header.len() as u64;
        put32(&mut header, crc);
        if zip64 {
            put32(&mut header, U32_OVERFLOW as u32);
            put32(&mut header, U32_OVERFLOW as u32);
        } else {
            put32(&mut header, compressed as u32);
            put32(&mut header, uncompressed as u32);
        }
        put16(&mut header, name_length(name)?);
        put16(&mut header, if zip64 { 20 } else { 0 });
        header.extend_from_slice(name.as_bytes());
        if zip64 {
            put16(&mut header, ZIP64_EXTRA);
            put16(&mut header, 16);
            put64(&mut header, uncompressed);
            put64(&mut header, compressed);
        }
        let start = self.out.stream_position()?;
        self.out.write_all(&header)?;
        Ok(start + crc_offset)
    }

    /// Writes the central directory and returns the underlying writer.
    ///
    /// # Errors
    ///
    /// Returns any error from writing.
    pub fn finish(mut self) -> io::Result<W> {
        let directory_start = self.out.stream_position()?;
        for entry in &self.written {
            let mut extra = Vec::new();
            if entry.uncompressed >= U32_OVERFLOW {
                put64(&mut extra, entry.uncompressed);
            }
            if entry.compressed >= U32_OVERFLOW {
                put64(&mut extra, entry.compressed);
            }
            if entry.offset >= U32_OVERFLOW {
                put64(&mut extra, entry.offset);
            }
            let zip64 = !extra.is_empty();

            let mut record = Vec::with_capacity(46 + entry.name.len() + 4 + extra.len());
            put32(&mut record, CENTRAL_HEADER);
            put16(&mut record, MADE_BY);
            put16(
                &mut record,
                if zip64 { NEEDED_ZIP64 } else { NEEDED_DEFAULT },
            );
            put16(&mut record, FLAG_UTF8);
            put16(&mut record, entry.method);
            put16(&mut record, entry.time);
            put16(&mut record, entry.date);
            put32(&mut record, entry.crc);
            put32(&mut record, entry.compressed.min(U32_OVERFLOW) as u32);
            put32(&mut record, entry.uncompressed.min(U32_OVERFLOW) as u32);
            put16(&mut record, entry.name.len() as u16);
            put16(&mut record, if zip64 { extra.len() as u16 + 4 } else { 0 });
            put16(&mut record, 0); // comment length
            put16(&mut record, 0); // disk number
            put16(&mut record, 0); // internal attributes
            put32(&mut record, 0o100_644 << 16); // a regular file, rw-r--r--
            put32(&mut record, entry.offset.min(U32_OVERFLOW) as u32);
            record.extend_from_slice(&entry.name);
            if zip64 {
                put16(&mut record, ZIP64_EXTRA);
                put16(&mut record, extra.len() as u16);
                record.extend_from_slice(&extra);
            }
            self.out.write_all(&record)?;
        }
        let directory_end = self.out.stream_position()?;
        let directory_size = directory_end - directory_start;
        let count = self.written.len() as u64;

        let mut end = Vec::new();
        if count >= U16_OVERFLOW
            || directory_size >= U32_OVERFLOW
            || directory_start >= U32_OVERFLOW
        {
            put32(&mut end, ZIP64_END_OF_CENTRAL_DIRECTORY);
            put64(&mut end, 44); // the size of the rest of this record
            put16(&mut end, MADE_BY);
            put16(&mut end, NEEDED_ZIP64);
            put32(&mut end, 0); // this disk
            put32(&mut end, 0); // the disk the directory starts on
            put64(&mut end, count);
            put64(&mut end, count);
            put64(&mut end, directory_size);
            put64(&mut end, directory_start);

            put32(&mut end, ZIP64_LOCATOR);
            put32(&mut end, 0);
            put64(&mut end, directory_end);
            put32(&mut end, 1); // total number of disks
        }
        put32(&mut end, END_OF_CENTRAL_DIRECTORY);
        put16(&mut end, 0);
        put16(&mut end, 0);
        put16(&mut end, count.min(U16_OVERFLOW) as u16);
        put16(&mut end, count.min(U16_OVERFLOW) as u16);
        put32(&mut end, directory_size.min(U32_OVERFLOW) as u32);
        put32(&mut end, directory_start.min(U32_OVERFLOW) as u32);
        put16(&mut end, 0); // comment length
        self.out.write_all(&end)?;
        self.out.flush()?;
        Ok(self.out)
    }
}

fn name_length(name: &str) -> io::Result<u16> {
    u16::try_from(name.len()).map_err(|_| invalid(format!("entry name too long: '{name}'")))
}

fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// The MS-DOS time and date fields for `when`, in UTC.
fn dos_date_time(when: SystemTime) -> (u16, u16) {
    let seconds = when
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let days = (seconds / 86_400) as i64;
    let of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    // DOS dates start in 1980 and cannot say anything earlier.
    if year < 1980 {
        return (0, (1 << 5) | 1);
    }
    let time = (((of_day / 3600) << 11) | ((of_day % 3600 / 60) << 5) | ((of_day % 60) / 2)) as u16;
    let date = ((((year - 1980) as u64) << 9) | (u64::from(month) << 5) | u64::from(day)) as u16;
    (time, date)
}

/// The Gregorian date `days` after 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// One entry of an archive being read.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The entry's name, with `/` as the separator.
    pub name: String,
    method: u16,
    flags: u16,
    crc: u32,
    compressed: u64,
    uncompressed: u64,
    offset: u64,
}

impl Entry {
    /// Whether the entry is a directory rather than a file.
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/')
    }
}

/// Reads a zip archive from its central directory.
pub struct ZipReader<R: Read + Seek> {
    input: R,
    entries: Vec<Entry>,
}

impl ZipReader<BufReader<File>> {
    /// Opens the archive at `path`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or is not a zip archive.
    pub fn open(path: &Path) -> io::Result<Self> {
        Self::new(BufReader::new(File::open(path)?))
    }
}

impl<R: Read + Seek> ZipReader<R> {
    /// Reads the central directory of the archive in `input`.
    ///
    /// # Errors
    ///
    /// Returns an error if `input` is not a zip archive this can read.
    pub fn new(mut input: R) -> io::Result<Self> {
        let length = input.seek(SeekFrom::End(0))?;
        // The end record is 22 bytes, followed by a comment of up to 64 KiB.
        let tail_length = length.min(22 + 0xFFFF);
        input.seek(SeekFrom::Start(length - tail_length))?;
        let mut tail = vec![0u8; tail_length as usize];
        input.read_exact(&mut tail)?;
        let end_at = (0..tail.len().saturating_sub(21))
            .rev()
            .find(|&at| get32(&tail, at) == END_OF_CENTRAL_DIRECTORY)
            .ok_or_else(|| invalid("not a zip archive: no end of central directory"))?;
        let end = &tail[end_at..];
        let mut count = u64::from(get16(end, 10));
        let mut directory_size = u64::from(get32(end, 12));
        let mut directory_start = u64::from(get32(end, 16));

        let locator_at = end_at.checked_sub(20);
        if let Some(locator_at) = locator_at.filter(|&at| get32(&tail, at) == ZIP64_LOCATOR) {
            let record_at = get64(&tail, locator_at + 8);
            input.seek(SeekFrom::Start(record_at))?;
            let mut record = [0u8; 56];
            input.read_exact(&mut record)?;
            if get32(&record, 0) != ZIP64_END_OF_CENTRAL_DIRECTORY {
                return Err(invalid("damaged zip64 end of central directory"));
            }
            count = get64(&record, 32);
            directory_size = get64(&record, 40);
            directory_start = get64(&record, 48);
        }

        if directory_start.saturating_add(directory_size) > length {
            return Err(invalid("central directory runs past the end of the file"));
        }
        input.seek(SeekFrom::Start(directory_start))?;
        let mut directory = vec![0u8; directory_size as usize];
        input.read_exact(&mut directory)?;

        let mut entries = Vec::new();
        let mut at = 0usize;
        for _ in 0..count {
            if at + 46 > directory.len() || get32(&directory, at) != CENTRAL_HEADER {
                return Err(invalid("damaged central directory"));
            }
            let record = &directory[at..];
            let name_length = usize::from(get16(record, 28));
            let extra_length = usize::from(get16(record, 30));
            let comment_length = usize::from(get16(record, 32));
            if 46 + name_length + extra_length + comment_length > record.len() {
                return Err(invalid("damaged central directory"));
            }
            let name = String::from_utf8_lossy(&record[46..46 + name_length]).into_owned();
            let extra = &record[46 + name_length..46 + name_length + extra_length];
            let mut entry = Entry {
                name,
                method: get16(record, 10),
                flags: get16(record, 8),
                crc: get32(record, 16),
                compressed: u64::from(get32(record, 20)),
                uncompressed: u64::from(get32(record, 24)),
                offset: u64::from(get32(record, 42)),
            };
            apply_zip64_extra(&mut entry, extra);
            entries.push(entry);
            at += 46 + name_length + extra_length + comment_length;
        }
        Ok(Self { input, entries })
    }

    /// Every entry, in the order the central directory lists them.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Positions the input at the start of `entry`'s data.
    fn seek_to_data(&mut self, entry: &Entry) -> io::Result<()> {
        if entry.flags & FLAG_ENCRYPTED != 0 {
            return Err(invalid(format!("'{}' is encrypted", entry.name)));
        }
        if entry.method != STORED && entry.method != DEFLATED {
            return Err(invalid(format!(
                "'{}' uses unsupported compression method {}",
                entry.name, entry.method
            )));
        }
        self.input.seek(SeekFrom::Start(entry.offset))?;
        let mut header = [0u8; 30];
        self.input.read_exact(&mut header)?;
        if get32(&header, 0) != LOCAL_HEADER {
            return Err(invalid(format!(
                "damaged local header for '{}'",
                entry.name
            )));
        }
        let skip = i64::from(get16(&header, 26)) + i64::from(get16(&header, 28));
        self.input.seek(SeekFrom::Current(skip))?;
        Ok(())
    }

    /// Reads `entry` into memory.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry cannot be read or fails its checksum.
    pub fn read(&mut self, entry: &Entry) -> io::Result<Vec<u8>> {
        self.seek_to_data(entry)?;
        let length = usize::try_from(entry.compressed)
            .map_err(|_| invalid(format!("'{}' is too large to read", entry.name)))?;
        let mut packed = vec![0u8; length];
        self.input.read_exact(&mut packed)?;
        let data = if entry.method == DEFLATED {
            deflate::inflate(&packed).map_err(|error| invalid(error.to_string()))?
        } else {
            packed
        };
        if data.len() as u64 != entry.uncompressed || crc32::checksum(&data) != entry.crc {
            return Err(invalid(format!("'{}' fails its checksum", entry.name)));
        }
        Ok(data)
    }

    /// Writes `entry` to the file at `path`, streaming a stored entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry cannot be read, fails its checksum, or
    /// the file cannot be written.
    pub fn extract(&mut self, entry: &Entry, path: &Path) -> io::Result<()> {
        if entry.method == DEFLATED {
            let data = self.read(entry)?;
            return std::fs::write(path, data);
        }
        self.seek_to_data(entry)?;
        let mut out = BufWriter::new(File::create(path)?);
        let mut crc = Crc32::new();
        let mut left = entry.compressed;
        let mut buffer = vec![0u8; 1 << 16];
        while left > 0 {
            let want = buffer
                .len()
                .min(usize::try_from(left).unwrap_or(usize::MAX));
            self.input.read_exact(&mut buffer[..want])?;
            crc.update(&buffer[..want]);
            out.write_all(&buffer[..want])?;
            left -= want as u64;
        }
        out.flush()?;
        if entry.compressed != entry.uncompressed || crc.finish() != entry.crc {
            return Err(invalid(format!("'{}' fails its checksum", entry.name)));
        }
        Ok(())
    }
}

/// Replaces the overflowed fields of `entry` with their ZIP64 values.
fn apply_zip64_extra(entry: &mut Entry, mut extra: &[u8]) {
    while extra.len() >= 4 {
        let id = get16(extra, 0);
        let size = usize::from(get16(extra, 2));
        let Some(body) = extra.get(4..4 + size) else {
            return;
        };
        if id == ZIP64_EXTRA {
            let mut at = 0;
            let mut next = |field: &mut u64| {
                if *field == U32_OVERFLOW && at + 8 <= body.len() {
                    *field = get64(body, at);
                    at += 8;
                }
            };
            next(&mut entry.uncompressed);
            next(&mut entry.compressed);
            next(&mut entry.offset);
            return;
        }
        extra = &extra[4 + size..];
    }
}

fn get16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn get32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn get64(bytes: &[u8], at: usize) -> u64 {
    u64::from(get32(bytes, at)) | u64::from(get32(bytes, at + 4)) << 32
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{ZipReader, ZipWriter, civil_from_days, dos_date_time};

    #[test]
    fn entries_round_trip_in_memory() {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer.add_deflated("version.txt", b"1.0.0").unwrap();
        writer
            .add_deflated("content.otio", "{ \"a\": 1 }\n".repeat(50).as_bytes())
            .unwrap();
        let bytes = writer.finish().unwrap().into_inner();

        let mut reader = ZipReader::new(Cursor::new(bytes)).unwrap();
        let entries = reader.entries().to_vec();
        let names: Vec<_> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["version.txt", "content.otio"]);
        assert_eq!(reader.read(&entries[0]).unwrap(), b"1.0.0");
        assert_eq!(
            reader.read(&entries[1]).unwrap(),
            "{ \"a\": 1 }\n".repeat(50).as_bytes()
        );
    }

    #[test]
    fn a_corrupted_entry_fails_its_checksum() {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer.add_deflated("a", &[b'x'; 10]).unwrap();
        let mut bytes = writer.finish().unwrap().into_inner();
        // Flip the stored checksum in the central directory.
        let at = bytes
            .windows(4)
            .position(|w| w == 0x0201_4b50u32.to_le_bytes())
            .unwrap();
        bytes[at + 16] ^= 0xFF;
        let mut reader = ZipReader::new(Cursor::new(bytes)).unwrap();
        let entry = reader.entries()[0].clone();
        assert!(reader.read(&entry).is_err());
    }

    #[test]
    fn something_that_is_not_an_archive_is_refused() {
        assert!(ZipReader::new(Cursor::new(b"not a zip".to_vec())).is_err());
    }

    #[test]
    fn dates_are_gregorian_and_dos_starts_in_1980() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        let (_, date) = dos_date_time(std::time::UNIX_EPOCH);
        assert_eq!(date, (1 << 5) | 1);
    }
}
