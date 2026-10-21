//! ORI Annotations: the review annotations of the ASWF's ORI project, read,
//! written and edited on an OpenTimelineIO timeline.
//!
//! **This is an otio-rust extension, not part of OpenTimelineIO.** The
//! schemas belong to the [ORI Annotations] project, which defines them in
//! Python as an OpenTimelineIO schemadef plugin. Nothing here changes until a
//! program turns the extension on:
//!
//! ```
//! otio_ext_ori_annotations::install();
//! otio_ext::enable(otio_ext_ori_annotations::ID)?;
//! // Annotations are built on timeline sync, so enabling them enables it.
//! assert!(otio_ext::is_enabled("sync"));
//! # Ok::<(), otio_ext::Error>(())
//! ```
//!
//! While it is off, `PaintStart.1` and the other ORI schemas read as
//! `UnknownSchema` and are written back unchanged, as upstream
//! OpenTimelineIO treats them when ORI's plugin is not on its path.
//!
//! # What an annotation is
//!
//! ORI draws on a frame with a list of commands: a stroke is a
//! `PaintStart.1`, one or more `PaintPoint.1` batches of points and a
//! `PaintEnd.1`, all sharing one `uuid`; text and shapes are one command
//! each. [`command`] reads and builds them.
//!
//! The commands for one source frame of one media clip live on a one-frame
//! clip in a track named `Annotations`, in the clip's
//! `metadata.annotation_commands`, with `metadata.clip_guid` naming the
//! media clip by its sync GUID. That is the form ORI's sync protocol and
//! Alchemist's review design both chose, and the one [`annotations`] writes.
//! It also reads ORI's older review export, where the track is named after
//! the review and the clip names the media in `annotated_clip_name`, and
//! the `AnnotationEffect.1` effect some files carry instead.
//!
//! ```
//! use opentime::RationalTime;
//! use otio_ext_ori_annotations::{annotations, command, coords};
//!
//! otio_ext_ori_annotations::install();
//! otio_ext::enable(otio_ext_ori_annotations::ID)?;
//!
//! let mut document = otio_core::from_str(r#"{"OTIO_SCHEMA":"Timeline.1",
//!     "tracks":{"OTIO_SCHEMA":"Stack.1","children":[]}}"#)?;
//! let timeline = document.root().unwrap();
//!
//! // The annotation clip for frame 10 of the media clip whose GUID is `shot`.
//! let track = annotations::ensure_track(&mut document, timeline)?;
//! let clip = annotations::ensure_clip(&mut document, track, "shot", RationalTime::new(10.0, 24.0))?;
//!
//! // A red stroke from the top left of a 1920x1080 frame to its centre.
//! let size = coords::ImageSize::new(1920.0, 1080.0);
//! let points = [(200.0, 200.0), (960.0, 540.0)].map(|(x, y)| size.pixels_to_ori(x, y));
//! let stroke = command::stroke(
//!     command::PaintStart { rgba: Some([1.0, 0.0, 0.0, 1.0]), ..command::PaintStart::new("stroke-1") },
//!     command::Vertices::with_size(&points, 0.01),
//! );
//! annotations::add_commands(&mut document, clip, &stroke)?;
//!
//! let found = annotations::list(&document, timeline)?;
//! assert_eq!(found.len(), 1);
//! assert_eq!(found[0].commands.len(), 3);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Coordinates are ORI's: height-normalised, Y up, origin at the image
//! centre. [`coords`] converts to and from pixels.
//!
//! What follows, phase by phase, is in [ADR 0012] and epic #126: the C ABI
//! and every SDK, then live review over timeline sync.
//!
//! [ORI Annotations]: https://github.com/richardssam/ORIAnnotations
//! [ADR 0012]: https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0012-ori-annotations.md

pub mod annotations;
pub mod command;
pub mod coords;
mod error;
pub mod schema;

pub use error::{Error, Result};

use otio_ext::Extension;

/// The id a program enables ORI Annotations by.
pub const ID: &str = "ori-annotations";

/// The ORI Annotations extension.
pub struct OriAnnotations;

/// The one instance [`install`] hands to the framework.
pub static ORI_ANNOTATIONS: OriAnnotations = OriAnnotations;

impl Extension for OriAnnotations {
    fn id(&self) -> &'static str {
        ID
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn summary(&self) -> &'static str {
        "ORI Annotations: the ORI project's review annotations on an OpenTimelineIO timeline."
    }

    fn depends_on(&self) -> &'static [&'static str] {
        &[otio_ext_sync::ID]
    }

    fn schemas(&self) -> &'static [&'static str] {
        &schema::NAMES
    }

    fn register(&self) -> std::result::Result<(), String> {
        schema::register()
    }
}

/// Makes ORI Annotations, and timeline sync which it is built on, available
/// to [`otio_ext::enable`]. Calling it again does nothing.
pub fn install() {
    otio_ext_sync::install();
    // Its id and schema names are its own; installing it conflicts only with
    // a second, different extension claiming them, which is a bug there.
    otio_ext::install(&ORI_ANNOTATIONS)
        .expect("another extension is installed as `ori-annotations` or claims an ORI schema");
}

/// Whether the extension is on.
#[must_use]
pub fn is_enabled() -> bool {
    otio_ext::is_enabled(ID)
}

/// Fails with [`Error::NotEnabled`] unless the extension is on.
fn require_enabled() -> Result<()> {
    if is_enabled() {
        Ok(())
    } else {
        Err(Error::NotEnabled)
    }
}
