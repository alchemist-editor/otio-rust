//! Final Cut Pro X XML, read and written as OpenTimelineIO.
//!
//! This is a port of upstream OpenTimelineIO's `otio-fcpx-xml-adapter`.
//!
//! ```
//! use otio_adapter::TextAdapter;
//! use otio_fcpx::FcpxXml;
//!
//! let document = FcpxXml::read_from_str(r#"
//!     <fcpxml version="1.8">
//!       <resources>
//!         <format id="r1" frameDuration="100/3000s"/>
//!         <asset id="r2" name="shot" src="file:///shot.mov"
//!                format="r1" start="0s" duration="10s"/>
//!       </resources>
//!       <project name="my cut">
//!         <sequence format="r1" duration="10s">
//!           <spine>
//!             <asset-clip name="shot" ref="r2" offset="0s" duration="10s"/>
//!           </spine>
//!         </sequence>
//!       </project>
//!     </fcpxml>"#, &Default::default())?;
//!
//! let root = document.root().expect("a parsed document has a root");
//! assert_eq!(document.try_get(root)?.name(), "my cut");
//! # Ok::<(), otio_adapter::Error>(())
//! ```
//!
//! # How the format maps onto OTIO
//!
//! Final Cut does not think in tracks. A sequence holds one `spine`, the main
//! storyline, and everything layered over or under it hangs off whichever
//! storyline item it overlaps, carrying a `lane` number saying how far above
//! or below it sits. Reading means working out where each element really
//! starts and grouping the results by lane, one track per lane, named after
//! it. Writing puts lane zero back into the spine and reattaches the rest.
//!
//! What comes back from a read depends on what the file holds: a library or
//! an event becomes a `SerializableCollection` of timelines, a bare project
//! becomes a `Timeline`, and a file of loose clips becomes a collection of
//! clips and compound clips.
//!
//! What Final Cut knows about a piece of media that OTIO has no field for —
//! its note, its keywords and its Spotlight metadata — is kept under the
//! `fcpx` key in the media reference's metadata and written back out on the
//! way past. Upstream declares a `META_NAMESPACE` of `fcpx_xml` and then
//! never uses it; `fcpx` is the key it actually reads and writes, so that is
//! the one [`META_NAMESPACE`] carries here.
//!
//! # Where this differs from upstream
//!
//! Behaviour matches upstream, including its quirks — notably that every time
//! is truncated to a frame rather than rounded, so a value a hair under a
//! frame boundary falls to the frame below. Four deliberate differences:
//!
//! - **A library with more than one event keeps all of them.** Upstream reads
//!   the first event and silently drops the rest, along with every timeline in
//!   them. Here the later events' projects are read into the same collection.
//!   An FCP X file holds one event, so a write puts them all back under one,
//!   which is what upstream's writer does with them in any case.
//! - **Lanes are ordered as numbers, not as strings.** Upstream sorts the lane
//!   spellings, so lane `10` composites below lane `2`. It never shows on a
//!   file with fewer than ten lanes, which is every file upstream tests, but
//!   on one that has them the picture is stacked wrongly.
//! - **A format is never named.** Upstream runs `ffprobe` to find the frame
//!   size for a format's `name`, and writes `""` whenever `ffprobe` is missing
//!   or the media is not on disk — which is the case for every file in its own
//!   test suite. This adapter does not shell out, so the name is always empty.
//!   The naming rule itself is ported, and testable, as [`format_name`].
//! - **An unnamed event is written unnamed.** Upstream falls back to today's
//!   date, which makes a write irreproducible.
//!
//! One upstream failure is reported rather than reproduced: an item in a lane
//! with no storyline item beneath it has nowhere to hang, and upstream
//! dereferences the missing parent. Here it is an [`Error::Unsupported`].
//!
//! [`Error::Unsupported`]: otio_adapter::Error::Unsupported

mod rational;
mod read;
mod write;

use otio_adapter::{Adapter, Result, TextAdapter};
use otio_core::Document;

pub use read::META_NAMESPACE;
pub use write::format_name;

/// What to do while reading FCP X XML.
///
/// Upstream's adapter takes no arguments, and neither does this one; the type
/// exists so that it can grow one without changing the trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ReadOptions {}

/// What to do while writing FCP X XML.
///
/// As [`ReadOptions`]: there is nothing to choose yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct WriteOptions {}

/// The FCP X XML adapter.
///
/// This is a marker: everything it does is on [`Adapter`] and [`TextAdapter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FcpxXml;

impl Adapter for FcpxXml {
    type ReadOptions = ReadOptions;
    type WriteOptions = WriteOptions;

    const NAME: &'static str = "fcpx_xml";
    const SUFFIXES: &'static [&'static str] = &["fcpxml"];

    fn read_from_bytes(input: &[u8], options: &Self::ReadOptions) -> Result<Document> {
        Self::read_from_str(std::str::from_utf8(input)?, options)
    }

    fn write_to_bytes(document: &Document, options: &Self::WriteOptions) -> Result<Vec<u8>> {
        Ok(Self::write_to_string(document, options)?.into_bytes())
    }
}

impl TextAdapter for FcpxXml {
    fn read_from_str(input: &str, _options: &Self::ReadOptions) -> Result<Document> {
        read::read_from_string(input)
    }

    fn write_to_string(document: &Document, _options: &Self::WriteOptions) -> Result<String> {
        write::write_to_string(document)
    }
}
