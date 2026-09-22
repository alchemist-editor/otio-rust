using System;
using OpenTimelineIO;

// The document owns every object in it, and releasing it releases them
// all at once. `using` is what makes that happen at the end of this block.
using var document = Document.New();

var timeline = document.NewTimeline("Cut");
var stack = document.NewStack("tracks");
timeline.SetTracks(stack);
var track = document.NewTrack("V1", "Video");
stack.AppendChild(track);

var names = new[] { "A", "B", "C" };
for (var index = 0; index < names.Length; index++)
{
    var clip = document.NewClip(names[index]);
    var start = new RationalTime(index * 24, 24);
    clip.SetSourceRange(new TimeRange(start, new RationalTime(24, 24)));
    track.AppendChild(clip);
}

document.SetRoot(timeline);

// Three seconds of picture, written as canonical OpenTimelineIO JSON.
Console.WriteLine(track.Duration().ToSeconds());
document.Save("cut.otio");
