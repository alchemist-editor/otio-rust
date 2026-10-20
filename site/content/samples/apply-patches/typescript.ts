import { init, Timeline, deserializeJsonFromString, serializeJsonToString } from "@alchemist-edit/otio";
import * as sync from "@alchemist-edit/otio/ext/sync";

// A cut, as an .otio file would hold it. Nothing in it has a GUID yet.
const CUT = `{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "cut",
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "Clip.2", "name": "A"},
      {"OTIO_SCHEMA": "Clip.2", "name": "B"}]}]}
}`;

const asTimeline = (json: string): Timeline => {
  const node = deserializeJsonFromString(json);
  if (!(node instanceof Timeline)) {
    throw new Error("not a timeline");
  }
  return node;
};

await init();
sync.enable();

// Beginning a transaction gives every object a GUID, and remembers the
// timeline as it was: that snapshot is the transaction. A second program
// starts from the same stamped timeline.
const editor = asTimeline(CUT);
const before = sync.begin(editor, "main", 1);
const viewer = asTimeline(serializeJsonToString(editor));

// Edit as usual, then commit for the patches that say what changed.
editor.name = "fine cut";
const patches = sync.commit(editor, before);
console.log(patches);

// The other copy applies them in place: what it held still names the
// same objects.
sync.applyTo(viewer, patches);
console.log(`the viewer now shows ${viewer.name}`);
