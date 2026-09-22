//! A track's markers, as upstream writes them: `_MarkerColor`,
//! `transcribe_aaf_descriptive_markers` and `_transcribe_marker`.
//!
//! Media Composer keeps a track's markers on an event slot of the
//! composition, as descriptive markers in a sequence, and finds the track
//! they belong to by the slot's physical track number. Each marker carries
//! its colour twice, in the eight-colour legacy encoding and the sixteen-
//! colour extended one, and mirrors its fields into an attribute list that
//! Media Composer shows in its marker window.

use aaf::write::{ObjRef, WriteValue};
use otio_core::{Any, Node, NodeId};

use super::py::{self, aaf_metadata, first_truthy};
use super::track::Track;
use super::{Fail, FileTranscriber, find_children, unwritable};

/// A colour as a marker stores it: its 16-bit channels and its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rgb {
    name: &'static str,
    red: i64,
    green: i64,
    blue: i64,
}

impl Rgb {
    fn record(self) -> WriteValue {
        WriteValue::record([
            ("red", self.red),
            ("green", self.green),
            ("blue", self.blue),
        ])
    }
}

/// Upstream's `_MarkerColor`: an OTIO marker colour in both of Avid's
/// encodings.
///
/// A colour the legacy encoding has is the same in both. The extended-only
/// colours fall back to a legacy one for the legacy encoding. The five
/// colours only Avid has are not mapped, as upstream does not map them, and
/// a colour with any other name is red.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarkerColor {
    extended: Rgb,
    legacy: Rgb,
}

impl MarkerColor {
    fn for_name(name: &str) -> Self {
        const fn rgb(name: &'static str, red: i64, green: i64, blue: i64) -> Rgb {
            Rgb {
                name,
                red,
                green,
                blue,
            }
        }
        let legacy = |c: Rgb| Self {
            extended: c,
            legacy: c,
        };
        let red = rgb("Red", 41471, 12134, 6564);
        let blue = rgb("Blue", 13107, 13107, 52428);
        let magenta = rgb("Magenta", 52428, 13107, 52428);
        match name.to_uppercase().as_str() {
            "GREEN" => legacy(rgb("Green", 13107, 52428, 13107)),
            "BLUE" => legacy(blue),
            "CYAN" => legacy(rgb("Cyan", 13107, 52428, 52428)),
            "MAGENTA" => legacy(magenta),
            "YELLOW" => legacy(rgb("Yellow", 58981, 58981, 6553)),
            "BLACK" => legacy(rgb("Black", 0, 0, 0)),
            "WHITE" => legacy(rgb("White", 65534, 65535, 65535)),
            "PINK" => Self {
                extended: rgb("Pink", 61184, 34304, 53504),
                legacy: magenta,
            },
            "PURPLE" => Self {
                extended: rgb("Purple", 23552, 16128, 62720),
                legacy: blue,
            },
            "ORANGE" => Self {
                extended: rgb("Orange", 62464, 33024, 12544),
                legacy: red,
            },
            _ => legacy(red),
        }
    }
}

