// Tests for media linkers and hook scripts written in C#.
//
// The registry is the library's, shared by the whole process, so every test
// here registers under names of its own and unregisters them when it ends.

using System;
using System.Runtime.CompilerServices;
using System.Text;
using OpenTimelineIO;

namespace OpenTimelineIO.Tests;

internal static partial class Program
{
    /// The message a call failed with, if it failed as a plugin failure.
    private static string? PluginFailure(Action body)
    {
        try
        {
            body();
        }
        catch (OtioException error) when (error.Status == Status.PluginError)
        {
            return error.Message;
        }
        return null;
    }

    /// A two-clip timeline written as OTIO JSON, each clip pointing at media
    /// under file:///media.
    private static byte[] PluginCut()
    {
        var timeline = new Timeline("Cut");
        var stack = new Stack("tracks");
        var track = new Track("V1", "Video");
        timeline.SetTracks(stack);
        stack.AppendChild(track);
        foreach (var name in new[] { "first", "second" })
        {
            var clip = new Clip(name);
            clip.SetMediaReference("DEFAULT_MEDIA", new ExternalReference(name, $"file:///media/{name}.mov"));
            clip.SetActiveMediaReferenceKey("DEFAULT_MEDIA");
            track.AppendChild(clip);
        }
        return Otio.WriteToBytes(Format.OtioJson, timeline, null);
    }

    /// The target URL of the first clip's active media.
    private static string FirstUrl(SerializableObject root)
    {
        var clips = root.FindClips();
        if (clips.Length == 0 || clips[0] is not Clip clip)
        {
            throw new InvalidOperationException("there is no first clip");
        }
        if (clip.MediaReference(null) is not ExternalReference external)
        {
            throw new InvalidOperationException("the first clip's media is not an external reference");
        }
        return external.TargetUrl();
    }

    /// A hook script that writes who ran it into the metadata of what it is
    /// handed, under a key.
    private static Func<SerializableObject, Metadata, SerializableObject> Stamp(string key) =>
        (target, arguments) =>
        {
            var who = arguments.Contains("who") ? arguments.GetString("who") : "nobody";
            ((SerializableObjectWithMetadata)target).Metadata.SetString(key, who);
            return target;
        };

    private static void AMediaLinkerWrittenInCSharpLinksEveryClip()
    {
        var written = PluginCut();
        Otio.RegisterMediaLinker("csharp_proxies", (clip, arguments) =>
        {
            var root = arguments.GetString("root");
            return new ExternalReference("proxy", $"{root}/{clip.Name()}.mov");
        });
        try
        {
            var root = Otio.ReadFromBytes(
                Format.OtioJson,
                written,
                new ReadOptions(mediaLinker: "csharp_proxies", mediaLinkerArguments: "{\"root\": \"/proxies\"}"));
            CheckEq(FirstUrl(root), "/proxies/first.mov", "the first clip's media");
            var second = (Clip)root.FindClips()[1];
            CheckEq(
                ((ExternalReference)second.MediaReference(null)!).TargetUrl(),
                "/proxies/second.mov",
                "the second clip's media");

            // Asked not to link, it does not.
            var unlinked = Otio.ReadFromBytes(
                Format.OtioJson,
                written,
                new ReadOptions(
                    mediaLinker: "csharp_proxies",
                    doNotLinkMedia: true,
                    mediaLinkerArguments: "{\"root\": \"/proxies\"}"));
            CheckEq(FirstUrl(unlinked), "file:///media/first.mov", "the unlinked clip's media");
        }
        finally
        {
            Otio.UnregisterMediaLinker("csharp_proxies");
        }
    }

    private static void ALinkerThatLeavesAClipAloneKeepsItsMedia()
    {
        var written = PluginCut();
        var seen = 0;
        Otio.RegisterMediaLinker("csharp_watcher", (clip, arguments) =>
        {
            seen += 1;
            return null;
        });
        try
        {
            var root = Otio.ReadFromBytes(Format.OtioJson, written, new ReadOptions(mediaLinker: "csharp_watcher"));
            CheckEq(seen, 2, "the clips the linker saw");
            CheckEq(FirstUrl(root), "file:///media/first.mov", "the first clip's media");
        }
        finally
        {
            Otio.UnregisterMediaLinker("csharp_watcher");
        }
    }

