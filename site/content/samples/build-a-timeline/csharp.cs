using System;
using OpenTimelineIO;

// Each object is made on its own and joins a timeline when you put it
// into one. Nothing has to exist before the thing it goes into.
var timeline = new Timeline("Cut");
var stack = new Stack("tracks");
var track = new Track("V1", "Video");

timeline.SetTracks(stack);
stack.AppendChild(track);

var names = new[] { "A", "B", "C" };
for (var index = 0; index < names.Length; index++)
{
    var clip = new Clip(names[index]);
    var start = new RationalTime(index * 24, 24);
    clip.SetSourceRange(new TimeRange(start, new RationalTime(24, 24)));
    track.AppendChild(clip);
}

// Three seconds of picture, written as canonical OpenTimelineIO JSON.
// The objects keep their timeline alive between them, so there is
// nothing to dispose.
Console.WriteLine(track.Duration().ToSeconds());
Otio.Save(timeline, "cut.otio");