/// `getpass.getuser()` as far as it goes without the password database:
/// the first of the variables it reads that is set and not empty.
fn login_name() -> Option<String> {
    ["LOGNAME", "USER", "LNAME", "USERNAME"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
}

impl FileTranscriber<'_> {
    /// `transcribe_aaf_descriptive_markers`: the markers on a track and on
    /// everything in it, onto one event slot of the composition.
    pub(crate) fn transcribe_aaf_descriptive_markers(&mut self, t: &Track) -> Result<(), Fail> {
        let markers_of = |id: NodeId| -> Result<Vec<NodeId>, Fail> {
            Ok(self
                .document
                .try_get(id)?
                .item()
                .map(|item| item.markers.clone())
                .unwrap_or_default())
        };
        let mut markers = Vec::new();
        for marker in markers_of(t.id)? {
            markers.push((marker, t.id));
        }
        for child in find_children(self.document, t.id)? {
            for marker in markers_of(child)? {
                markers.push((marker, child));
            }
        }
        if markers.is_empty() {
            return Ok(());
        }

        // One event slot per marked track, numbered from 1000 so that it
        // stays clear of the composition's other slots.
        let event_mob_slot = self.f.create("EventMobSlot")?;
        self.f.set(event_mob_slot, "EditRate", t.edit_rate)?;
        let mut existing = Vec::new();
        for slot in self.f.get_objects(self.composition_mob, "Slots")? {
            existing.push(self.slot_id(slot)?);
        }
        let mut event_slot_id = 1000;
        while existing.contains(&event_slot_id) {
            event_slot_id += 1;
        }
        self.f.set(event_mob_slot, "SlotID", event_slot_id)?;
        let number = self.physical_track_number(t)?;
        self.f.set(event_mob_slot, "PhysicalTrackNumber", number)?;

        let sequence = self.f.create_sequence("DescriptiveMetadata")?;
        let mut transcribed = Vec::new();
        for (marker, parent) in markers {
            transcribed.push(self.transcribe_marker(t, marker, parent)?);
        }
        // By increasing position; a stable sort, as Python's is.
        transcribed.sort_by_key(|&(_, position)| position);
        for (marker, _) in transcribed {
            self.f.append(sequence, "Components", marker)?;
        }

        self.f.set(event_mob_slot, "Segment", sequence)?;
        self.f
            .append(self.composition_mob, "Slots", event_mob_slot)?;
        Ok(())
    }

    /// `_transcribe_marker`: one marker, and its position on the track.
    ///
    /// Dates and the user come from the marker's metadata when it has them,
    /// so a marker read from an AAF keeps them; a new marker is dated now,
    /// by the writer's clock, which is read once per marker whether or not
    /// it is needed, as upstream reads it.
    fn transcribe_marker(
        &mut self,
        t: &Track,
        marker: NodeId,
        parent: NodeId,
    ) -> Result<(ObjRef, i64), Fail> {
        let Node::Marker(otio_marker) = self.document.try_get(marker)? else {
            return Err(unwritable("a marker that is not a marker".to_owned()));
        };
        let name = otio_marker.base.name.clone();
        let marked_range = otio_marker.marked_range;
        let color_name = otio_marker
            .color
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_default();

        let marker_metadata = aaf_metadata(&otio_marker.base.metadata)?;
        let prev_attrs = marker_metadata.get_dict("CommentMarkerAttributeList")?;
        let prev_comments = marker_metadata.get_dict("UserComments")?;

        // Avid spells the property "CommentMarkerUSer", and that is the key
        // the reader keeps it under.
        let username = match first_truthy([
            marker_metadata.get("CommentMarkerUSer"),
            prev_attrs.get("_ATN_CRM_USER"),
        ]) {
            Some(user) => user.clone(),
            None => Any::String(self.user()?),
        };

        let marker_color = MarkerColor::for_name(&color_name);

        // Media Composer dates a marker by these two integers, not by the
        // strings below.
        let time_now = self.f.now();
        let create_date = match first_truthy([prev_attrs.get("_ATN_CRM_LONG_CREATE_DATE")]) {
            Some(date) => py::py_int(date)?,
            None => time_now.to_unix(),
        };
        let mod_date = match first_truthy([prev_attrs.get("_ATN_CRM_LONG_MOD_DATE")]) {
            Some(date) => py::py_int(date)?,
            None => create_date,
        };
        let date_str = first_truthy([
            marker_metadata.get("CommentMarkerDate"),
            prev_attrs.get("_ATN_CRM_DATE"),
        ])
        .cloned()
        .unwrap_or_else(|| {
            Any::String(format!(
                "{:02}/{:02}/{:04}",
                time_now.month, time_now.day, time_now.year
            ))
        });
        let time_str = first_truthy([
            marker_metadata.get("CommentMarkerTime"),
            prev_attrs.get("_ATN_CRM_TIME"),
        ])
        .cloned()
        .unwrap_or_else(|| Any::String(format!("{:02}:{:02}", time_now.hour, time_now.minute)));

        let range_in_track = self
            .document
            .transformed_time_range(marked_range, parent, t.id)?;

        let aaf_marker = self.f.create("DescriptiveMarker")?;
        // Media Composer ignores a marker's length, but the reader takes it
        // for the marker's duration, so it is set for the round trip.
        self.f.set(
            aaf_marker,
            "Length",
            py::float_int(marked_range.duration().value())?,
        )?;
        // The slot the marker annotates, which the reader matches with the
        // event slot's physical track number to find the track again.
        let slot_id = self.slot_id(t.timeline_mobslot)?;
        self.f
            .set(aaf_marker, "DescribedSlots", WriteValue::array([slot_id]))?;
        let position = py::float_int(range_in_track.start_time().value())?;
        self.f.set(aaf_marker, "Position", position)?;
        self.f.set(aaf_marker, "Comment", name.as_str())?;
        self.set_py(aaf_marker, "CommentMarkerUser", &username)?;
        self.f.set(
            aaf_marker,
            "CommentMarkerColor",
            marker_color.legacy.record(),
        )?;
        self.f.set(
            aaf_marker,
            "CommentMarkerColorExtended",
            marker_color.extended.record(),
        )?;
        self.set_py(aaf_marker, "CommentMarkerTime", &time_str)?;
        self.set_py(aaf_marker, "CommentMarkerDate", &date_str)?;

        let tagged = |value: &Any| -> Result<WriteValue, Fail> {
            py::to_write_value(value)?
                .ok_or_else(|| unwritable("None as a tagged value".to_owned()))
        };
        let attrs: [(&str, WriteValue); 9] = [
            ("_ATN_CRM_COM", name.as_str().into()),
            ("_ATN_CRM_USER", tagged(&username)?),
            ("_ATN_CRM_DATE", tagged(&date_str)?),
            ("_ATN_CRM_TIME", tagged(&time_str)?),
            ("_ATN_CRM_COLOR", marker_color.legacy.name.into()),
            ("_ATN_CRM_COLOR_EXTENDED", marker_color.extended.name.into()),
            ("_ATN_CRM_MARKNAME", name.as_str().into()),
            ("_ATN_CRM_LONG_CREATE_DATE", create_date.into()),
            ("_ATN_CRM_LONG_MOD_DATE", mod_date.into()),
        ];
        for (key, value) in attrs {
            self.f
                .set_tagged_value(aaf_marker, "CommentMarkerAttributeList", key, value)?;
        }

        // Media Composer's own identifier for the marker, kept when the
        // marker came from it and never made up.
        let database_id = first_truthy([
            prev_comments.get("DatabaseID"),
            prev_attrs.get("_ATN_CRM_ID"),
        ])
        .cloned();
        if let Some(id) = &database_id {
            self.f.set_tagged_value(
                aaf_marker,
                "CommentMarkerAttributeList",
                "_ATN_CRM_ID",
                tagged(id)?,
            )?;
        }
        self.f
            .set_tagged_value(aaf_marker, "UserComments", "Comment", name.as_str())?;
        if let Some(id) = &database_id {
            self.f
                .set_tagged_value(aaf_marker, "UserComments", "DatabaseID", tagged(id)?)?;
        }

        Ok((aaf_marker, position))
    }

    /// The user a new marker is credited to: the one the options name, or
    /// the one `getpass.getuser()` would find.
    fn user(&self) -> Result<String, Fail> {
        if let Some(user) = &self.options.user {
            return Ok(user.clone());
        }
        login_name().ok_or_else(|| {
            unwritable(
                "no user to credit a new marker to: set WriteOptions::user, or LOGNAME or USER"
                    .to_owned(),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::MarkerColor;

    #[test]
    fn extended_colours_fall_back_to_legacy_ones() {
        let orange = MarkerColor::for_name("orange");
        assert_eq!(orange.extended.name, "Orange");
        assert_eq!(orange.legacy.name, "Red");
        assert_eq!(MarkerColor::for_name("").legacy.name, "Red");
        assert_eq!(MarkerColor::for_name("Cyan").extended.name, "Cyan");
    }
}