    private static void ALinkerThatThrowsStopsTheReadInItsOwnWords()
    {
        var written = PluginCut();
        var options = new ReadOptions(mediaLinker: "csharp_offline");
        Otio.RegisterMediaLinker("csharp_offline", (clip, arguments) =>
            throw new InvalidOperationException("the proxies are offline"));
        try
        {
            var message = PluginFailure(() => Otio.ReadFromBytes(Format.OtioJson, written, options));
            Check(
                message is not null && message.Contains("the proxies are offline"),
                $"expected the linker's own failure, got {message}");

            // A mistake is a failure too, not a crash: here a missing argument,
            // which the SDK throws about, and a null dereference, which .NET
            // does.
            Otio.RegisterMediaLinker("csharp_offline", (clip, arguments) =>
                new ExternalReference("proxy", arguments.GetString("missing")));
            Check(
                PluginFailure(() => Otio.ReadFromBytes(Format.OtioJson, written, options)) is not null,
                "expected a missing argument to fail the read");
            Otio.RegisterMediaLinker("csharp_offline", (clip, arguments) =>
            {
                string? nothing = null;
                return new ExternalReference("proxy", nothing!.ToUpperInvariant());
            });
            Check(
                PluginFailure(() => Otio.ReadFromBytes(Format.OtioJson, written, options)) is not null,
                "expected a null dereference to fail the read");

            // A message longer than the room the library gives is cut short,
            // not written past it.
            var longer = new string('x', 5000);
            Otio.RegisterMediaLinker("csharp_offline", (clip, arguments) =>
                throw new InvalidOperationException("é" + longer));
            var cut = PluginFailure(() => Otio.ReadFromBytes(Format.OtioJson, written, options));
            Check(
                cut is not null && cut.Contains("éxxx") && Encoding.UTF8.GetByteCount(cut) < 5000,
                "expected a long message to be cut short");
        }
        finally
        {
            Otio.UnregisterMediaLinker("csharp_offline");
        }

        // And a linker nobody registered is refused, as upstream refuses one.
        var unknown = PluginFailure(() =>
            Otio.ReadFromBytes(Format.OtioJson, written, new ReadOptions(mediaLinker: "csharp_nowhere")));
        Check(
            unknown is not null && unknown.Contains("csharp_nowhere"),
            $"expected an unknown linker to be refused, got {unknown}");
    }

    private static void HookScriptsWrittenInCSharpRunAroundReadsAndWrites()
    {
        var written = PluginCut();
        Otio.RegisterHookScript("csharp_stamp_read", Stamp("read_by"));
        Otio.AttachHookScript("post_adapter_read", "csharp_stamp_read");
        Otio.RegisterHookScript("csharp_stamp_write", Stamp("written_by"));
        Otio.AttachHookScript("pre_adapter_write", "csharp_stamp_write");
        try
        {
            var root = (Timeline)Otio.ReadFromBytes(
                Format.OtioJson, written, new ReadOptions(hookArguments: "{\"who\": \"the C# test\"}"));
            CheckEq(root.Metadata.GetString("read_by"), "the C# test", "what the read hook left");

            // A write runs its hooks on a copy, so the timeline is left alone.
            var output = Otio.WriteToBytes(
                Format.OtioJson, root, new WriteOptions(hookArguments: "{\"who\": \"the writer\"}"));
            Check(
                Encoding.UTF8.GetString(output).Contains("\"written_by\": \"the writer\""),
                "the write hook did not reach what was written");
            Check(!root.Metadata.Contains("written_by"), "the write hook changed the timeline");
        }
        finally
        {
            Otio.DetachHookScript("post_adapter_read", "csharp_stamp_read");
            Otio.UnregisterHookScript("csharp_stamp_read");
            Otio.DetachHookScript("pre_adapter_write", "csharp_stamp_write");
            Otio.UnregisterHookScript("csharp_stamp_write");
        }
    }

    private static void AHookOfYourOwnRunsWhenAsked()
    {
        var clip = new Clip("A");
        Otio.RegisterHookScript("csharp_stamp", Stamp("stamped_by"));
        // A script may hand back a different object to go on with, built
        // fresh in a timeline of its own.
        Otio.RegisterHookScript("csharp_replace", (target, arguments) => new Clip("replacement"));
        // A script that answers with nothing fails, since a hook needs an
        // object to go on with.
        Otio.RegisterHookScript("csharp_nothing", (target, arguments) => null!);
        Otio.AttachHookScript("csharp_mine", "csharp_stamp");
        Otio.AttachHookScript("csharp_swap", "csharp_replace");
        Otio.AttachHookScript("csharp_empty", "csharp_nothing");
        try
        {
            var result = clip.RunHook("csharp_mine", "{\"who\": \"me\"}");
            Check(result == clip, "the hook answered with something other than the clip");
            CheckEq(clip.Metadata.GetString("stamped_by"), "me", "what the hook left");

            var swapped = clip.RunHook("csharp_swap", null);
            Check(swapped is Clip, "the replacement is not a clip");
            CheckEq(((Clip)swapped).Name(), "replacement", "the replacement's name");

            var empty = PluginFailure(() => clip.RunHook("csharp_empty", null));
            Check(
                empty is not null && empty.Contains("no object"),
                $"expected a script with no answer to fail, got {empty}");

            Check(
                PluginFailure(() => clip.RunHook("csharp_undeclared", null)) is not null,
                "expected an undeclared hook to fail");

            // The clip is still whole and usable after all of that.
            CheckEq(clip.Name(), "A", "the clip's name afterwards");
        }
        finally
        {
            Otio.DetachHookScript("csharp_mine", "csharp_stamp");
            Otio.DetachHookScript("csharp_swap", "csharp_replace");
            Otio.DetachHookScript("csharp_empty", "csharp_nothing");
            Otio.UnregisterHookScript("csharp_stamp");
            Otio.UnregisterHookScript("csharp_replace");
            Otio.UnregisterHookScript("csharp_nothing");
        }
    }

