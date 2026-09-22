// Tests for the generated Go SDK.
//
// These are written by hand, not generated. A generator that also wrote its
// own tests would only prove it is self-consistent; what needs proving is
// that the Go it writes does what a Go programmer reading it would expect,
// against the same library and the same fixtures the Rust tests use.

package otio_test

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	otio "github.com/jhodges10/otio-rust/sdk/go"
)

// The EDL the Rust adapter's own tests read, so that the two agree about
// what is in it.
const screeningEDL = "../../crates/otio-cmx3600/tests/data/screening_example.edl"

func TestVersionIsReported(t *testing.T) {
	if otio.Version() == "" {
		t.Fatal("the library reports no version")
	}
}

func TestReadingAnEDLFindsItsClips(t *testing.T) {
	document, err := otio.ReadFromFile(otio.FormatCMX3600, screeningEDL, nil)
	if err != nil {
		t.Fatalf("reading the EDL: %v", err)
	}
	defer document.Close()

	root, err := document.Root()
	if err != nil {
		t.Fatalf("asking for the root: %v", err)
	}
	clips, err := root.FindClips()
	if err != nil {
		t.Fatalf("finding the clips: %v", err)
	}
	if len(clips) != 9 {
		t.Fatalf("expected 9 clips, found %d", len(clips))
	}

	// Every one of them really is a clip, and says so.
	for _, node := range clips {
		if _, ok := node.AsClip(); !ok {
			kind, _ := node.SchemaKind()
			t.Fatalf("FindClips answered with a %v", kind)
		}
	}

	// And the typed view of the same list is the same length.
	if typed := otio.Filter(clips, otio.Node.AsClip); len(typed) != len(clips) {
		t.Fatalf("Filter kept %d of %d clips", len(typed), len(clips))
	}
}

func TestOpenWorksOutTheFormatFromTheName(t *testing.T) {
	document, err := otio.Open(screeningEDL)
	if err != nil {
		t.Fatalf("opening the EDL: %v", err)
	}
	defer document.Close()

	root, err := document.Root()
	if err != nil {
		t.Fatal(err)
	}
	name, err := root.Name()
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(name, "Example_Screening") {
		t.Fatalf("the timeline is called %q", name)
	}
}

func TestOpenDeclinesASuffixNoFormatClaims(t *testing.T) {
	_, err := otio.Open("somewhere/cut.wav")
	if !errors.Is(err, otio.ErrNoValue) {
		t.Fatalf("expected ErrNoValue for an unknown suffix, got %v", err)
	}
}

func TestBuildingATimelineFromNothing(t *testing.T) {
	document := otio.New()
	defer document.Close()

	timeline, err := document.NewTimeline("Cut")
	if err != nil {
		t.Fatal(err)
	}
	stack, err := document.NewStack("tracks")
	if err != nil {
		t.Fatal(err)
	}
	track, err := document.NewTrack("V1", "Video")
	if err != nil {
		t.Fatal(err)
	}
	if err := timeline.SetTracks(&stack.Node); err != nil {
		t.Fatal(err)
	}
	if err := stack.AppendChild(track.Node); err != nil {
		t.Fatal(err)
	}

	rate := 24.0
	for _, name := range []string{"A", "B", "C"} {
		clip, err := document.NewClip(name)
		if err != nil {
			t.Fatal(err)
		}
		span := otio.TimeRange{
			StartTime: otio.RationalTime{Value: 0, Rate: rate},
			Duration:  otio.RationalTime{Value: 12, Rate: rate},
		}
		if err := clip.SetSourceRange(span); err != nil {
			t.Fatal(err)
		}
		if err := track.AppendChild(clip.Node); err != nil {
			t.Fatal(err)
		}
	}

	count, err := track.ChildCount()
	if err != nil {
		t.Fatal(err)
	}
	if count != 3 {
		t.Fatalf("the track holds %d children", count)
	}

	duration, err := track.Duration()
	if err != nil {
		t.Fatal(err)
	}
	if duration.Value != 36 || duration.Rate != rate {
		t.Fatalf("the track lasts %v at %v", duration.Value, duration.Rate)
	}

	// The kind a track carries is its own, not the schema's.
	kind, err := track.Kind()
	if err != nil {
		t.Fatal(err)
	}
	if kind != "Video" {
		t.Fatalf("the track is a %q track", kind)
	}
	schema, err := track.SchemaKind()
	if err != nil {
		t.Fatal(err)
	}
	if schema != otio.NodeKindTrack {
		t.Fatalf("the track says it is a %v", schema)
	}
}

