//! Reading an ALE into a collection of clips.

use std::collections::BTreeMap;

use opentime::{RationalTime, TimeRange};
use otio_adapter::cdl::Cdl;
use otio_adapter::{Error, Result};
use otio_core::schema::{
    Base, Clip, ExternalReference, ItemData, MediaReferenceData, Node, SerializableCollection,
};
use otio_core::upgrade::DEFAULT_MEDIA_KEY;
use otio_core::{Any, AnyDictionary, Document, NodeId};

use crate::ReadOptions;

/// The three words that begin a section, and so end the one before it.
const SECTION_DESIGNATORS: [&str; 3] = ["Heading", "Column", "Data"];

/// Reads an ALE.
pub fn read(input: &str, options: &ReadOptions) -> Result<Document> {
    let lines = split_lines(input);
    let mut header: BTreeMap<String, String> = BTreeMap::new();
    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<(usize, &str)> = Vec::new();
    let mut fps = options.fps;

    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        at += 1;

        if line.trim().is_empty() {
            continue;
        }

        if line.trim() == "Heading" {
            read_heading(&lines, &mut at, &mut header)?;
        }

        // Upstream re-reads the heading's rate on every pass of this loop
        // rather than once after the heading, which makes no difference:
        // nothing after the heading changes it.
        if let Some(stated) = header.get("FPS") {
            fps = rate_from_heading(stated)?;
        }

        if line.trim() == "Column" {
            if at >= lines.len() {
                return Err(Error::parse_at(at, "unexpected end of file after 'Column'"));
            }
            columns = read_columns(&lines, &mut at)?;
        }

        if line.trim() == "Data" {
            while at < lines.len() {
                let row = lines[at];
                at += 1;
                if !row.trim().is_empty() {
                    rows.push((at, row));
                }
            }
        }
    }

    let mut document = Document::new();
    let mut children = Vec::with_capacity(rows.len());
    for (line, row) in rows {
        children.push(
            read_row(&mut document, row, &columns, fps, &options.name_column)
                .map_err(|error| error.at_line(line))?,
        );
    }

    // A collection holds its members without parenting them: it groups
    // objects rather than laying them out in time, so `append_child`, which
    // is about compositions, does not apply.
    let collection = document.insert(Node::SerializableCollection(SerializableCollection {
        base: Base {
            name: String::new(),
            metadata: file_metadata(&header, &columns),
            extension: None,
        },
        children,
    }));
    document.set_root(Some(collection));

    Ok(document)
}

/// Splits input into lines on every ending a text file might use.
///
/// Upstream reads these files through Python, whose universal newlines turn a
/// bare carriage return into a line ending too. Some ALEs are old enough for
/// that to matter.
fn split_lines(input: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = input.as_bytes();

    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\n' => {
                lines.push(&input[start..at]);
                at += 1;
                start = at;
            }
            b'\r' => {
                lines.push(&input[start..at]);
                at += if bytes.get(at + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = at;
            }
            _ => at += 1,
        }
    }
    if start < bytes.len() {
        lines.push(&input[start..]);
    }

    lines
}

/// Reads the key/value pairs of the `Heading` section.
///
/// Stops at the next section designator without consuming it. A heading line
/// is any number of tab-separated pairs, so both one pair per line and every
/// pair on one line are valid, and real files use each.
fn read_heading(
    lines: &[&str],
    at: &mut usize,
    header: &mut BTreeMap<String, String>,
) -> Result<()> {
    while *at < lines.len() && !SECTION_DESIGNATORS.contains(&lines[*at]) {
        let line = lines[*at];
        *at += 1;

        if line.trim().is_empty() {
            continue;
        }

        let segments: Vec<&str> = line.split('\t').collect();
        if segments.len() < 2 || segments.len() % 2 != 0 {
            return Err(Error::parse_at(
                *at,
                format!("invalid heading line: {line}"),
            ));
        }
        for pair in segments.chunks_exact(2) {
            header.insert(pair[0].to_string(), pair[1].to_string());
        }
    }

    Ok(())
}

