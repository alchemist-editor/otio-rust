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
	"github.com/alchemist-editor/otio-rust/sdk/go/ext/sync"
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

// A timeline whose one clip has the sync GUID main/clip-1.
const oriReviewTimeline = `{"OTIO_SCHEMA":"Timeline.1","name":"Program","metadata":{"sync":{"guid":"main"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"main/stack"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"main/v1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Beach Run","metadata":{"sync":{"guid":"main/clip-1"}},"source_range":{"OTIO_SCHEMA":"TimeRange.1","start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},"duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}}]}]}}`

// One stroke, s1.
const oriReviewStroke = `[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0,0,1]},{"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1","x":[0.1],"y":[0.2],"size":[0.01],"alpha":[1]}},{"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}]`

// Where a reviewer drew it: frame 10 of main/clip-1.
const oriReviewAnchor = `{"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/clip-1","sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},"capturedAt":1787742000000}`

// What a viewer who may comment hears on joining.
const oriReviewWelcome = `{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","selfUserId":"u_bea","role":"viewer","roster":[],"session":{"id":"s_1","createdAt":0}}}`

// The comment, as the host's session hears it from Furnace.
const oriReviewRelay = `{"type":"review_relay","commit":{"clientMutationId":"m1","anchor":` + oriReviewAnchor + `,"bodyMarkdown":"Too dark.","annotationCommands":` + oriReviewStroke + `},"ack":{"clientMutationId":"m1","commentId":"c1","serverOrder":1,"revision":1},"authorUserId":"u_bea","authorPresenceId":"p_2","createdAt":0}`

// A stroke in progress, as ORI's RV plugin sends one.
const oriReviewPartial = `{"type":"message","kind":"Annotation.1/PARTIAL","sender":"p_3","payload":{"clip_guid":"main/clip-1","frame":10.0,"fps":24.0,"events":[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s2"}]}}`

func TestOriMergeRules(t *testing.T) {
	enableOri(t)
	timeline := openTimeline(t, oriReviewTimeline)
	track, err := ori.EnsureTrack(timeline)
	if err != nil {
		t.Fatal(err)
	}
	clip, err := ori.EnsureClip(track, "main/clip-1", 10, 24)
	if err != nil {
		t.Fatal(err)
	}
	if err := ori.SetCommands(clip, `[{"OTIO_SCHEMA":"TextAnnotation.1","uuid":"t"},{"OTIO_SCHEMA":"PaintStart.1","uuid":"a"}]`); err != nil {
		t.Fatal(err)
	}
	replaced, added, err := ori.MergeCommands(clip, `[{"OTIO_SCHEMA":"PaintStart.1","uuid":"a","brush":"gauss"},{"OTIO_SCHEMA":"PaintStart.1","uuid":"b"}]`)
	if err != nil || replaced != 1 || added != 1 {
		t.Fatalf("MergeCommands = %d, %d, %v; want 1, 1", replaced, added, err)
	}
	removed, err := ori.ReplaceByKind(clip, `[{"OTIO_SCHEMA":"PaintStart.1","uuid":"c"}]`)
	if err != nil || removed != 2 {
		t.Fatalf("ReplaceByKind = %d, %v; want 2", removed, err)
	}
	left, err := ori.Commands(clip, false)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(left, `"uuid":"t"`) || !strings.Contains(left, `"uuid":"c"`) {
		t.Errorf("the text was not kept beside the new stroke: %s", left)
	}
	if count, err := ori.MergeDuplicates(timeline); err != nil || count != 0 {
		t.Errorf("MergeDuplicates = %d, %v; want 0", count, err)
	}
	if _, err := ori.OwnerOf(clip, "t"); !errors.Is(err, otio.ErrNoValue) {
		t.Errorf("OwnerOf a stroke no comment added returned %v, not ErrNoValue", err)
	}
}

func TestOriLiveReview(t *testing.T) {
	enableOri(t)

	// A stroke in progress from another peer.
	partial, err := ori.ReadPartial(oriReviewPartial)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(partial, `"clip_guid":"main/clip-1"`) || !strings.Contains(partial, `"brush":"circle"`) {
		t.Errorf("the partial is not read with ORI's defaults: %s", partial)
	}
	if _, err := ori.ReadPartial(`{"type":"leader","leader":null}`); !errors.Is(err, otio.ErrNoValue) {
		t.Errorf("ReadPartial of another event returned %v, not ErrNoValue", err)
	}

	// A reviewer commits a stroke with a note.
	reviewer := newSession(t, `{"token":"join-code","identity":{"capabilities":["review.comment.create","review.annotation.create"]},"encodings":[]}`)
	if err := sync.SessionConnect(reviewer); err != nil {
		t.Fatal(err)
	}
	drainFrames(t, reviewer)
	if err := sync.SessionReceive(reviewer, []byte(oriReviewWelcome), false); err != nil {
		t.Fatal(err)
	}
	drainEvents(t, reviewer)
	copyOfTimeline := openTimeline(t, oriReviewTimeline)
	id, err := ori.Commit(reviewer, copyOfTimeline, oriReviewAnchor, "Too dark.", oriReviewStroke, "")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(id, "mutation_") {
		t.Errorf("the commit's id is %q", id)
	}
	frames := strings.Join(drainFrames(t, reviewer), "\n")
	if !strings.Contains(frames, `"type":"review_commit"`) || !strings.Contains(frames, `"uuid":"s1"`) {
		t.Errorf("the commit was not sent with its stroke: %s", frames)
	}
	if err := ori.SendPartial(reviewer, `{"clip_guid":"main/clip-1","frame":10,"fps":24,"events":[]}`); err != nil {
		t.Fatal(err)
	}
	_, _, err = sync.SessionPollTransmit(reviewer, 0)
	var failure *otio.Error
	if !errors.As(err, &failure) || failure.Status != otio.StatusUnsupported {
		t.Errorf("Furnace sending a stroke in progress returned %v, not unsupported", err)
	}

	// The host folds the comment the server relays into its timeline.
	timeline := openTimeline(t, oriReviewTimeline)
	clip, already, err := ori.Fold(timeline, oriReviewRelay)
	if err != nil || already {
		t.Fatalf("Fold = %v, %v", already, err)
	}
	if _, already, err = ori.Fold(timeline, oriReviewRelay); err != nil || !already {
		t.Errorf("folding the comment again = %v, %v; want already", already, err)
	}
	commands, err := ori.Commands(clip, false)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Count(commands, `"uuid":"s1"`) != 3 {
		t.Errorf("the stroke did not arrive whole: %s", commands)
	}
	if owner, err := ori.OwnerOf(clip, "s1"); err != nil || owner != "u_bea" {
		t.Errorf("OwnerOf = %q, %v; want u_bea", owner, err)
	}
}