func TestAskingAnObjectForSomethingItIsNotFailsLoudly(t *testing.T) {
	document := otio.New()
	defer document.Close()

	clip, err := document.NewClip("A")
	if err != nil {
		t.Fatal(err)
	}

	// A clip is not a track, and the library says so rather than handing
	// back an empty string.
	track := otio.Track{Composition: otio.Composition{Item: clip.Item}}
	kind, err := track.Kind()
	if err == nil {
		t.Fatalf("a clip answered a track's question with %q", kind)
	}
	var failure *otio.Error
	if !errors.As(err, &failure) {
		t.Fatalf("the failure is a %T", err)
	}
	if failure.Status != otio.StatusCoreError {
		t.Fatalf("the failure is %v", failure.Status)
	}
	if !strings.Contains(failure.Error(), "not a track") {
		t.Fatalf("the failure says %q", failure.Error())
	}

	// And the checked conversion declines rather than building one.
	if _, ok := clip.Node.AsTrack(); ok {
		t.Fatal("a clip converted to a track")
	}
}

func TestAnObjectOfNoDocumentFailsRatherThanPanicking(t *testing.T) {
	var orphan otio.Node
	if _, err := orphan.Name(); err == nil {
		t.Fatal("an object belonging to no document answered a question")
	}
	var clip otio.Clip
	if _, err := clip.SourceRange(); err == nil {
		t.Fatal("a zero clip answered a question")
	}
}

func TestAStaleHandleIsRefused(t *testing.T) {
	document := otio.New()
	defer document.Close()

	clip, err := document.NewClip("doomed")
	if err != nil {
		t.Fatal(err)
	}
	if err := document.RemoveNode(clip.Node); err != nil {
		t.Fatal(err)
	}
	if _, err := clip.Name(); err == nil {
		t.Fatal("a removed object still answers")
	} else {
		var failure *otio.Error
		if !errors.As(err, &failure) || failure.Status != otio.StatusStaleHandle {
			t.Fatalf("expected a stale handle, got %v", err)
		}
	}
}

func TestNoValueIsAnAnswerAndNotAFailure(t *testing.T) {
	document := otio.New()
	defer document.Close()

	clip, err := document.NewClip("untrimmed")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := clip.SourceRange(); !errors.Is(err, otio.ErrNoValue) {
		t.Fatalf("an untrimmed clip reported %v", err)
	}

	span := otio.TimeRange{
		StartTime: otio.RationalTime{Value: 5, Rate: 24},
		Duration:  otio.RationalTime{Value: 10, Rate: 24},
	}
	if err := clip.SetSourceRange(span); err != nil {
		t.Fatal(err)
	}
	got, err := clip.SourceRange()
	if err != nil {
		t.Fatal(err)
	}
	if got != span {
		t.Fatalf("the clip reports %+v", got)
	}
}

func TestTimeValuesComputeWithoutADocument(t *testing.T) {
	first := otio.RationalTimeFromFrames(24, 24)
	if seconds := first.ToSeconds(); seconds != 1 {
		t.Fatalf("24 frames at 24 is %v seconds", seconds)
	}

	timecode, err := first.ToTimecode()
	if err != nil {
		t.Fatal(err)
	}
	if timecode != "00:00:01:00" {
		t.Fatalf("the timecode is %q", timecode)
	}

	again, err := otio.RationalTimeFromTimecode(timecode, 24)
	if err != nil {
		t.Fatal(err)
	}
	if !again.Equals(first) {
		t.Fatalf("%v came back as %v", first, again)
	}

	sum := first.Add(otio.RationalTimeFromFrames(12, 24))
	if sum.Value != 36 {
		t.Fatalf("24 and 12 frames make %v", sum.Value)
	}

	span := otio.TimeRangeFromStartEndTime(first, sum)
	if span.Duration.Value != 12 {
		t.Fatalf("the span lasts %v", span.Duration.Value)
	}
	if !span.ContainsTime(otio.RationalTimeFromFrames(30, 24)) {
		t.Fatal("the span does not contain a time inside it")
	}

	// 29.97 written out is not the rate; 30000/1001 is, and the library
	// says which of the SMPTE rates a written-out one meant.
	nearest := otio.NearestSMPTETimecodeRate(29.97)
	if !otio.IsDropFrameRate(nearest) {
		t.Fatalf("the rate nearest 29.97 is %v, which is not a drop-frame rate", nearest)
	}
	if !otio.IsSMPTETimecodeRate(24) {
		t.Fatal("24 is a SMPTE timecode rate")
	}
	if otio.IsDropFrameRate(24) {
		t.Fatal("24 is not a drop-frame rate")
	}
}

