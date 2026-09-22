use opentime::{RationalTime, TimeRange};
use otio_core::schema::{Base, Clip, ItemData, Track};
use otio_core::{Document, Node, NodeId};

/// One second of picture, named.
fn second(document: &mut Document, name: &str) -> NodeId {
    document.insert(Node::Clip(Clip {
        item: ItemData {
            base: Base {
                name: name.into(),
                ..Base::default()
            },
            source_range: Some(TimeRange::new(
                RationalTime::new(0.0, 24.0),
                RationalTime::new(24.0, 24.0),
            )),
            ..ItemData::new()
        },
        ..Clip::default()
    }))
}

fn show(document: &Document, track: NodeId) -> Result<(), otio_core::Error> {
    let names: Vec<&str> = document
        .children_of(track)?
        .iter()
        .map(|id| document.try_get(*id).map(Node::name).unwrap_or(""))
        .collect();
    println!("{} - {} frames", names.join(" "), document.duration(track)?.value());
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::new();
    let track = document.insert(Node::Track(Track {
        kind: "Video".into(),
        ..Track::default()
    }));
    for name in ["A", "B", "C"] {
        let clip = second(&mut document, name);
        document.append_child(track, clip)?;
    }
    show(&document, track)?;

    // Insert makes room: everything from the insertion point onwards moves
    // later, and the track gets longer.
    let inserted = second(&mut document, "D");
    otio_core::edit::insert(
        &mut document,
        inserted,
        track,
        RationalTime::new(24.0, 24.0),
        false,
        None,
    )?;
    show(&document, track)?;

    // Overwrite does not: it lays an item over a span and whatever was in
    // that span gives way. The track is the same length afterwards.
    let laid = second(&mut document, "E");
    otio_core::edit::overwrite(
        &mut document,
        laid,
        track,
        TimeRange::new(RationalTime::new(48.0, 24.0), RationalTime::new(24.0, 24.0)),
        false,
        None,
    )?;
    show(&document, track)?;

    Ok(())
}