    private static void WhatAPluginIsLentStaysTheLibrarys()
    {
        var written = PluginCut();
        SerializableObject? kept = null;
        Status? closing = null;
        Status? moving = null;
        Otio.RegisterHookScript("csharp_lender", (target, arguments) =>
        {
            kept = target;
            // Closing what a plugin is lent does nothing: it is the library's.
            target.Close();
            // Moving it into a timeline of the plugin's own would free it
            // under the library, so that is refused.
            var holder = new Stack("holder");
            moving = Threw(() => holder.AppendChild(target));
            return target;
        });
        // A hook run on a timeline built here is lent that very timeline, so
        // closing it from inside the hook is refused until the hook returns.
        var mine = new Clip("mine");
        Otio.RegisterHookScript("csharp_closer", (target, arguments) =>
        {
            closing = Threw(() => mine.Close());
            return target;
        });
        Otio.AttachHookScript("post_adapter_read", "csharp_lender");
        Otio.AttachHookScript("csharp_close", "csharp_closer");
        try
        {
            var root = Otio.ReadFromBytes(Format.OtioJson, written, null);
            CheckEq(root.FindClips().Length, 2, "the clips after the hook closed what it was lent");
            CheckEq(moving, Status.InvalidArgument, "moving what a plugin is lent");
            // What a plugin was handed is of no use once the call is over.
            CheckEq(
                kept is null ? null : Threw(() => kept.FindClips()),
                Status.NullPointer,
                "using what a plugin kept");

            mine.RunHook("csharp_close", null);
            CheckEq(closing, Status.InvalidArgument, "closing a timeline a hook is running on");
            CheckEq(mine.Name(), "mine", "the clip after its hook");
        }
        finally
        {
            Otio.DetachHookScript("post_adapter_read", "csharp_lender");
            Otio.DetachHookScript("csharp_close", "csharp_closer");
            Otio.UnregisterHookScript("csharp_lender");
            Otio.UnregisterHookScript("csharp_closer");
        }
    }

    /// Registers a script whose closure only the registration holds, and
    /// answers a weak reference to what it closes over.
    [MethodImpl(MethodImplOptions.NoInlining)]
    private static WeakReference RegisterForgettable(string name)
    {
        var captured = new object();
        Otio.RegisterHookScript(name, (target, arguments) =>
        {
            GC.KeepAlive(captured);
            return target;
        });
        return new WeakReference(captured);
    }

    private static void UnregisteringSaysWhetherThereWasAnything()
    {
        Otio.RegisterHookScript("csharp_brief", Stamp("x"));
        Check(Otio.UnregisterHookScript("csharp_brief"), "unregistering found nothing");
        Check(!Otio.UnregisterHookScript("csharp_brief"), "unregistering twice found something");
        Otio.RegisterMediaLinker("csharp_brief", (clip, arguments) => null);
        Check(Otio.UnregisterMediaLinker("csharp_brief"), "unregistering a linker found nothing");
        Check(!Otio.UnregisterMediaLinker("csharp_brief"), "unregistering a linker twice found something");

        CheckEq(
            Threw(() => Otio.RegisterMediaLinker("", (clip, arguments) => null)),
            Status.InvalidArgument,
            "a linker with no name");
        var refused = false;
        try
        {
            Otio.RegisterMediaLinker("csharp_null", null!);
        }
        catch (ArgumentNullException)
        {
            refused = true;
        }
        Check(refused, "a null linker was accepted");

        // Once the library lets a plugin go, so does C#: the delegate, and
        // whatever it closes over, can be collected.
        var weak = RegisterForgettable("csharp_forgotten");
        Check(Otio.UnregisterHookScript("csharp_forgotten"), "unregistering the forgettable script");
        GC.Collect();
        GC.WaitForPendingFinalizers();
        GC.Collect();
        Check(!weak.IsAlive, "the script outlived its registration");

        // Registering a name again releases what was there before.
        var replaced = RegisterForgettable("csharp_replaced");
        Otio.RegisterHookScript("csharp_replaced", Stamp("x"));
        GC.Collect();
        GC.WaitForPendingFinalizers();
        GC.Collect();
        Check(!replaced.IsAlive, "a replaced script outlived its replacement");
        Otio.UnregisterHookScript("csharp_replaced");
    }

    private static readonly (string Name, Action Body)[] PluginTests =
    {
        ("a media linker written in C# links every clip", AMediaLinkerWrittenInCSharpLinksEveryClip),
        ("a linker that leaves a clip alone keeps its media", ALinkerThatLeavesAClipAloneKeepsItsMedia),
        ("a linker that throws stops the read in its own words", ALinkerThatThrowsStopsTheReadInItsOwnWords),
        ("hook scripts written in C# run around reads and writes", HookScriptsWrittenInCSharpRunAroundReadsAndWrites),
        ("a hook of your own runs when asked", AHookOfYourOwnRunsWhenAsked),
        ("what a plugin is lent stays the library's", WhatAPluginIsLentStaysTheLibrarys),
        ("unregistering says whether there was anything", UnregisteringSaysWhetherThereWasAnything),
    };
}
