// Tests for the multicam prototype: Multicam.1, MulticamClip.1 and
// Timeline.multicams, proposed for OpenTimelineIO and not yet part of it.

package otio_test

import (
	"testing"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
)

func span(t *testing.T, start, frames float64) otio.TimeRange {
	t.Helper()
	return otio.TimeRange{
		StartTime: otio.RationalTime{Value: start, Rate: 24},
		Duration:  otio.RationalTime{Value: frames, Rate: 24},
	}
}

// must answers with value, or fails the test by panicking with err: Go
// cannot pass a call's two results and a third argument together.
func must[T any](value T, err error) T {
	if err != nil {
		panic(err)
	}
	return value
}

func TestMulticamCutSwitchesAngleAndFlattens(t *testing.T) {
	timeline := must(otio.NewTimeline("cut"))
	tracks, _ := must(timeline.Tracks()).AsStack()

	made := must(otio.MulticamNew(timeline.Node, "Interview", nil))
	definition, ok := made.AsStack()
	if !ok {
		t.Fatal("a multicam does not read as a stack")
	}
	for _, name := range []string{"A", "B"} {
		angle := must(otio.NewStack(name))
		picture := must(otio.NewTrack(name+" V", "Video"))
		take := must(otio.NewClip(name + "_take"))
		if err := take.SetSourceRange(span(t, 0, 240)); err != nil {
			t.Fatal(err)
		}
		if err := picture.AppendChild(take.Node); err != nil {
			t.Fatal(err)
		}
		if err := angle.AppendChild(picture.Node); err != nil {
			t.Fatal(err)
		}
		if err := definition.AppendChild(angle.Node); err != nil {
			t.Fatal(err)
		}
	}
	if err := otio.MulticamSet(timeline.Node, "mc1", definition.Node); err != nil {
		t.Fatal(err)
	}
	if count := must(otio.MulticamCount(timeline.Node)); count != 1 {
		t.Fatalf("MulticamCount = %d, want 1", count)
	}

	track := must(otio.NewTrack("V1", "Video"))
	if err := tracks.AppendChild(track.Node); err != nil {
		t.Fatal(err)
	}
	cut := must(otio.MulticamClipNew(timeline.Node, "cut 1", "mc1", "A", 0, span(t, 24, 48)))
	if err := track.AppendChild(cut); err != nil {
		t.Fatal(err)
	}
	if err := otio.MulticamClipSetAngle(cut, "B", 0); err != nil {
		t.Fatal(err)
	}
	played := must(otio.MulticamResolve(timeline.Node, cut, ""))
	if name := must(played.Name()); name != "B V" {
		t.Errorf("resolved track = %q, want %q", name, "B V")
	}
	if report := must(otio.MulticamValidate(timeline.Node)); report != "" {
		t.Errorf("MulticamValidate = %q, want no problems", report)
	}

	if lost := must(otio.MulticamFlatten(timeline.Node)); lost != "" {
		t.Errorf("MulticamFlatten lost %q", lost)
	}
	clips := must(timeline.FindClips())
	if len(clips) != 1 || must(clips[0].Name()) != "B_take" {
		t.Fatalf("after flattening, clips = %v", clips)
	}
	clip, _ := clips[0].AsClip()
	got := must(clip.SourceRange())
	if got.StartTime.Value != 24 || got.Duration.Value != 48 {
		t.Errorf("flattened source range = %+v, want 24 for 48", got)
	}
}
