// Tests for media linkers and hook scripts written in Go.
//
// The registry is the library's, shared by the whole process, so every test
// here registers under names of its own and unregisters them when it ends.

package otio_test

import (
	"errors"
	"strings"
	"testing"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
)

// isPluginError reports whether err is a failure a plugin, or the registry,
// reported.
func isPluginError(err error) bool {
	var failure *otio.Error
	return errors.As(err, &failure) && failure.Status == otio.StatusPluginError
}

// pluginCut writes a two-clip timeline as OTIO JSON, each clip pointing at
// media under file:///media.
func pluginCut(t *testing.T) []byte {
	t.Helper()
	timeline, err := otio.NewTimeline("Cut")
	if err != nil {
		t.Fatal(err)
	}
	defer timeline.Close()
	stack, err := otio.NewStack("tracks")
	if err != nil {
		t.Fatal(err)
	}
	track, err := otio.NewTrack("V1", "Video")
	if err != nil {
		t.Fatal(err)
	}
	if err := timeline.SetTracks(&stack.Node); err != nil {
		t.Fatal(err)
	}
	if err := stack.AppendChild(track.Node); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"first", "second"} {
		clip, err := otio.NewClip(name)
		if err != nil {
			t.Fatal(err)
		}
		reference, err := otio.NewExternalReference(name, "file:///media/"+name+".mov")
		if err != nil {
			t.Fatal(err)
		}
		if err := clip.SetMediaReference("DEFAULT_MEDIA", reference.Node); err != nil {
			t.Fatal(err)
		}
		if err := clip.SetActiveMediaReferenceKey("DEFAULT_MEDIA"); err != nil {
			t.Fatal(err)
		}
		if err := track.AppendChild(clip.Node); err != nil {
			t.Fatal(err)
		}
	}
	written, err := otio.WriteToBytes(otio.FormatOTIOJSON, timeline.Node, nil)
	if err != nil {
		t.Fatal(err)
	}
	return written
}

// firstURL answers the target URL of the first clip's active media.
func firstURL(t *testing.T, root otio.Node) string {
	t.Helper()
	clips, err := root.FindClips()
	if err != nil || len(clips) == 0 {
		t.Fatalf("finding the clips: %d, %v", len(clips), err)
	}
	clip, _ := clips[0].AsClip()
	media, err := clip.MediaReference("")
	if err != nil {
		t.Fatal(err)
	}
	external, ok := media.AsExternalReference()
	if !ok {
		t.Fatal("the clip's media is not an external reference")
	}
	url, err := external.TargetURL()
	if err != nil {
		t.Fatal(err)
	}
	return url
}

func TestAMediaLinkerWrittenInGoLinksEveryClip(t *testing.T) {
	written := pluginCut(t)
	err := otio.RegisterMediaLinker("go_proxies", func(clip otio.Clip, arguments otio.Metadata) (otio.Node, error) {
		name, err := clip.Name()
		if err != nil {
			return otio.Node{}, err
		}
		root, err := arguments.GetString("root")
		if err != nil {
			return otio.Node{}, errors.New("no root to link under")
		}
		reference, err := otio.NewExternalReference("proxy", root+"/"+name+".mov")
		return reference.Node, err
	})
	if err != nil {
		t.Fatal(err)
	}
	defer otio.UnregisterMediaLinker("go_proxies")

	options := &otio.ReadOptions{
		MediaLinker:          "go_proxies",
		MediaLinkerArguments: `{"root": "/proxies"}`,
	}
	root, err := otio.ReadFromBytes(otio.FormatOTIOJSON, written, options)
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()
	if url := firstURL(t, root); url != "/proxies/first.mov" {
		t.Fatalf("the first clip links to %q", url)
	}

	// Asked not to link, it does not.
	options.DoNotLinkMedia = true
	unlinked, err := otio.ReadFromBytes(otio.FormatOTIOJSON, written, options)
	if err != nil {
		t.Fatal(err)
	}
	defer unlinked.Close()
	if url := firstURL(t, unlinked); url != "file:///media/first.mov" {
		t.Fatalf("the first clip links to %q", url)
	}
}

