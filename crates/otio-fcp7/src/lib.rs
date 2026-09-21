//! Final Cut Pro 7 interchange XML, read and written as OpenTimelineIO.
//!
//! FCP 7 is long gone, but the XML it defined is still how a great many tools
//! hand an edit to one another — Premiere Pro, Resolve, Hiero and Media
//! Composer all read or write some dialect of it. This is a port of upstream
//! OpenTimelineIO's `otio-fcp-adapter`.
//!
//! ```
//! use otio_adapter::TextAdapter;
//! use otio_fcp7::Fcp7Xml;
//!
//! let document = Fcp7Xml::read_from_str(r#"
//!     <xmeml version="4">
//!       <sequence>
//!         <name>my cut</name>
//!         <rate><timebase>24</timebase><ntsc>FALSE</ntsc></rate>
//!         <media><video/></media>
//!       </sequence>
//!     </xmeml>"#, &Default::default())?;
//!
//! let root = document.root().expect("a parsed document has a root");
//! assert_eq!(document.try_get(root)?.name(), "my cut");
//! # Ok::<(), otio_adapter::Error>(())
//! ```
//!
//! # What survives a round trip
//!
//! The format carries far more per-element detail than OTIO has fields for.
//! Everything this adapter does not turn into a real OTIO field is kept under
//! the `fcp_xml` key in the relevant object's metadata and written back out on
//! the way past, so a file read and written again keeps its colour settings,
//! its effect parameters and its host application's bookkeeping.
//!
//! Three things are deliberately not kept, because they go stale the moment
//! anything in the edit moves and the writer recomputes them: `timecode`,
//! `rate` and `link` elements, and the `frame` and `string` children of a
//! `timecode`.
//!
//! # Where this differs from upstream
//!
//! Behaviour matches upstream OpenTimelineIO 0.19.0, including its quirks.
//! Three deliberate differences where upstream raises an exception rather
//! than producing a different answer:
//!
//! - A sequence with no `media` element keeps its markers on the timeline
//!   rather than failing. Upstream reaches through a `tracks` that is `None`.
//! - A timeline with no `global_start_time` writes as if it started at zero,
//!   at the rate of its own tracks. Upstream passes `None` where a time is
//!   required.
//! - A `timecode` whose preserved metadata carries a `timebase` and an `ntsc`
//!   flag is written at that rate. Upstream's own code path for this cannot
//!   run, because it looks for keys that its own reader nests one level
//!   deeper, and would fail on the strings it finds if it could.
//!
//! Four more where upstream silently loses or corrupts what it was given:
//!
//! - **A transition keeps its `effect` subtree.** It is the only statement of
//!   what the transition actually is — the effect id, the wipe code and
//!   accuracy, the start and end ratios, the reverse flag — and OTIO has a
//!   field for none of it. Upstream keeps the display name and drops the
//!   rest, so every wipe comes back out as a plain cross dissolve.
//! - **`enabled` is written from the field.** Upstream's writer never looks
//!   at it and reproduces whatever the file it read happened to say, so a
//!   clip disabled in code is written as enabled and one re-enabled stays
//!   disabled.
//! - **Filters are written from the effect list.** Upstream's writer never
//!   looks at `effects` either, so an effect added in code is not written and
//!   one deleted in code is written anyway.
//! - **A `file`'s `timecode` is read at the file's rate.** Upstream reads it
//!   in the clip's context while the same element, read again inside the
//!   media reference, gets the file's — so a 24fps file in a 29.97 sequence
//!   gets a media start several seconds away from its own available range.

mod dict;
mod err;
mod read;
mod util;
mod write;

use otio_adapter::{Adapter, Result, TextAdapter};
use otio_core::Document;

pub use dict::META_NAMESPACE;

/// What to do while reading FCP 7 XML.
///
/// Upstream's adapter takes no arguments, and neither does this one; the type
/// exists so that it can grow one without changing the trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ReadOptions {}

/// What to do while writing FCP 7 XML.
///
/// As [`ReadOptions`]: there is nothing to choose yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct WriteOptions {}

/// The FCP 7 XML adapter.
///
/// This is a marker: everything it does is on [`Adapter`] and [`TextAdapter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fcp7Xml;

impl Adapter for Fcp7Xml {
    type ReadOptions = ReadOptions;
    type WriteOptions = WriteOptions;

    const NAME: &'static str = "fcp_xml";
    const SUFFIXES: &'static [&'static str] = &["xml"];

    fn read_from_bytes(input: &[u8], options: &Self::ReadOptions) -> Result<Document> {
        Self::read_from_str(std::str::from_utf8(input)?, options)
    }

    fn write_to_bytes(document: &Document, options: &Self::WriteOptions) -> Result<Vec<u8>> {
        Ok(Self::write_to_string(document, options)?.into_bytes())
    }
}

impl TextAdapter for Fcp7Xml {
    fn read_from_str(input: &str, _options: &Self::ReadOptions) -> Result<Document> {
        read::read_from_string(input)
    }

    fn write_to_string(document: &Document, _options: &Self::WriteOptions) -> Result<String> {
        write::write_to_string(document)
    }
}