func TestMetadataGoesInAndComesBack(t *testing.T) {
	document := otio.New()
	defer document.Close()

	clip, err := document.NewClip("A")
	if err != nil {
		t.Fatal(err)
	}
	metadata := clip.Metadata()
	if err := metadata.SetString("cmx_3600/reel", "ZZ100"); err != nil {
		t.Fatal(err)
	}
	if err := metadata.SetInt("take", 3); err != nil {
		t.Fatal(err)
	}

	reel, err := metadata.GetString("cmx_3600/reel")
	if err != nil {
		t.Fatal(err)
	}
	if reel != "ZZ100" {
		t.Fatalf("the reel is %q", reel)
	}
	take, err := metadata.GetInt("take")
	if err != nil {
		t.Fatal(err)
	}
	if take != 3 {
		t.Fatalf("the take is %d", take)
	}

	kind, err := metadata.Kind("take")
	if err != nil {
		t.Fatal(err)
	}
	if kind != otio.ValueKindInt {
		t.Fatalf("the take is held as a %v", kind)
	}

	present, err := metadata.Contains("nothing")
	if err != nil {
		t.Fatal(err)
	}
	if present {
		t.Fatal("the metadata claims to hold a key it was never given")
	}
}

func TestClearingChildrenHandsThemAllBack(t *testing.T) {
	document := otio.New()
	defer document.Close()

	track, err := document.NewTrack("V1", "Video")
	if err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"A", "B", "C", "D"} {
		clip, err := document.NewClip(name)
		if err != nil {
			t.Fatal(err)
		}
		if err := track.AppendChild(clip.Node); err != nil {
			t.Fatal(err)
		}
	}

	// This one empties as it answers, so the generated wrapper cannot ask
	// twice. If it did, it would come back with nothing.
	removed, err := track.ClearChildren()
	if err != nil {
		t.Fatal(err)
	}
	if len(removed) != 4 {
		t.Fatalf("clearing the track handed back %d children", len(removed))
	}
	for index, node := range removed {
		name, err := node.Name()
		if err != nil {
			t.Fatal(err)
		}
		if name != []string{"A", "B", "C", "D"}[index] {
			t.Fatalf("child %d is %q", index, name)
		}
	}

	count, err := track.ChildCount()
	if err != nil {
		t.Fatal(err)
	}
	if count != 0 {
		t.Fatalf("the track still holds %d children", count)
	}
}

func TestEveryChildAndItsRangeComeBackTogether(t *testing.T) {
	document := otio.New()
	defer document.Close()

	track, err := document.NewTrack("V1", "Video")
	if err != nil {
		t.Fatal(err)
	}
	for index := 0; index < 3; index++ {
		_ = index
		clip, err := document.NewClip("")
		if err != nil {
			t.Fatal(err)
		}
		span := otio.TimeRange{
			StartTime: otio.RationalTime{Value: 0, Rate: 24},
			Duration:  otio.RationalTime{Value: 10, Rate: 24},
		}
		if err := clip.SetSourceRange(span); err != nil {
			t.Fatal(err)
		}
		if err := track.AppendChild(clip.Node); err != nil {
			t.Fatal(err)
		}
	}

	// Two lists filled in step, which is one call and not two.
	children, ranges, err := track.RangesOfChildren()
	if err != nil {
		t.Fatal(err)
	}
	if len(children) != 3 || len(ranges) != 3 {
		t.Fatalf("%d children and %d ranges", len(children), len(ranges))
	}
	for index, span := range ranges {
		if span.StartTime.Value != float64(index*10) {
			t.Fatalf("child %d starts at %v", index, span.StartTime.Value)
		}
	}
}

