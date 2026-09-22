using System;
using OpenTimelineIO;

// An EDL never says what rate its timecode is at, so this has to be
// right: a file read at the wrong rate puts every event in the wrong
// place rather than failing.
// A value here is a `readonly struct`, as one in .NET should be, so the
// rate is chosen when the options are made rather than set afterwards.
var defaults = Document.ReadOptionsDefault();
var options = new ReadOptions(24, defaults.NameColumn, defaults.IgnoreTimecodeMismatch);

using var document = Document.ReadFromFile(Format.Cmx3600, "cut.edl", options);

var root = document.Root();
if (root is not null)
{
    foreach (var node in root.FindClips())
    {
        if (node is Clip clip)
        {
            Console.WriteLine(clip.Name());
        }
    }
}
