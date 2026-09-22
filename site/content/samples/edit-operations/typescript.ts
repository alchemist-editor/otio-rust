import { init, Clip, edit, RationalTime, TimeRange, Track } from "@otio/otio";

await init();

/** One second of picture, named. */
function second(name: string): Clip {
  const clip = new Clip({ name });
  clip.sourceRange = new TimeRange(
    new RationalTime(0, 24),
    new RationalTime(24, 24),
  );
  return clip;
}

function show(track: Track) {
  const names = track.children().map((child) => child.name).join(" ");
  console.log(names, "-", track.duration().value, "frames");
}

const track = new Track({ name: "V1", kind: "Video" });
for (const name of ["A", "B", "C"]) track.appendChild(second(name));
show(track);

// Insert makes room: everything from the insertion point onwards moves later,
// and the track gets longer.
edit.insert(second("D"), track, new RationalTime(24, 24), false);
show(track);

// Overwrite does not: it lays an item over a span and whatever was in that
// span gives way. The track is the same length afterwards.
edit.overwrite(
  second("E"),
  track,
  new TimeRange(new RationalTime(48, 24), new RationalTime(24, 24)),
  false,
);
show(track);
