using System;
using OpenTimelineIO;

var timeline = new Timeline("Cut");
var stack = new Stack("tracks");
var track = new Track("V1", "Video");

timeline.SetTracks(stack);
stack.AppendChild(track);

var oneSecond = new TimeRange(new RationalTime(0, 24), new RationalTime(24, 24));

// An AAF clip is cut from media of a known length, so each clip's media says
// how much of it there is. A new clip has no media at all, so its reference
// goes in under upstream's key and is made the active one.
foreach (var name in new[] { "A001C003", "A001C004" })
{
    var media = new ExternalReference(targetUrl: $"file:///media/{name}.mov");
    media.SetAvailableRange(oneSecond);

    var clip = new Clip(name);
    clip.SetMediaReference("DEFAULT_MEDIA", media);
    clip.SetActiveMediaReferenceKey("DEFAULT_MEDIA");
    clip.SetSourceRange(oneSecond);
    track.AppendChild(clip);
}

// Every clip needs a MobID, from its metadata, its media's metadata or the
// AAF its media names. A cut built from scratch has none, so let the writer
// make them up rather than refuse the clip.
Otio.WriteToFile(Format.Aaf, timeline, "cut.aaf", new WriteOptions(aafUseEmptyMobIds: true));

foreach (var node in Otio.ReadFromFile(Format.Aaf, "cut.aaf", null).FindClips())
{
    if (node is Clip clip)
    {
        Console.WriteLine(clip.Name());
    }
}
