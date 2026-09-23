using System;
using OpenTimelineIO;

var timeline = new Timeline("Cut");
var track = new Track("V1", "Video");
((Stack)timeline.Tracks()!).AppendChild(track);

// A cut of two clips: one whose media is a file beside the program, and one
// whose media is on the web.
foreach (var (name, url) in new[]
{
    ("A001C003", "shot.mov"),
    ("A001C004", "https://example.com/remote.mov"),
})
{
    var clip = new Clip(name);
    clip.SetMediaReference("DEFAULT_MEDIA", new ExternalReference(targetUrl: url));
    clip.SetActiveMediaReferenceKey("DEFAULT_MEDIA");
    track.AppendChild(clip);
}

// Every clip whose media is a file has the file copied into the bundle and
// its reference pointed at the copy. Media that is not a file would stop the
// write, so it is made missing instead. Format.Otiod writes the same layout
// as a directory.
Otio.WriteToFile(
    Format.Otioz,
    timeline,
    "cut.otioz",
    new WriteOptions(bundleMediaPolicy: BundleMediaPolicy.MissingIfNotFile));

// Unpacked, with each reference made absolute, the media is ready to use.
var bundled = Otio.ReadFromFile(
    Format.Otioz,
    "cut.otioz",
    new ReadOptions(bundleExtractPath: "cut", bundleAbsoluteMediaPaths: true));
foreach (var node in bundled.FindClips())
{
    if (node is Clip clip)
    {
        Console.WriteLine(
            clip.MediaReference(null) is ExternalReference media ? media.TargetUrl() : "missing");
    }
}
