use otio_ext_sync::document::{self, Transaction};

// A cut, as an .otio file would hold it. Nothing in it has a GUID yet.
const CUT: &str = r#"{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "cut",
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "Clip.2", "name": "A"},
      {"OTIO_SCHEMA": "Clip.2", "name": "B"}]}]}
}"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    otio_ext_sync::install();
    otio_ext::enable(otio_ext_sync::ID)?;

    // Beginning a transaction gives every object a GUID, and remembers the
    // timeline as it was: that snapshot is what a second program starts
    // from.
    let mut editor = otio_core::from_str(CUT)?;
    let timeline = editor.root().ok_or("the cut has no root")?;
    let transaction = Transaction::begin(&mut editor, timeline, "main", 1)?;
    let mut viewer = otio_core::from_str(&transaction.before().timeline)?;
    let shown = viewer.root().ok_or("the snapshot has no root")?;

    // Edit as usual, then commit for the patches that say what changed.
    if let Some(base) = editor.try_get_mut(timeline)?.base_mut() {
        base.name = "fine cut".into();
    }
    let patches = transaction.commit(&mut editor)?;
    println!("{}", otio_ext_sync::patch::to_json(&patches));

    // The other copy applies them in place: what it held still names the
    // same objects.
    document::apply_to(&mut viewer, shown, &patches)?;
    let name = viewer.try_get(shown)?.base().map(|base| base.name.clone());
    println!("the viewer now shows {}", name.unwrap_or_default());
    Ok(())
}
