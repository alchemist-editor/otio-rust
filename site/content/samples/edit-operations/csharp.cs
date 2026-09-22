using System;
using System.Linq;
using OpenTimelineIO;

// One second of picture, named.
static Clip Second(string name)
{
    var clip = new Clip(name);
    clip.SetSourceRange(new TimeRange(new RationalTime(0, 24), new RationalTime(24, 24)));
    return clip;
}

static void Show(Track track)
{
    var names = string.Join(" ", track.Children().Select(child => child.Name()));
    Console.WriteLine($"{names} - {track.Duration().Value} frames");
}

var track = new Track("V1", "Video");
foreach (var name in new[] { "A", "B", "C" })
{
    track.AppendChild(Second(name));
}
Show(track);

// Insert makes room: everything from the insertion point onwards moves later,
// and the track gets longer.
Otio.Insert(Second("D"), track, new RationalTime(24, 24), false, null);
Show(track);

// Overwrite does not: it lays an item over a span and whatever was in that
// span gives way. The track is the same length afterwards.
Otio.Overwrite(
    Second("E"),
    track,
    new TimeRange(new RationalTime(48, 24), new RationalTime(24, 24)),
    false,
    null);
Show(track);
