//! Writing a document out as an ALE.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use opentime::{DropFrame, RationalTime, TimeRange};
use otio_adapter::text::float;
use otio_adapter::{Error, Result};
use otio_core::schema::Node;
use otio_core::{Any, AnyDictionary, Document, NodeId};

use crate::video_format::{DEFAULT_VIDEO_FORMAT, parse_image_size, video_format_for};
use crate::{DEFAULT_FPS, WriteOptions, root};

/// The columns an ALE always carries, whatever the document says.
///
/// These are the five this adapter derives from a clip's real fields rather
/// than from its metadata, so a file without them would lose the clip's
/// timing and its media.
const REQUIRED_COLUMNS: [&str; 5] = ["Duration", "End", "Start", "Name", "Source File"];

/// Writes an ALE.
pub fn write(document: &Document, options: &WriteOptions) -> Result<String> {
    let root = root(document)?;
    let clips = document.find_clips(root)?;
    let file_metadata = ale_metadata(document, root);

    let mut header = heading(document, root);
    // Every value here is separated with tabs, so say so whatever the
    // document carried.
    header.insert("FIELD_DELIM".to_string(), "TABS".to_string());

    let fps = resolve_fps(options, &mut header)?;
    resolve_video_format(document, &clips, options, &mut header);

    let columns = resolve_columns(document, &clips, options, file_metadata.as_ref());

    let mut result = String::from("Heading\n");
    for (key, value) in &header {
        let _ = writeln!(result, "{key}\t{value}");
    }

    let _ = write!(result, "\nColumn\n{}\n", columns.join("\t"));
    result.push_str("\nData\n");

    for clip in clips {
        let row: Vec<String> = columns
            .iter()
            .map(|column| value_for_column(document, clip, column, fps))
            .collect::<Result<_>>()?;
        let _ = writeln!(result, "{}", row.join("\t"));
    }

    Ok(result)
}

/// Returns the document root's `ALE` metadata, if it has any.
fn ale_metadata(document: &Document, root: NodeId) -> Option<AnyDictionary> {
    document
        .get(root)?
        .base()?
        .metadata
        .get("ALE")?
        .as_dictionary()
        .cloned()
}

/// Returns the heading the document carries, as strings.
fn heading(document: &Document, root: NodeId) -> BTreeMap<String, String> {
    ale_metadata(document, root)
        .and_then(|ale| ale.get("header")?.as_dictionary().cloned())
        .unwrap_or_default()
        .iter()
        .map(|(key, value)| (key.clone(), display(value)))
        .collect()
}

/// Settles the rate timecode is written at, and states it in the heading.
///
/// A rate the caller gave replaces whatever the heading said. Otherwise the
/// heading's own rate stands, and a heading with none gains the default.
fn resolve_fps(options: &WriteOptions, header: &mut BTreeMap<String, String>) -> Result<f64> {
    if let Some(fps) = options.fps {
        header.insert("FPS".to_string(), float(fps));
        return Ok(fps);
    }

    match header.get("FPS") {
        Some(stated) => stated.trim().parse().map_err(|_| {
            Error::unsupported(format!("the heading's FPS is not a number: {stated}"))
        }),
        None => {
            // Written without a decimal point, which is what upstream's
            // str(24) produces and what real headings look like.
            header.insert("FPS".to_string(), "24".to_string());
            Ok(DEFAULT_FPS)
        }
    }
}

/// States a `VIDEO_FORMAT` in the heading, if one is wanted and missing.
fn resolve_video_format(
    document: &Document,
    clips: &[NodeId],
    options: &WriteOptions,
    header: &mut BTreeMap<String, String>,
) {
    if let Some(format) = &options.video_format {
        header.insert("VIDEO_FORMAT".to_string(), format.clone());
    } else if !header.contains_key("VIDEO_FORMAT") {
        header.insert(
            "VIDEO_FORMAT".to_string(),
            guess_video_format(document, clips).to_string(),
        );
    }
}

/// Guesses an Avid project format from the clips' `Image Size` columns.
///
/// The largest frame any clip mentions decides it, so a reel holding both HD
/// and 4K is called by the 4K. Width and height are taken independently,
/// which is upstream's behaviour and only differs from taking them together
/// for a set of clips with conflicting aspect ratios.
fn guess_video_format(document: &Document, clips: &[NodeId]) -> &'static str {
    let mut widest = 0;
    let mut tallest = 0;

    for clip in clips {
        let Some((width, height)) = clip_metadata(document, *clip)
            .and_then(|fields| fields.get("Image Size").map(display))
            .as_deref()
            .and_then(parse_image_size)
        else {
            continue;
        };
        widest = widest.max(width);
        tallest = tallest.max(height);
    }

    if tallest == 0 {
        DEFAULT_VIDEO_FORMAT
    } else {
        video_format_for(widest, tallest)
    }
}

