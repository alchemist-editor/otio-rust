use opentime::{RationalTime, TimeRange};
use otio_core::schema::{Base, Clip, ItemData, Stack, Timeline, Track};
use otio_core::{Document, Node};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A document is an arena. Objects go into it and are named by handles,
    // so nothing here is a pointer into anything.
    let mut document = Document::new();

    let tracks = document.insert(Node::Stack(Stack::default()));
    let timeline = document.insert(Node::Timeline(Timeline {
        base: Base {
            name: "Cut".into(),
            ..Base::default()
        },
        tracks: Some(tracks),
        global_start_time: None,
    }));
    document.set_root(Some(timeline));

    let track = document.insert(Node::Track(Track {
        kind: "Video".into(),
        ..Track::default()
    }));
    document.append_child(tracks, track)?;

    for (index, name) in ["A", "B", "C"].into_iter().enumerate() {
        let clip = document.insert(Node::Clip(Clip {
            item: ItemData {
                base: Base {
                    name: name.into(),
                    ..Base::default()
                },
                source_range: Some(TimeRange::new(
                    RationalTime::new(index as f64 * 24.0, 24.0),
                    RationalTime::new(24.0, 24.0),
                )),
                ..ItemData::new()
            },
            ..Clip::default()
        }));
        document.append_child(track, clip)?;
    }

    // Three seconds of picture, written as canonical OpenTimelineIO JSON.
    println!("{}", document.duration(track)?.to_seconds());
    std::fs::write("cut.otio", otio_core::to_string_pretty(&document)?)?;

    Ok(())
}
