use opentime::RationalTime;
use otio_ext_ori_annotations::annotations;
use otio_ext_ori_annotations::command::{self, Command, PaintStart, TextAnnotation, Vertices};
use otio_ext_ori_annotations::coords::ImageSize;

// A cut with one shot, which sync knows by the GUID `shot`.
const CUT: &str = r#"{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "review",
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "Clip.2", "name": "shot",
       "metadata": {"sync": {"guid": "shot"}}}]}]}
}"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ORI Annotations is an extension; enabling it enables sync too.
    otio_ext_ori_annotations::install();
    otio_ext::enable(otio_ext_ori_annotations::ID)?;

    let mut document = otio_core::from_str(CUT)?;
    let timeline = document.root().ok_or("the cut has no root")?;

    // The Annotations track, and the clip on it for frame 10 of the shot.
    let track = annotations::ensure_track(&mut document, timeline)?;
    let frame = annotations::ensure_clip(&mut document, track, "shot", RationalTime::new(10.0, 24.0))?;

    // ORI's coordinates are height-normalised with the origin at the centre,
    // so convert from the pixels of the 1920x1080 picture.
    let hd = ImageSize::new(1920.0, 1080.0);
    let points: Vec<(f64, f64)> = [(200.0, 200.0), (960.0, 540.0), (1720.0, 880.0)]
        .into_iter()
        .map(|(x, y)| hd.pixels_to_ori(x, y))
        .collect();

    // A stroke is a PaintStart, its points and a PaintEnd sharing one uuid.
    let stroke = command::stroke(
        PaintStart {
            rgba: Some([1.0, 0.25, 0.25, 1.0]),
            participant_hash: Some("reviewer-1".into()),
            timestamp: Some(command::timestamp_now()),
            ..PaintStart::new(otio_ext_sync::guid::new_guid())
        },
        Vertices::with_size(&points, 0.01),
    );
    annotations::add_commands(&mut document, frame, &stroke)?;

    let (x, y) = hd.pixels_to_ori(960.0, 620.0);
    let label = Command::Text(TextAnnotation {
        text: Some("Eyeline is late".into()),
        position: Some([x, y]),
        rgba: Some([1.0, 1.0, 1.0, 1.0]),
        ..TextAnnotation::new(otio_ext_sync::guid::new_guid())
    });
    annotations::add_commands(&mut document, frame, &[label])?;

    // Read them back: every annotation on the shot, frame by frame.
    for annotation in annotations::for_clip(&document, timeline, "shot")? {
        println!("{:?}: {} commands", annotation.frame, annotation.commands.len());
    }

    // The file is plain OpenTimelineIO with ORI's schemas in it, as ORI's
    // own Python writes it.
    println!("{}", otio_core::to_string_pretty(&document, 4)?);
    Ok(())
}