func TestALinkerThatLeavesAClipAloneKeepsItsMedia(t *testing.T) {
	written := pluginCut(t)
	seen := 0
	err := otio.RegisterMediaLinker("go_watcher", func(clip otio.Clip, arguments otio.Metadata) (otio.Node, error) {
		seen++
		return otio.Node{}, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	defer otio.UnregisterMediaLinker("go_watcher")

	root, err := otio.ReadFromBytes(otio.FormatOTIOJSON, written, &otio.ReadOptions{MediaLinker: "go_watcher"})
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()
	if seen != 2 {
		t.Fatalf("the linker saw %d clips", seen)
	}
	if url := firstURL(t, root); url != "file:///media/first.mov" {
		t.Fatalf("the first clip links to %q", url)
	}
}

func TestALinkerThatFailsStopsTheReadInItsOwnWords(t *testing.T) {
	written := pluginCut(t)
	err := otio.RegisterMediaLinker("go_offline", func(otio.Clip, otio.Metadata) (otio.Node, error) {
		return otio.Node{}, errors.New("the proxies are offline")
	})
	if err != nil {
		t.Fatal(err)
	}
	defer otio.UnregisterMediaLinker("go_offline")

	_, err = otio.ReadFromBytes(otio.FormatOTIOJSON, written, &otio.ReadOptions{MediaLinker: "go_offline"})
	if !isPluginError(err) || !strings.Contains(err.Error(), "the proxies are offline") {
		t.Fatalf("expected the linker's own failure, got %v", err)
	}

	// A panic is a failure too, not a crash.
	err = otio.RegisterMediaLinker("go_offline", func(otio.Clip, otio.Metadata) (otio.Node, error) {
		panic("no disk")
	})
	if err != nil {
		t.Fatal(err)
	}
	_, err = otio.ReadFromBytes(otio.FormatOTIOJSON, written, &otio.ReadOptions{MediaLinker: "go_offline"})
	if !isPluginError(err) || !strings.Contains(err.Error(), "no disk") {
		t.Fatalf("expected the panic as a failure, got %v", err)
	}

	// And a linker nobody registered is refused, as upstream refuses one.
	_, err = otio.ReadFromBytes(otio.FormatOTIOJSON, written, &otio.ReadOptions{MediaLinker: "go_nowhere"})
	if !isPluginError(err) || !strings.Contains(err.Error(), "go_nowhere") {
		t.Fatalf("expected an unknown linker to be refused, got %v", err)
	}
}

// stamp is a hook script that writes who ran it into the metadata of what
// it is handed, under key.
func stamp(key string) otio.HookScript {
	return func(target otio.Node, arguments otio.Metadata) (otio.Node, error) {
		who, err := arguments.GetString("who")
		if err != nil {
			who = "nobody"
		}
		return target, target.Metadata().SetString(key, who)
	}
}

func TestHookScriptsWrittenInGoRunAroundReadsAndWrites(t *testing.T) {
	written := pluginCut(t)
	if err := otio.RegisterHookScript("go_stamp_read", stamp("read_by")); err != nil {
		t.Fatal(err)
	}
	defer otio.UnregisterHookScript("go_stamp_read")
	if err := otio.AttachHookScript("post_adapter_read", "go_stamp_read"); err != nil {
		t.Fatal(err)
	}
	defer otio.DetachHookScript("post_adapter_read", "go_stamp_read")

	root, err := otio.ReadFromBytes(otio.FormatOTIOJSON, written, &otio.ReadOptions{
		HookArguments: `{"who": "the Go test"}`,
	})
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()
	if who, err := root.Metadata().GetString("read_by"); err != nil || who != "the Go test" {
		t.Fatalf("the read hook left %q, %v", who, err)
	}

	// A write runs its hooks on a copy, so the timeline is left alone.
	if err := otio.RegisterHookScript("go_stamp_write", stamp("written_by")); err != nil {
		t.Fatal(err)
	}
	defer otio.UnregisterHookScript("go_stamp_write")
	if err := otio.AttachHookScript("pre_adapter_write", "go_stamp_write"); err != nil {
		t.Fatal(err)
	}
	defer otio.DetachHookScript("pre_adapter_write", "go_stamp_write")
	out, err := otio.WriteToBytes(otio.FormatOTIOJSON, root, &otio.WriteOptions{
		HookArguments: `{"who": "the writer"}`,
	})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(out), `"written_by": "the writer"`) {
		t.Fatal("the write hook did not reach what was written")
	}
	if _, err := root.Metadata().GetString("written_by"); !errors.Is(err, otio.ErrNoValue) {
		t.Fatalf("the write hook changed the timeline: %v", err)
	}
}

func TestAHookOfYourOwnRunsWhenAsked(t *testing.T) {
	clip, err := otio.NewClip("A")
	if err != nil {
		t.Fatal(err)
	}
	defer clip.Close()

	// A script may hand back a different object to go on with.
	err = otio.RegisterHookScript("go_replace", func(target otio.Node, arguments otio.Metadata) (otio.Node, error) {
		replacement, err := otio.NewClip("replacement")
		return replacement.Node, err
	})
	if err != nil {
		t.Fatal(err)
	}
	defer otio.UnregisterHookScript("go_replace")
	if err := otio.RegisterHookScript("go_stamp", stamp("stamped_by")); err != nil {
		t.Fatal(err)
	}
	defer otio.UnregisterHookScript("go_stamp")

	if err := otio.AttachHookScript("go_mine", "go_stamp"); err != nil {
		t.Fatal(err)
	}
	defer otio.DetachHookScript("go_mine", "go_stamp")
	result, err := clip.RunHook("go_mine", `{"who": "me"}`)
	if err != nil {
		t.Fatal(err)
	}
	if !result.Equals(clip.Node) {
		t.Fatal("the hook answered with something other than the clip")
	}
	if who, _ := clip.Metadata().GetString("stamped_by"); who != "me" {
		t.Fatalf("the hook left %q", who)
	}

	if err := otio.AttachHookScript("go_swap", "go_replace"); err != nil {
		t.Fatal(err)
	}
	defer otio.DetachHookScript("go_swap", "go_replace")
	swapped, err := clip.RunHook("go_swap", "")
	if err != nil {
		t.Fatal(err)
	}
	if name, _ := swapped.Name(); name != "replacement" {
		t.Fatalf("the hook answered with %q", name)
	}

	// A script that answers with nothing fails, since a hook needs an
	// object to go on with.
	err = otio.RegisterHookScript("go_nothing", func(otio.Node, otio.Metadata) (otio.Node, error) {
		return otio.Node{}, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	defer otio.UnregisterHookScript("go_nothing")
	if err := otio.AttachHookScript("go_empty", "go_nothing"); err != nil {
		t.Fatal(err)
	}
	defer otio.DetachHookScript("go_empty", "go_nothing")
	if _, err := clip.RunHook("go_empty", ""); !isPluginError(err) {
		t.Fatalf("expected a script with no answer to fail, got %v", err)
	}

	if _, err := clip.RunHook("go_undeclared", ""); !isPluginError(err) {
		t.Fatalf("expected an undeclared hook to fail, got %v", err)
	}
}

func TestUnregisteringSaysWhetherThereWasAnything(t *testing.T) {
	if err := otio.RegisterHookScript("go_brief", stamp("x")); err != nil {
		t.Fatal(err)
	}
	if !otio.UnregisterHookScript("go_brief") || otio.UnregisterHookScript("go_brief") {
		t.Fatal("unregistering did not answer once, then not again")
	}
	if err := otio.RegisterMediaLinker("", func(otio.Clip, otio.Metadata) (otio.Node, error) {
		return otio.Node{}, nil
	}); err == nil {
		t.Fatal("a linker with no name was accepted")
	}
	if err := otio.RegisterMediaLinker("go_nil", nil); err == nil {
		t.Fatal("a nil linker was accepted")
	}
}

func TestAHookCannotFreeTheDocumentItRunsOn(t *testing.T) {
	timeline, err := otio.NewTimeline("lent")
	if err != nil {
		t.Fatal(err)
	}
	defer timeline.Close()
	var moved error
	err = otio.RegisterHookScript("go_escape", func(target otio.Node, arguments otio.Metadata) (otio.Node, error) {
		// Moving the target into another document would free the one the
		// library is holding.
		elsewhere, err := otio.NewStack("elsewhere")
		if err != nil {
			return otio.Node{}, err
		}
		defer elsewhere.Close()
		moved = elsewhere.AppendChild(target)
		// And closing the caller's own handle on it does nothing yet.
		timeline.Close()
		return target, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	defer otio.UnregisterHookScript("go_escape")
	if err := otio.AttachHookScript("go_escape_hook", "go_escape"); err != nil {
		t.Fatal(err)
	}
	defer otio.DetachHookScript("go_escape_hook", "go_escape")
	if _, err := timeline.RunHook("go_escape_hook", ""); err != nil {
		t.Fatal(err)
	}
	if moved == nil {
		t.Fatalf("moving the lent target out was allowed: %v", moved)
	}
	if name, err := timeline.Name(); err != nil || name != "lent" {
		t.Fatalf("the timeline did not survive its hook: %q, %v", name, err)
	}
}
