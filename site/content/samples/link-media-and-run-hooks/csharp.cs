using System;
using System.Text;
using OpenTimelineIO;

// A cut of two clips, as an .otio file would hold it.
const string cut = """
{
  "OTIO_SCHEMA": "Track.1",
  "name": "V1",
  "kind": "Video",
  "children": [
    {"OTIO_SCHEMA": "Clip.2", "name": "A"},
    {"OTIO_SCHEMA": "Clip.2", "name": "B"}
  ]
}
""";

// A media linker is handed each clip as it is read, with the arguments
// the read was given, and answers with the media the clip should use.
// Answering null leaves the clip as it was.
Otio.RegisterMediaLinker("proxies", (clip, arguments) =>
{
    var root = arguments.GetString("root");
    return new ExternalReference(clip.Name(), $"{root}/{clip.Name()}.mov");
});

// A hook script is handed the whole result, and answers with what the
// read goes on with: here the same object, stamped.
Otio.RegisterHookScript("stamp", (target, arguments) =>
{
    var stamped = (SerializableObjectWithMetadata)target;
    stamped.Metadata.SetString("read_by", arguments.GetString("who"));
    return target;
});
Otio.AttachHookScript("post_adapter_read", "stamp");

try
{
    // The read names the linker, and carries both sets of arguments as JSON.
    var track = (Track)Otio.ReadFromBytes(
        Format.OtioJson,
        Encoding.UTF8.GetBytes(cut),
        new ReadOptions(
            mediaLinker: "proxies",
            mediaLinkerArguments: """{"root": "/proxies"}""",
            hookArguments: """{"who": "the conform"}"""));

    Console.WriteLine($"read by {track.Metadata.GetString("read_by")}");
    foreach (var node in track.FindClips())
    {
        if (node is Clip clip && clip.MediaReference(null) is ExternalReference media)
        {
            Console.WriteLine($"{clip.Name()} -> {media.TargetUrl()}");
        }
    }
}
finally
{
    Otio.DetachHookScript("post_adapter_read", "stamp");
    Otio.UnregisterHookScript("stamp");
    Otio.UnregisterMediaLinker("proxies");
}
