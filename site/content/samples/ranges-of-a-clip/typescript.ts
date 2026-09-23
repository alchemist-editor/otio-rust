import {
  init,
  Clip,
  ExternalReference,
  Gap,
  RationalTime,
  TimeRange,
  Track,
} from "@alchemist-edit/otio";

await init();

const frames = (span: TimeRange) =>
  `${span.startTime.value} for ${span.duration.value}`;

// Ten seconds of rushes on disk. `availableRange` belongs to the media, not
// to the clip: it is what the file offers, whoever uses it.
const media = new ExternalReference({ targetUrl: "file:///A001.mov" });
media.availableRange = new TimeRange(
  new RationalTime(0, 24),
  new RationalTime(240, 24),
);

// Three seconds of it, starting two seconds in. A source range is in the
// media's clock, which is why it starts at 48 rather than at 0.
const clip = new Clip({ name: "shot" });
clip.setMediaReference("DEFAULT_MEDIA", media);
clip.sourceRange = new TimeRange(
  new RationalTime(48, 24),
  new RationalTime(72, 24),
);

const track = new Track({ name: "V1", kind: "Video" });
// A second of black in front of it, so the clip does not start the track.
const head = new Gap({});
head.sourceRange = new TimeRange(
  new RationalTime(0, 24),
  new RationalTime(24, 24),
);
track.appendChild(head);
track.appendChild(clip);

// The same clip, asked four questions. The first three answer in the media's
// clock; the last answers in the track's.
console.log("available:", frames(clip.availableRange()));
console.log("trimmed:  ", frames(clip.trimmedRange()));
console.log("visible:  ", frames(clip.visibleRange()));
console.log("in parent:", frames(clip.rangeInParent()));