func TestAnEditOperationChangesTheTimeline(t *testing.T) {
	document := otio.New()
	defer document.Close()

	track, err := document.NewTrack("V1", "Video")
	if err != nil {
		t.Fatal(err)
	}
	for index := 0; index < 3; index++ {
		clip, err := document.NewClip("")
		if err != nil {
			t.Fatal(err)
		}
		span := otio.TimeRange{
			StartTime: otio.RationalTime{Value: 0, Rate: 24},
			Duration:  otio.RationalTime{Value: 24, Rate: 24},
		}
		if err := clip.SetSourceRange(span); err != nil {
			t.Fatal(err)
		}
		if err := track.AppendChild(clip.Node); err != nil {
			t.Fatal(err)
		}
	}

	before, err := track.ChildCount()
	if err != nil {
		t.Fatal(err)
	}

	// Cut the second clip in two, which makes one more child than there was.
	if err := document.Slice(track.Node, otio.RationalTime{Value: 36, Rate: 24}, true); err != nil {
		t.Fatalf("slicing the track: %v", err)
	}

	after, err := track.ChildCount()
	if err != nil {
		t.Fatal(err)
	}
	if after != before+1 {
		t.Fatalf("slicing took the track from %d children to %d", before, after)
	}

	// The cut lands where it was asked to.
	span, err := track.RangeOfChildAtIndex(1)
	if err != nil {
		t.Fatal(err)
	}
	if span.StartTime.Value != 24 || span.Duration.Value != 12 {
		t.Fatalf("the second child is now %+v", span)
	}
}

func TestAFreshlyBuiltObjectIsEnabled(t *testing.T) {
	document := otio.New()
	defer document.Close()

	for name, build := range map[string]func(string) (bool, error){
		"clip": func(name string) (bool, error) {
			clip, err := document.NewClip(name)
			if err != nil {
				return false, err
			}
			return clip.Enabled()
		},
		"stack": func(name string) (bool, error) {
			stack, err := document.NewStack(name)
			if err != nil {
				return false, err
			}
			return stack.Enabled()
		},
		"track": func(name string) (bool, error) {
			track, err := document.NewTrack(name, "Video")
			if err != nil {
				return false, err
			}
			return track.Enabled()
		},
	} {
		enabled, err := build(name)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if !enabled {
			t.Errorf("a freshly built %s is not enabled", name)
		}
	}
}

func TestADocumentSurvivesARoundTripThroughJSON(t *testing.T) {
	document, err := otio.Open(screeningEDL)
	if err != nil {
		t.Fatal(err)
	}
	defer document.Close()

	text, err := document.ToJSON(2)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(text, "\"OTIO_SCHEMA\"") {
		t.Fatalf("the JSON does not look like OTIO: %.80s", text)
	}

	again, err := otio.FromJSON(text)
	if err != nil {
		t.Fatal(err)
	}
	defer again.Close()

	round, err := again.ToJSON(2)
	if err != nil {
		t.Fatal(err)
	}
	if round != text {
		t.Fatal("the document changed on the way through JSON")
	}
}

func TestSavingAndOpeningAgainKeepsTheClips(t *testing.T) {
	document, err := otio.Open(screeningEDL)
	if err != nil {
		t.Fatal(err)
	}
	defer document.Close()

	path := filepath.Join(t.TempDir(), "round-trip.otio")
	if err := document.Save(path); err != nil {
		t.Fatalf("saving: %v", err)
	}
	if _, err := os.Stat(path); err != nil {
		t.Fatalf("nothing was written: %v", err)
	}

	again, err := otio.Open(path)
	if err != nil {
		t.Fatalf("opening what was written: %v", err)
	}
	defer again.Close()

	root, err := again.Root()
	if err != nil {
		t.Fatal(err)
	}
	clips, err := root.FindClips()
	if err != nil {
		t.Fatal(err)
	}
	if len(clips) != 9 {
		t.Fatalf("the saved document holds %d clips", len(clips))
	}
}