/// Reads the column names, skipping any blank lines before them.
fn read_columns(lines: &[&str], at: &mut usize) -> Result<Vec<String>> {
    loop {
        let Some(line) = lines.get(*at) else {
            return Err(Error::parse_at(
                *at,
                "unexpected end of file after 'Column'",
            ));
        };
        *at += 1;
        if !line.trim().is_empty() {
            return Ok(line.split('\t').map(str::to_string).collect());
        }
    }
}

/// Returns the rate to read timecode at, given the heading's `FPS`.
///
/// Timecode only exists at SMPTE rates, so a heading stating 23.976 means the
/// 24000/1001 rate that is written as 23.976. A rate that is not near any
/// SMPTE rate is refused rather than silently rounded to one.
fn rate_from_heading(stated: &str) -> Result<f64> {
    let stated_rate: f64 = stated
        .trim()
        .parse()
        .map_err(|_| Error::parse(format!("FPS is not a number: {stated}")))?;
    let rate = RationalTime::nearest_smpte_timecode_rate(stated_rate);
    if (stated_rate - rate).abs() > 1.0 {
        return Err(Error::parse(format!(
            "FPS is not a supported SMPTE timecode frame rate: {stated}"
        )));
    }
    Ok(rate)
}

/// Builds the collection's own metadata: the heading, and the column order.
///
/// Keeping the column order is what lets a file written back out have its
/// columns in the order it arrived with, rather than in whatever order a
/// dictionary happens to yield.
fn file_metadata(header: &BTreeMap<String, String>, columns: &[String]) -> AnyDictionary {
    let mut ale = AnyDictionary::new();
    ale.insert("header".to_string(), Any::Dictionary(strings(header)));
    ale.insert(
        "columns".to_string(),
        Any::Vector(
            columns
                .iter()
                .map(|name| Any::String(name.clone()))
                .collect(),
        ),
    );

    let mut metadata = AnyDictionary::new();
    metadata.insert("ALE".to_string(), Any::Dictionary(ale));
    metadata
}

/// Wraps a map of strings as metadata.
fn strings(values: &BTreeMap<String, String>) -> AnyDictionary {
    values
        .iter()
        .map(|(key, value)| (key.clone(), Any::String(value.clone())))
        .collect()
}

/// Reads one row of the `Data` section as a clip.
fn read_row(
    document: &mut Document,
    line: &str,
    columns: &[String],
    fps: f64,
    name_column: &str,
) -> Result<NodeId> {
    let row: Vec<&str> = line.split('\t').collect();
    if row.len() > columns.len() {
        return Err(Error::parse(format!("too many values on row: {line}")));
    }

    // Columns the row does not reach are blank rather than absent, which is
    // what lets a writer that stopped early still round trip.
    let mut fields: BTreeMap<String, String> = columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            (
                column.clone(),
                row.get(index).copied().unwrap_or_default().to_string(),
            )
        })
        .collect();

    // The name column is read but not removed, so it stays in the clip's ALE
    // metadata as well. That is upstream's behaviour, and the writer relies
    // on it: it discovers columns from that metadata.
    let name = fields.get(name_column).cloned().unwrap_or_default();

    let source_range = read_source_range(&mut fields, fps, line)?;
    let media_reference = read_media_reference(document, &mut fields);
    let cdl = read_cdl(&mut fields);

    let mut metadata = AnyDictionary::new();
    if !cdl.is_empty() {
        metadata.insert("cdl".to_string(), Any::Dictionary(cdl.to_metadata()));
    }
    metadata.insert("ALE".to_string(), Any::Dictionary(strings(&fields)));

    let mut media_references = std::collections::BTreeMap::new();
    let mut active_media_reference_key = String::new();
    if let Some(reference) = media_reference {
        media_references.insert(DEFAULT_MEDIA_KEY.to_string(), reference);
        active_media_reference_key = DEFAULT_MEDIA_KEY.to_string();
    }

    Ok(document.insert(Node::Clip(Clip {
        item: ItemData {
            base: Base::new(name, metadata),
            source_range,
            ..ItemData::new()
        },
        media_references,
        active_media_reference_key,
    })))
}