/// Settles which columns to write, and in what order.
fn resolve_columns(
    document: &Document,
    clips: &[NodeId],
    options: &WriteOptions,
    file_metadata: Option<&AnyDictionary>,
) -> Vec<String> {
    let mut columns = match &options.columns {
        Some(columns) => columns.clone(),
        None => {
            // The document's own order first, so a file keeps the column
            // order it arrived with, then anything a clip has picked up
            // since.
            let mut columns: Vec<String> = file_metadata
                .and_then(|ale| ale.get("columns")?.as_slice())
                .unwrap_or_default()
                .iter()
                .map(display)
                .collect();

            for clip in clips {
                let Some(fields) = clip_metadata(document, *clip) else {
                    continue;
                };
                for key in fields.keys() {
                    if !columns.iter().any(|column| column == key) {
                        columns.push(key.clone());
                    }
                }
            }
            columns
        }
    };

    // Each missing one goes to the front, so they end up in the reverse of
    // the order listed here: Source File, Name, Start, End, Duration.
    for required in REQUIRED_COLUMNS {
        if !columns.iter().any(|column| column == required) {
            columns.insert(0, required.to_string());
        }
    }

    columns
}

/// Returns a clip's `ALE` metadata, if it has any.
fn clip_metadata(document: &Document, clip: NodeId) -> Option<&AnyDictionary> {
    document
        .get(clip)?
        .base()?
        .metadata
        .get("ALE")?
        .as_dictionary()
}

/// Returns one clip's value for one column.
///
/// The five derived columns are read off the clip; everything else comes from
/// its `ALE` metadata, and a column the clip says nothing about is blank.
fn value_for_column(document: &Document, clip: NodeId, column: &str, fps: f64) -> Result<String> {
    let node = document.try_get(clip)?;

    match column {
        "Name" => Ok(node.name().to_string()),
        "Source File" => Ok(target_url(document, node).unwrap_or_default()),
        "Start" => timecode(source_range(node), TimeRange::start_time, fps),
        "Duration" => timecode(source_range(node), TimeRange::duration, fps),
        "End" => timecode(source_range(node), TimeRange::end_time_exclusive, fps),
        _ => Ok(clip_metadata(document, clip)
            .and_then(|fields| fields.get(column))
            .map(display)
            .unwrap_or_default()),
    }
}

/// Returns a clip's span of its media, if it has one.
fn source_range(node: &Node) -> Option<TimeRange> {
    node.item().and_then(|item| item.source_range)
}

/// Renders one edge of a clip's span as timecode, or nothing if it has none.
fn timecode(
    range: Option<TimeRange>,
    edge: impl Fn(TimeRange) -> RationalTime,
    fps: f64,
) -> Result<String> {
    match range {
        Some(range) => Ok(edge(range).to_timecode_at(fps, DropFrame::InferFromRate)?),
        None => Ok(String::new()),
    }
}

/// Returns the URL of a clip's media, if its active reference has one.
///
/// Only an external reference does. A generated or missing reference has no
/// file to name, and an image sequence names a directory and a pattern rather
/// than a file, which this column has no room for.
fn target_url(document: &Document, node: &Node) -> Option<String> {
    let Node::Clip(clip) = node else {
        return None;
    };
    let reference = clip
        .media_references
        .get(&clip.active_media_reference_key)?;
    match document.get(*reference)? {
        Node::ExternalReference(reference) if !reference.target_url.is_empty() => {
            Some(reference.target_url.clone())
        }
        _ => None,
    }
}

/// Renders a metadata value as a column value.
///
/// A value that means "nothing" in the format it came from — a zero, a false,
/// an empty string or an empty container — is written as blank, which is what
/// upstream's `str(value or "")` does.
fn display(value: &Any) -> String {
    match value {
        Any::Null => String::new(),
        Any::Bool(value) => if *value { "True" } else { "" }.to_string(),
        Any::Int(0) | Any::UInt(0) => String::new(),
        Any::Int(value) => value.to_string(),
        Any::UInt(value) => value.to_string(),
        Any::Double(value) if *value == 0.0 => String::new(),
        Any::Double(value) => float(*value),
        Any::String(value) => value.clone(),
        Any::Vector(values) if values.is_empty() => String::new(),
        Any::Dictionary(values) if values.is_empty() => String::new(),
        // A column is one field of text, so a structured value has no ALE
        // spelling. Upstream writes Python's repr here; this writes Rust's.
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::display;
    use otio_core::Any;

    #[test]
    fn nothing_renders_as_blank() {
        assert_eq!(display(&Any::Null), "");
        assert_eq!(display(&Any::Int(0)), "");
        assert_eq!(display(&Any::Bool(false)), "");
        assert_eq!(display(&Any::String(String::new())), "");
    }
}