func TestWritingBytesInEveryFormatTheLibraryKnows(t *testing.T) {
	document, err := otio.Open(screeningEDL)
	if err != nil {
		t.Fatal(err)
	}
	defer document.Close()

	for _, format := range []otio.Format{
		otio.FormatOTIOJSON,
		otio.FormatCMX3600,
		otio.FormatFcp7XML,
	} {
		written, err := document.WriteToBytes(format, nil)
		if err != nil {
			t.Fatalf("writing %v: %v", format, err)
		}
		if len(written) == 0 {
			t.Fatalf("writing %v produced nothing", format)
		}
	}
}

func TestAnEnumSaysWhatTheCInterfaceCallsIt(t *testing.T) {
	if got := otio.StatusNoValue.String(); got != "OTIO_STATUS_NO_VALUE" {
		t.Fatalf("StatusNoValue is spelled %q", got)
	}
	if got := otio.NodeKindClip.String(); got != "OTIO_NODE_KIND_CLIP" {
		t.Fatalf("NodeKindClip is spelled %q", got)
	}
	if got := otio.FormatCMX3600.Name(); got != "cmx_3600" {
		t.Fatalf("FormatCMX3600 is named %q", got)
	}
}

func TestAnObjectKnowsWhichSchemasItIs(t *testing.T) {
	document := otio.New()
	defer document.Close()

	clip, err := document.NewClip("A")
	if err != nil {
		t.Fatal(err)
	}
	for _, schema := range []otio.NodeKind{
		otio.NodeKindClip,
		otio.NodeKindItem,
		otio.NodeKindComposable,
		otio.NodeKindSerializableObjectWithMetadata,
		otio.NodeKindSerializableObject,
	} {
		if !clip.IsA(schema) {
			t.Fatalf("a clip says it is not a %v", schema)
		}
	}
	for _, schema := range []otio.NodeKind{otio.NodeKindTrack, otio.NodeKindGap} {
		if clip.IsA(schema) {
			t.Fatalf("a clip says it is a %v", schema)
		}
	}
}

// Absorb is the call that lets an object be built on its own and put into a
// timeline afterwards, which is the shape upstream's Python and C++ users
// expect. It is written by hand in the generator rather than emitted, so it
// needs a test of its own more than the mechanical calls do.
func TestAnObjectBuiltOnItsOwnCanJoinATimeline(t *testing.T) {
	timeline := otio.New()
	defer timeline.Close()

	track, err := timeline.NewTrack("V1", "Video")
	if err != nil {
		t.Fatal(err)
	}

	// A clip built somewhere else entirely, knowing nothing about the
	// timeline it is going to end up in.
	aside := otio.New()
	clip, err := aside.NewClip("Insert")
	if err != nil {
		t.Fatal(err)
	}
	rate := 24.0
	span := otio.TimeRange{
		StartTime: otio.RationalTime{Value: 0, Rate: rate},
		Duration:  otio.RationalTime{Value: 48, Rate: rate},
	}
	if err := clip.SetSourceRange(span); err != nil {
		t.Fatal(err)
	}

	translated, err := timeline.Absorb(aside)
	if err != nil {
		t.Fatalf("absorbing the clip's document: %v", err)
	}

	moved, ok := translated[clip.Node]
	if !ok {
		t.Fatal("the clip that moved is not in the translation")
	}
	if moved.Owner() != timeline {
		t.Fatal("the clip did not arrive in this document")
	}
	if err := track.AppendChild(moved); err != nil {
		t.Fatalf("appending the clip that moved: %v", err)
	}

	name, err := moved.Name()
	if err != nil {
		t.Fatal(err)
	}
	if name != "Insert" {
		t.Fatalf("the clip arrived named %q", name)
	}
	duration, err := track.Duration()
	if err != nil {
		t.Fatal(err)
	}
	if duration.Value != 48 || duration.Rate != rate {
		t.Fatalf("the track runs %v, not 48/24", duration)
	}

	// The source is gone: it was consumed, so the handle the caller still
	// holds into it fails rather than reaching freed memory.
	if _, err := clip.Name(); err == nil {
		t.Fatal("a handle into the consumed document still answers")
	}
	// Closing it again is harmless, which is what lets a deferred Close sit
	// beside every document whether or not it was absorbed.
	aside.Close()
}
