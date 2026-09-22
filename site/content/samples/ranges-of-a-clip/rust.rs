use opentime::{RationalTime, TimeRange};
use otio_core::schema::{
    Base, Clip, ExternalReference, Gap, ItemData, MediaReferenceData, Track,
};
use otio_core::{Document, Node};
use std::collections::BTreeMap;

fn frames(range: TimeRange) -> String {
    format!(
        "{} for {}",
        range.start_time().value(),
        range.duration().value()
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::new();

    // Ten seconds of rushes on disk. `available_range` belongs to the media,
    // not to the clip: it is what the file offers, whoever uses it.
    let media = document.insert(Node::ExternalReference(ExternalReference {
        media: MediaReferenceData {
            available_range: Some(TimeRange::new(
                RationalTime::new(0.0, 24.0),
                RationalTime::new(240.0, 24.0),
            )),
            ..MediaReferenceData::default()
        },
        target_url: "file:///A001.mov".into(),
    }));

    // Three seconds of it, starting two seconds in. A source range is in the
    // media's clock, which is why it starts at 48 rather than at 0.
    let clip = document.insert(Node::Clip(Clip {
        item: ItemData {
            base: Base {
                name: "shot".into(),
                ..Base::default()
            },
            source_range: Some(TimeRange::new(
                RationalTime::new(48.0, 24.0),
                RationalTime::new(72.0, 24.0),
            )),
            ..ItemData::new()
        },
        media_references: BTreeMap::from([("DEFAULT_MEDIA".to_string(), media)]),
        active_media_reference_key: "DEFAULT_MEDIA".into(),
    }));

    // A second of black in front of it, so the clip does not start the track.
    let head = document.insert(Node::Gap(Gap {
        item: ItemData {
            source_range: Some(TimeRange::new(
                RationalTime::new(0.0, 24.0),
                RationalTime::new(24.0, 24.0),
            )),
            ..ItemData::new()
        },
    }));

    let track = document.insert(Node::Track(Track {
        kind: "Video".into(),
        ..Track::default()
    }));
    document.append_child(track, head)?;
    document.append_child(track, clip)?;

    // The same clip, asked four questions. The first three answer in the
    // media's clock; the last answers in the track's.
    println!("available: {}", frames(document.available_range(clip)?));
    println!("trimmed:   {}", frames(document.trimmed_range(clip)?));
    println!("visible:   {}", frames(document.visible_range(clip)?));
    println!("in parent: {}", frames(document.range_in_parent(clip)?));

    Ok(())
}
