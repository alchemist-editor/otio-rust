using OpenTimelineIO;

// An EDL never says what rate its timecode is at, so this has to be right: a
// file read at the wrong rate puts every event in the wrong place rather
// than failing.
// A value here is a `readonly struct`, as one in .NET should be, so the rate
// is chosen when the options are made rather than set afterwards.
var defaults = Otio.ReadOptionsDefault();
var options = new ReadOptions(24, defaults.NameColumn, defaults.IgnoreTimecodeMismatch);

var timeline = Otio.ReadFromFile(Format.Cmx3600, "cut.edl", options);

// Nothing happens in between. The timeline an EDL parses to is the same
// timeline FCP X writes out, so converting is a read and a write: the object
// model is the interchange, and the file formats are two ways of spelling it.
Otio.WriteToFile(Format.FcpxXml, timeline, "cut.fcpxml", null);
