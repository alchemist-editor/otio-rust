use otio_ext_sync::{Snapshot, diff, patch};

// Two versions of one sequence, as Furnace sends them: in the second, clip B
// has moved in front of clip A. Every object carries its GUID in
// `metadata.sync.guid`, which is how a patch names it.
const BEFORE: &str = r#"{"sequenceId": "main", "generation": 1, "otioJson": {
  "OTIO_SCHEMA": "Timeline.1", "name": "cut", "metadata": {"sync": {"guid": "cut"}},
  "tracks": {"OTIO_SCHEMA": "Stack.1", "metadata": {"sync": {"guid": "tracks"}}, "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "metadata": {"sync": {"guid": "v1"}},
     "children": [
       {"OTIO_SCHEMA": "Clip.2", "name": "A", "metadata": {"sync": {"guid": "a"}}},
       {"OTIO_SCHEMA": "Clip.2", "name": "B", "metadata": {"sync": {"guid": "b"}}}]}]}}}"#;

const AFTER: &str = r#"{"sequenceId": "main", "generation": 2, "otioJson": {
  "OTIO_SCHEMA": "Timeline.1", "name": "cut", "metadata": {"sync": {"guid": "cut"}},
  "tracks": {"OTIO_SCHEMA": "Stack.1", "metadata": {"sync": {"guid": "tracks"}}, "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "metadata": {"sync": {"guid": "v1"}},
     "children": [
       {"OTIO_SCHEMA": "Clip.2", "name": "B", "metadata": {"sync": {"guid": "b"}}},
       {"OTIO_SCHEMA": "Clip.2", "name": "A", "metadata": {"sync": {"guid": "a"}}}]}]}}}"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    otio_ext_sync::install();
    otio_ext::enable(otio_ext_sync::ID)?;

    let before = Snapshot::from_json(BEFORE)?;
    let after = Snapshot::from_json(AFTER)?;
    let (patches, stats) = diff(&before, &after)?;

    // One move, of A to after B: a patch names objects by GUID.
    println!("{}", patch::to_json_pretty(&patches));
    println!("fell back to the whole timeline: {}", stats.fell_back);
    Ok(())
}
