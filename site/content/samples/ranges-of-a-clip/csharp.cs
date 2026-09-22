using System;
using OpenTimelineIO;

static string Frames(TimeRange span) => $"{span.StartTime.Value} for {span.Duration.Value}";

// Ten seconds of rushes on disk. The available range belongs to the media,
// not to the clip: it is what the file offers, whoever uses it.
var media = new ExternalReference("A001", "file:///A001.mov");
media.SetAvailableRange(new TimeRange(new RationalTime(0, 24), new RationalTime(240, 24)));

// Three seconds of it, starting two seconds in. A source range is in the
// media's clock, which is why it starts at 48 rather than at 0.
var clip = new Clip("shot");
clip.SetMediaReference("DEFAULT_MEDIA", media);
clip.SetSourceRange(new TimeRange(new RationalTime(48, 24), new RationalTime(72, 24)));

// A second of black in front of it, so the clip does not start the track.
var head = new Gap();
head.SetSourceRange(new TimeRange(new RationalTime(0, 24), new RationalTime(24, 24)));

var track = new Track("V1", "Video");
track.AppendChild(head);
track.AppendChild(clip);

// The same clip, asked four questions. The first three answer in the media's
// clock; the last answers in the track's.
Console.WriteLine("available: " + Frames(clip.AvailableRange()));
Console.WriteLine("trimmed:   " + Frames(clip.TrimmedRange()));
Console.WriteLine("visible:   " + Frames(clip.VisibleRange()));
Console.WriteLine("in parent: " + Frames(clip.RangeInParent()));
