using System;
using OpenTimelineIO;

// An EDL never says what rate its timecode is at, so this has to be
// right: a file read at the wrong rate puts every event in the wrong
// place rather than failing.
// A value here is a `readonly struct`, as one in .NET should be, so the
// rate is chosen when the options are made rather than set afterwards.
var defaults = Otio.ReadOptionsDefault();
var options = new ReadOptions(24, defaults.NameColumn, defaults.IgnoreTimecodeMismatch);

// Reading hands back the object the file is about, which for an EDL is
// the timeline it describes.
var timeline = Otio.ReadFromFile(Format.Cmx3600, "cut.edl", options);

foreach (var node in timeline.FindClips())
{
    if (node is Clip clip)
    {
        Console.WriteLine(clip.Name());
    }
}