/// Takes a field's value if it is present and not blank, removing it.
fn take(fields: &mut BTreeMap<String, String>, key: &str) -> Option<String> {
    if fields.get(key).is_some_and(|value| !value.is_empty()) {
        fields.remove(key)
    } else {
        None
    }
}

/// Reads `Start`, `Duration` and `End` as the clip's span of its media.
///
/// A row needs a `Start` and at least one of the other two. Where all three
/// are stated they have to agree: a row that says otherwise is more likely to
/// be wrong than to mean something.
fn read_source_range(
    fields: &mut BTreeMap<String, String>,
    fps: f64,
    line: &str,
) -> Result<Option<TimeRange>> {
    let Some(value) = take(fields, "Start") else {
        return Ok(None);
    };
    let start = timecode(&value, fps, "Start")?;

    let duration = take(fields, "Duration")
        .map(|value| timecode(&value, fps, "Duration"))
        .transpose()?;
    let end = take(fields, "End")
        .map(|value| timecode(&value, fps, "End"))
        .transpose()?;

    let (duration, end) = match (duration, end) {
        (Some(duration), Some(end)) => (duration, end),
        (Some(duration), None) => (duration, start + duration),
        (None, Some(end)) => (end - start, end),
        (None, None) => {
            // Upstream reaches this case as a TypeError subtracting from
            // None, which its own catch-all turns into a parse error. Say
            // what is actually missing instead.
            return Err(Error::parse(format!(
                "a Start needs a Duration or an End: {line}"
            )));
        }
    };

    if end != start + duration {
        return Err(Error::parse(format!(
            "inconsistent Start, End, Duration: {line}"
        )));
    }

    Ok(Some(TimeRange::new(start, duration)))
}

/// Reads one timecode field.
fn timecode(value: &str, fps: f64, field: &str) -> Result<RationalTime> {
    RationalTime::from_timecode(value, fps)
        .map_err(|_| Error::parse(format!("invalid {field} timecode: {value}")))
}

/// Reads `Source File` as the clip's media.
fn read_media_reference(
    document: &mut Document,
    fields: &mut BTreeMap<String, String>,
) -> Option<NodeId> {
    let target_url = take(fields, "Source File")?;
    Some(document.insert(Node::ExternalReference(ExternalReference {
        media: MediaReferenceData::default(),
        target_url,
    })))
}

/// Reads the colour-decision columns.
///
/// `CDL` carries slope, offset, power and saturation together; `ASC_SOP` and
/// `ASC_SAT` state them separately and win where both appear, because they are
/// the more specific columns.
fn read_cdl(fields: &mut BTreeMap<String, String>) -> Cdl {
    let mut cdl = Cdl::default();

    if let Some(value) = fields.get("CDL").filter(|value| !value.is_empty()) {
        cdl = Cdl::parse_loose(value);
        if !cdl.is_empty() {
            fields.remove("CDL");
        }
    }

    if let Some(value) = fields.get("ASC_SOP").filter(|value| !value.is_empty()) {
        // Reassigned rather than merged, as upstream does: an ASC_SOP column
        // that reads as nothing discards what the CDL column gave, even
        // though that column has already been consumed.
        cdl = Cdl::parse_loose(value);
        if !cdl.is_empty() {
            fields.remove("ASC_SOP");
        }
    }

    if let Some(value) = fields.get("ASC_SAT").filter(|value| !value.is_empty()) {
        if let Ok(sat) = value.trim().parse::<f64>() {
            cdl.sat = Some(sat);
            fields.remove("ASC_SAT");
        }
    }

    cdl
}
