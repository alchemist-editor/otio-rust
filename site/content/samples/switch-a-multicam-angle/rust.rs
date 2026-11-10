use otio_core::multicam::{self, Cut};

/// Two cameras on one interview, and an edit with two cuts on camera A.
const EDIT: &str = r#"{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "interview",
  "multicams": {
    "mc1": {"OTIO_SCHEMA": "Multicam.1", "name": "Interview", "children": [
      {"OTIO_SCHEMA": "Stack.1", "name": "A", "children": [
        {"OTIO_SCHEMA": "Track.1", "name": "A V", "kind": "Video", "children": [
          {"OTIO_SCHEMA": "Clip.2", "name": "A_take",
           "source_range": {"OTIO_SCHEMA": "TimeRange.1",
             "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 0, "rate": 24},
             "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 240, "rate": 24}}}]}]},
      {"OTIO_SCHEMA": "Stack.1", "name": "B", "children": [
        {"OTIO_SCHEMA": "Track.1", "name": "B V", "kind": "Video", "children": [
          {"OTIO_SCHEMA": "Clip.2", "name": "B_take",
           "source_range": {"OTIO_SCHEMA": "TimeRange.1",
             "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 0, "rate": 24},
             "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 240, "rate": 24}}}]}]}]}
  },
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "MulticamClip.1", "name": "cut 1", "multicam": "mc1", "angle": "A",
       "source_range": {"OTIO_SCHEMA": "TimeRange.1",
         "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 0, "rate": 24},
         "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 48, "rate": 24}}},
      {"OTIO_SCHEMA": "MulticamClip.1", "name": "cut 2", "multicam": "mc1", "angle": "A",
       "source_range": {"OTIO_SCHEMA": "TimeRange.1",
         "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 48, "rate": 24},
         "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 48, "rate": 24}}}]}]}
}"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut document = otio_core::from_str(EDIT)?;
    let timeline = document.root().ok_or("no timeline")?;
    let cuts = document.find_children(timeline, None, false, &multicam::is_multicam_clip)?;

    // Switch the second cut to camera B. It keeps its place and its range.
    let second = cuts[1];
    let cut = Cut {
        angle: "B".into(),
        ..multicam::cut(document.try_get(second)?).ok_or("not a cut")?
    };
    multicam::set_cut(&mut document, second, &cut)?;

    // What each cut plays: a track of one angle.
    for &id in &cuts {
        let played = multicam::resolve(&document, timeline, id, None)?;
        println!("{} plays {}", document.try_get(id)?.name(), document.try_get(played.track)?.name());
    }
    assert!(multicam::validate(&document, timeline)?.is_empty());
    std::fs::write("edit.otio", otio_core::to_string(&document)?)?;

    // For a reader without multicam: each cut becomes the material it plays.
    let lost = multicam::flatten(&mut document, timeline)?;
    println!("flattening lost {} things", lost.len());
    std::fs::write("flat.otio", otio_core::to_string(&document)?)?;
    Ok(())
}
