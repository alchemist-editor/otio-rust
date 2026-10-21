// Tests for ORI Annotations, an otio-rust extension and not part of
// OpenTimelineIO.

package otio_test

import (
	"errors"
	"math"
	"os"
	"path/filepath"
	"strings"
	"testing"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
	"github.com/alchemist-editor/otio-rust/sdk/go/ext/ori"
)

func enableOri(t *testing.T) {
	t.Helper()
	if err := ori.Enable(); err != nil {
		t.Fatal(err)
	}
}

func oriFixture(t *testing.T, name string) string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("..", "..", "extensions", "ori", "otio-ext-ori-annotations", "tests", "fixtures", name))
	if err != nil {
		t.Fatal(err)
	}
	return string(data)
}

func TestOriExtensionEnablesSync(t *testing.T) {
	enableOri(t)
	for _, id := range []string{"ori-annotations", "sync"} {
		on, err := otio.IsExtensionEnabled(id)
		if err != nil {
			t.Fatal(err)
		}
		if !on {
			t.Errorf("%s is off after ori.Enable", id)
		}
	}
}

func TestOriReadsWhatOriWrote(t *testing.T) {
	enableOri(t)
	timeline := openTimeline(t, oriFixture(t, "ori_built.otio"))

	holders, err := ori.List(timeline)
	if err != nil {
		t.Fatal(err)
	}
	if len(holders) != 2 {
		t.Fatalf("found %d annotations, want 2", len(holders))
	}
	described, err := ori.Describe(holders[0])
	if err != nil {
		t.Fatal(err)
	}
	want := `{"form":"track","clip_guid":"shot","clip_name":null,"frame":{"value":10.0,"rate":24.0},"note":null,"status":null,"command_count":4}`
	if described != want {
		t.Errorf("described as %s", described)
	}

	track, err := ori.FindTrack(timeline)
	if err != nil {
		t.Fatal(err)
	}
	if name, _ := track.Name(); name != "Annotations" {
		t.Errorf("found the track %q", name)
	}
	if _, err := ori.FindClip(track, "shot", 99); err == nil {
		t.Error("found a clip at a frame nobody annotated")
	} else {
		var failure *otio.Error
		if !errors.As(err, &failure) || failure.Status != otio.StatusNoValue {
			t.Errorf("a missing clip returned %v", err)
		}
	}
	atFour, err := ori.ListAtFrame(timeline, "shot", 4)
	if err != nil || len(atFour) != 1 {
		t.Fatalf("frame 4 has %d annotations (%v), want 1", len(atFour), err)
	}
}

func TestOriAnnotatesAFrame(t *testing.T) {
	enableOri(t)
	built := openTimeline(t, oriFixture(t, "ori_built.otio"))
	holders, err := ori.List(built)
	if err != nil {
		t.Fatal(err)
	}
	commands, err := ori.Commands(holders[0], false)
	if err != nil {
		t.Fatal(err)
	}

	timeline := openTimeline(t, `{"OTIO_SCHEMA": "Timeline.1",
		"tracks": {"OTIO_SCHEMA": "Stack.1", "children": []}}`)
	track, err := ori.EnsureTrack(timeline)
	if err != nil {
		t.Fatal(err)
	}
	clip, err := ori.EnsureClip(track, "shot", 10, 24)
	if err != nil {
		t.Fatal(err)
	}
	if err := ori.AddCommands(clip, commands); err != nil {
		t.Fatal(err)
	}
	again, err := ori.EnsureClip(track, "shot", 10, 24)
	if err != nil {
		t.Fatal(err)
	}
	if !again.Equals(clip) {
		t.Error("asking again added a second clip")
	}
	if back, _ := ori.Commands(clip, false); back != commands {
		t.Errorf("the commands came back as %s", back)
	}

	replaced, err := ori.ReplaceCommands(clip, "stroke-1",
		`[{"OTIO_SCHEMA": "TextAnnotation.1", "uuid": "stroke-1", "text": "was a stroke"}]`)
	if err != nil || !replaced {
		t.Fatalf("replaced %v, %v", replaced, err)
	}
	removed, err := ori.RemoveCommands(clip, `["text-1"]`)
	if err != nil || removed != 1 {
		t.Fatalf("removed %d, %v", removed, err)
	}
	left, _ := ori.Commands(clip, false)
	if !strings.Contains(left, `"text":"was a stroke"`) || strings.Contains(left, "text-1") {
		t.Errorf("left %s", left)
	}
	if err := ori.SetCommands(clip, "[]"); err != nil {
		t.Fatal(err)
	}
	if err := ori.AddCommands(clip, "not json"); err == nil {
		t.Error("added commands that are not JSON")
	}
}

func TestOriCoordinates(t *testing.T) {
	enableOri(t)
	x, y, err := ori.PixelsToOri(1920, 1080, 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	if math.Abs(x+8.0/9.0) > 1e-12 || math.Abs(y-0.5) > 1e-12 {
		t.Errorf("the top left corner is %v, %v", x, y)
	}
	px, py, err := ori.OriToPixels(1920, 1080, x, y)
	if err != nil || math.Abs(px) > 1e-9 || math.Abs(py) > 1e-9 {
		t.Errorf("back to %v, %v (%v)", px, py, err)
	}
	bx, by, err := ori.OriToBounds(0.5, 0.5, -16, -9, 16, 9)
	if err != nil || bx != 9 || by != 9 {
		t.Errorf("in the bounds at %v, %v (%v)", bx, by, err)
	}
}
