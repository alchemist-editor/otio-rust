//! The edit statement: the fixed-shape line at the head of an event.
//!
//! ```text
//! 001  ZZ100_50 V     C        01:00:04:05 01:00:05:12 00:59:53:11 00:59:54:18
//! 002  Clip2    V     D 100    00:00:09:07 00:00:17:15 00:00:01:21 00:00:10:05
//! ```
//!
//! Eight fields is a cut. Nine is a transition, with the extra field carrying
//! how long it runs. The columns are conventionally aligned, but nothing
//! depends on the alignment: the fields are separated by whitespace, and real
//! files use tabs, single spaces and padded columns interchangeably.

use opentime::RationalTime;
use otio_adapter::{Error, Result};

/// One parsed edit statement.
#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    /// The event number, as written.
    pub event_id: String,
    /// The reel the media sits on, or a special identifier such as `BL`.
    pub reel: String,
    /// Which channels the event lands on: `V`, `A1`, `AA/V` and so on.
    pub channel: String,
    /// The edit: `C` for a cut, `D` for a dissolve, `W###` for a wipe.
    pub edit_type: String,
    /// How long a transition runs, in frames. Only a transition has one.
    pub transition_data: Option<String>,
    /// Where the event starts in its source.
    pub source_in: String,
    /// Where the event ends in its source.
    pub source_out: String,
    /// Where the event starts on the timeline.
    pub record_in: String,
    /// Where the event ends on the timeline.
    pub record_out: String,
}

impl Statement {
    /// Parses one edit statement, stated at `rate`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Parse`] if the line does not have eight or nine
    /// fields, if an eight-field line is not a cut, or if a timecode cannot be
    /// read.
    pub fn parse(line: &str, rate: f64) -> Result<Self> {
        let fields: Vec<&str> = line.split_whitespace().collect();

        let mut statement = match fields.as_slice() {
            // A transition. The key transitions (K, KB, KO) also land here,
            // and are refused later: nobody has worked out what their extra
            // field means.
            [
                event_id,
                reel,
                channel,
                edit_type,
                transition_data,
                source_in,
                source_out,
                record_in,
                record_out,
            ] => Self {
                event_id: (*event_id).to_string(),
                reel: (*reel).to_string(),
                channel: (*channel).to_string(),
                edit_type: (*edit_type).to_string(),
                transition_data: Some((*transition_data).to_string()),
                source_in: (*source_in).to_string(),
                source_out: (*source_out).to_string(),
                record_in: (*record_in).to_string(),
                record_out: (*record_out).to_string(),
            },
            // A cut.
            [
                event_id,
                reel,
                channel,
                edit_type,
                source_in,
                source_out,
                record_in,
                record_out,
            ] => {
                if *edit_type != "C" {
                    return Err(Error::parse(format!(
                        "incorrect edit type {edit_type} in form statement: {line}"
                    )));
                }
                Self {
                    event_id: (*event_id).to_string(),
                    reel: (*reel).to_string(),
                    channel: (*channel).to_string(),
                    edit_type: (*edit_type).to_string(),
                    transition_data: None,
                    source_in: (*source_in).to_string(),
                    source_out: (*source_out).to_string(),
                    record_in: (*record_in).to_string(),
                    record_out: (*record_out).to_string(),
                }
            }
            fields => {
                return Err(Error::parse(format!(
                    "incorrect number of fields [{}] in form statement: {line}",
                    fields.len()
                )));
            }
        };

        // Some systems write frame numbers where timecode belongs. Normalize
        // them here so nothing downstream has to know which it was.
        for field in [
            &mut statement.source_in,
            &mut statement.source_out,
            &mut statement.record_in,
            &mut statement.record_out,
        ] {
            if !field.contains(':') {
                *field = frames_as_timecode(field, rate)?;
            }
        }

        Ok(statement)
    }
}

/// Rewrites a frame count as the timecode for the same instant.
fn frames_as_timecode(field: &str, rate: f64) -> Result<String> {
    let frames: i64 = field
        .parse()
        .map_err(|_| Error::parse(format!("not a timecode or a frame number: {field}")))?;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a frame count large enough to lose precision is past any real timeline"
    )]
    Ok(RationalTime::from_frames(frames as f64, rate)
        .to_timecode_at(rate, opentime::DropFrame::InferFromRate)?)
}

/// Returns the number a string begins with, if it begins with one.
///
/// Upstream matches `^\d+`, so `001` and `001A` both read as 1.
#[must_use]
pub fn leading_number(text: &str) -> Option<u64> {
    let digits = text
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(text.len());
    text[..digits].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{Statement, leading_number};

    #[test]
    fn reads_a_cut() {
        let statement = Statement::parse(
            "001  ZZ100_50 V     C        01:00:04:05 01:00:05:12 00:59:53:11 00:59:54:18",
            24.0,
        )
        .expect("a cut");
        assert_eq!(statement.reel, "ZZ100_50");
        assert_eq!(statement.channel, "V");
        assert_eq!(statement.transition_data, None);
        assert_eq!(statement.record_out, "00:59:54:18");
    }

    #[test]
    fn reads_a_dissolve() {
        let statement = Statement::parse(
            "002  Clip2    V     D 100    00:00:09:07 00:00:17:15 00:00:01:21 00:00:10:05",
            24.0,
        )
        .expect("a dissolve");
        assert_eq!(statement.edit_type, "D");
        assert_eq!(statement.transition_data.as_deref(), Some("100"));
    }

    #[test]
    fn tabs_and_single_spaces_separate_as_well_as_columns_do() {
        let statement = Statement::parse(
            "001  Z10 V  C\t\t01:00:04:05 01:00:05:12 00:59:53:11 00:59:54:18",
            24.0,
        )
        .expect("a cut");
        assert_eq!(statement.reel, "Z10");
        assert_eq!(statement.source_in, "01:00:04:05");
    }

    #[test]
    fn frame_numbers_read_as_well_as_timecode() {
        let statement = Statement::parse("1 CLPA V C     113 170 0 57", 24.0).expect("a cut");
        assert_eq!(statement.source_in, "00:00:04:17");
        assert_eq!(statement.record_in, "00:00:00:00");
    }

    #[test]
    fn an_eight_field_line_has_to_be_a_cut() {
        let error = Statement::parse("1 CLPA V D 113 170 0 57", 24.0).expect_err("not a cut");
        assert!(error.to_string().contains("incorrect edit type"));
    }

    #[test]
    fn an_event_id_reads_up_to_its_first_non_digit() {
        assert_eq!(leading_number("001"), Some(1));
        assert_eq!(leading_number("001A"), Some(1));
        assert_eq!(leading_number("A001"), None);
    }
}
