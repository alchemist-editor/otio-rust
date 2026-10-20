import { init } from "@alchemist-edit/otio";
import * as sync from "@alchemist-edit/otio/ext/sync";

// Two versions of one sequence, as Furnace sends them: in the second, clip B
// has moved in front of clip A. Every object carries its GUID in
// `metadata.sync.guid`, which is how a patch names it.
const clip = (name: string, guid: string) => ({
  OTIO_SCHEMA: "Clip.2",
  name,
  metadata: { sync: { guid } },
});
const snapshot = (generation: number, children: object[]) =>
  JSON.stringify({
    sequenceId: "main",
    generation,
    otioJson: {
      OTIO_SCHEMA: "Timeline.1",
      name: "cut",
      metadata: { sync: { guid: "cut" } },
      tracks: {
        OTIO_SCHEMA: "Stack.1",
        metadata: { sync: { guid: "tracks" } },
        children: [
          { OTIO_SCHEMA: "Track.1", name: "V1", kind: "Video", metadata: { sync: { guid: "v1" } }, children },
        ],
      },
    },
  });

const before = snapshot(1, [clip("A", "a"), clip("B", "b")]);
const after = snapshot(2, [clip("B", "b"), clip("A", "a")]);

await init();
sync.enable();

// One move, of A to after B: a patch names objects by GUID.
console.log(sync.diff(before, after, true));
const stats = JSON.parse(sync.diffStats(before, after)) as { fellBack: boolean };
console.log(`fell back to the whole timeline: ${stats.fellBack}`);
