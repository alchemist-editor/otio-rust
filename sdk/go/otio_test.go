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
	"runtime"
	"strings"
	"sync"
	"testing"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
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
	root, err := otio.ReadFromFile(otio.FormatCMX3600, screeningEDL, nil)
	if err != nil {
		t.Fatalf("reading the EDL: %v", err)
	}
	defer root.Close()

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

// An AAF the Rust adapter's tests read, whose five clips each carry the
// MobID of the media they were cut from.
const coloredClipsAAF = "../../crates/otio-aaf/tests/data/colored_clips.aaf"

func TestAnAAFReadsAndWritesBackOut(t *testing.T) {
	root, err := otio.ReadFromFile(otio.FormatAAF, coloredClipsAAF, nil)
	if err != nil {
		t.Fatalf("reading the AAF: %v", err)
	}
	defer root.Close()
	clips, err := root.FindClips()
	if err != nil {
		t.Fatalf("finding the clips: %v", err)
	}
	if len(clips) != 5 {
		t.Fatalf("expected 5 clips, found %d", len(clips))
	}

	// A cut read from an AAF keeps each clip's MobID, so it writes back out
	// with no leave to make any up.
	written, err := otio.WriteToBytes(otio.FormatAAF, root, nil)
	if err != nil {
		t.Fatalf("writing the AAF: %v", err)
	}
	again, err := otio.ReadFromBytes(otio.FormatAAF, written, nil)
	if err != nil {
		t.Fatalf("reading the written AAF: %v", err)
	}
	defer again.Close()
	if clips, _ := again.FindClips(); len(clips) != 5 {
		t.Fatalf("expected 5 clips after writing, found %d", len(clips))
	}

	// A fixed time and seed write the same file twice.
	fixed := &otio.WriteOptions{AAFTime: 1714979289, AAFIDSeed: 59}
	first, err := otio.WriteToBytes(otio.FormatAAF, root, fixed)
	if err != nil {
		t.Fatal(err)
	}
	second, err := otio.WriteToBytes(otio.FormatAAF, root, fixed)
	if err != nil {
		t.Fatal(err)
	}
	if string(first) != string(second) {
		t.Fatal("two writes with the same time and seed differ")
	}
}

func TestAnAAFReadsWithEachOfUpstreamsOptions(t *testing.T) {
	for _, options := range []otio.ReadOptions{
		{AAFKeepNesting: true},
		{AAFMarkersOnSlots: true},
		{AAFBakeKeyframes: true},
	} {
		root, err := otio.ReadFromFile(otio.FormatAAF, coloredClipsAAF, &options)
		if err != nil {
			t.Fatalf("reading with %+v: %v", options, err)
		}
		clips, _ := root.FindClips()
		root.Close()
		if len(clips) != 5 {
			t.Fatalf("reading with %+v found %d clips", options, len(clips))
		}
	}
}

func TestOpenWorksOutTheFormatFromTheName(t *testing.T) {
	root, err := otio.Open(screeningEDL)
	if err != nil {
		t.Fatalf("opening the EDL: %v", err)
	}
	defer root.Close()

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
	timeline, err := otio.NewTimeline("Cut")
	if err != nil {
		t.Fatal(err)
	}
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

	rate := 24.0
	for _, name := range []string{"A", "B", "C"} {
		clip, err := otio.NewClip(name)
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
	clip, err := otio.NewClip("A")
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

func TestEveryFailureCarriesItsOwnMessageWhateverThreadItRanOn(t *testing.T) {
	// The library hands each call's message back with its status, so nothing
	// here holds a goroutine to its OS thread. Many goroutines failing in two
	// different ways at once, and yielding between the call and the check,
	// must each still read the sentence their own call wrote.
	clip, err := otio.NewClip("A")
	if err != nil {
		t.Fatal(err)
	}
	track := otio.Track{Composition: otio.Composition{Item: clip.Item}}

	var group sync.WaitGroup
	failures := make(chan string, 400)
	for index := 0; index < 200; index++ {
		group.Add(2)
		go func() {
			defer group.Done()
			_, err := otio.RationalTimeFromTimecode("nonsense", 24)
			runtime.Gosched()
			var failure *otio.Error
			if !errors.As(err, &failure) || failure.Status != otio.StatusTimeError ||
				strings.Contains(failure.Message, "not a track") || failure.Message == "" {
				failures <- "timecode: " + err.Error()
			}
		}()
		go func() {
			defer group.Done()
			_, err := track.Kind()
			runtime.Gosched()
			var failure *otio.Error
			if !errors.As(err, &failure) || failure.Status != otio.StatusCoreError ||
				!strings.Contains(failure.Message, "not a track") {
				failures <- "track kind: " + err.Error()
			}
		}()
	}
	group.Wait()
	close(failures)
	for failure := range failures {
		t.Error(failure)
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
	clip, err := otio.NewClip("doomed")
	if err != nil {
		t.Fatal(err)
	}
	if err := clip.Remove(); err != nil {
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
	clip, err := otio.NewClip("untrimmed")
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
	clip, err := otio.NewClip("A")
	if err != nil {
		t.Fatal(err)
	}
	metadata := clip.Metadata()
	// A path separates its steps with dots, so this writes a reel inside a
	// cmx_3600 dictionary rather than one key with a funny name. The path is
	// followed rather than created, so the dictionary has to exist first.
	if err := metadata.SetDictionary("cmx_3600"); err != nil {
		t.Fatal(err)
	}
	if err := metadata.SetString("cmx_3600.reel", "ZZ100"); err != nil {
		t.Fatal(err)
	}
	if err := metadata.SetInt("take", 3); err != nil {
		t.Fatal(err)
	}

	reel, err := metadata.GetString("cmx_3600.reel")
	if err != nil {
		t.Fatal(err)
	}
	if reel != "ZZ100" {
		t.Fatalf("the reel is %q", reel)
	}
	// Reading it back by its steps is not enough on its own: a single key
	// literally named "cmx_3600.reel" would answer the same. What proves the
	// dictionary is really nested is that cmx_3600 is a dictionary of one.
	nested, err := metadata.Kind("cmx_3600")
	if err != nil {
		t.Fatal(err)
	}
	if nested != otio.ValueKindDictionary {
		t.Fatalf("cmx_3600 is held as a %v, not a dictionary", nested)
	}
	inside, err := metadata.Len("cmx_3600")
	if err != nil {
		t.Fatal(err)
	}
	if inside != 1 {
		t.Fatalf("the cmx_3600 dictionary holds %d entries", inside)
	}
	if key, err := metadata.KeyAt("cmx_3600", 0); err != nil {
		t.Fatal(err)
	} else if key != "reel" {
		t.Fatalf("the entry inside cmx_3600 is named %q", key)
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
	track, err := otio.NewTrack("V1", "Video")
	if err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"A", "B", "C", "D"} {
		clip, err := otio.NewClip(name)
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
	track, err := otio.NewTrack("V1", "Video")
	if err != nil {
		t.Fatal(err)
	}
	for index := 0; index < 3; index++ {
		_ = index
		clip, err := otio.NewClip("")
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
	track, err := otio.NewTrack("V1", "Video")
	if err != nil {
		t.Fatal(err)
	}
	for index := 0; index < 3; index++ {
		clip, err := otio.NewClip("")
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
	if err := otio.Slice(track.Node, otio.RationalTime{Value: 36, Rate: 24}, true); err != nil {
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
	for name, build := range map[string]func(string) (bool, error){
		"clip": func(name string) (bool, error) {
			clip, err := otio.NewClip(name)
			if err != nil {
				return false, err
			}
			return clip.Enabled()
		},
		"stack": func(name string) (bool, error) {
			stack, err := otio.NewStack(name)
			if err != nil {
				return false, err
			}
			return stack.Enabled()
		},
		"track": func(name string) (bool, error) {
			track, err := otio.NewTrack(name, "Video")
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
	root, err := otio.Open(screeningEDL)
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()

	text, err := root.ToJSON(2)
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
	root, err := otio.Open(screeningEDL)
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()

	path := filepath.Join(t.TempDir(), "round-trip.otio")
	if err := otio.Save(root, path); err != nil {
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

	clips, err := again.FindClips()
	if err != nil {
		t.Fatal(err)
	}
	if len(clips) != 9 {
		t.Fatalf("the saved document holds %d clips", len(clips))
	}
}

func TestWritingBytesInEveryFormatTheLibraryKnows(t *testing.T) {
	root, err := otio.Open(screeningEDL)
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()

	for _, format := range []otio.Format{
		otio.FormatOTIOJSON,
		otio.FormatCMX3600,
		otio.FormatFcp7XML,
	} {
		written, err := otio.WriteToBytes(format, root, nil)
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
	if got := otio.FormatAAF.Name(); got != "AAF" {
		t.Fatalf("FormatAAF is named %q", got)
	}
}

func TestAnObjectKnowsWhichSchemasItIs(t *testing.T) {
	clip, err := otio.NewClip("A")
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

// Building an object on its own and putting it into a timeline afterwards is
// the shape upstream's Python and C++ users expect, and the reason this
// package hides the arena at all. Underneath, the clip starts in an arena of
// its own and moves into the track's when it is appended; nothing here says
// so, which is the point.
func TestAnObjectBuiltOnItsOwnCanJoinATimeline(t *testing.T) {
	track, err := otio.NewTrack("V1", "Video")
	if err != nil {
		t.Fatal(err)
	}
	defer track.Close()

	// A clip built somewhere else entirely, knowing nothing about the
	// timeline it is going to end up in.
	clip, err := otio.NewClip("Insert")
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

	if err := track.AppendChild(clip.Node); err != nil {
		t.Fatalf("appending a clip built on its own: %v", err)
	}

	// The handle the caller has held all along still names the clip, which
	// is what moving it had to preserve: it was reissued on the way over.
	name, err := clip.Name()
	if err != nil {
		t.Fatalf("the clip that moved is unreadable: %v", err)
	}
	if name != "Insert" {
		t.Fatalf("the clip arrived named %q", name)
	}
	child, err := track.ChildAt(0)
	if err != nil {
		t.Fatal(err)
	}
	if !child.Equals(clip.Node) {
		t.Fatal("the track's child is not the clip that was appended")
	}
	duration, err := track.Duration()
	if err != nil {
		t.Fatal(err)
	}
	if duration.Value != 48 || duration.Rate != rate {
		t.Fatalf("the track runs %v, not 48/24", duration)
	}

	// And now that it is in, naming it is no longer naming a stranger.
	index, err := track.IndexOfChild(clip.Node)
	if err != nil {
		t.Fatal(err)
	}
	if index != 0 {
		t.Fatalf("the clip is at index %d", index)
	}
	if err := track.DetachChild(clip.Node); err != nil {
		t.Fatalf("detaching the track's own child: %v", err)
	}
}

// The edit operations are the other half of the same story: they are handed
// an item that has never been anywhere and a composition that is already
// somewhere, and the call has to be made where the composition is. Anchoring
// on the item instead — which the TypeScript SDK did until the description
// started saying which object a call is made in — refuses the track.
func TestAnEditPutsANewlyBuiltItemIntoATrack(t *testing.T) {
	rate := 24.0
	span := func(start, length float64) otio.TimeRange {
		return otio.TimeRange{
			StartTime: otio.RationalTime{Value: start, Rate: rate},
			Duration:  otio.RationalTime{Value: length, Rate: rate},
		}
	}
	shot := func(name string) otio.Clip {
		t.Helper()
		clip, err := otio.NewClip(name)
		if err != nil {
			t.Fatal(err)
		}
		if err := clip.SetSourceRange(span(0, 24)); err != nil {
			t.Fatal(err)
		}
		return clip
	}

	track, err := otio.NewTrack("V1", "Video")
	if err != nil {
		t.Fatal(err)
	}
	defer track.Close()
	if err := track.AppendChild(shot("shot_01").Node); err != nil {
		t.Fatal(err)
	}

	second := shot("shot_02")
	if err := otio.Insert(second.Node, track.Node, otio.RationalTime{Value: 24, Rate: rate}, false, nil); err != nil {
		t.Fatalf("inserting a clip built on its own: %v", err)
	}
	third := shot("shot_03")
	if err := otio.Overwrite(third.Node, track.Node, span(0, 24), false, nil); err != nil {
		t.Fatalf("overwriting with a clip built on its own: %v", err)
	}

	count, err := track.ChildCount()
	if err != nil {
		t.Fatal(err)
	}
	if count != 2 {
		t.Fatalf("the track holds %d children", count)
	}
	for index, want := range []string{"shot_03", "shot_02"} {
		child, err := track.ChildAt(index)
		if err != nil {
			t.Fatal(err)
		}
		name, err := child.Name()
		if err != nil {
			t.Fatal(err)
		}
		if name != want {
			t.Fatalf("child %d is %q, not %q", index, name, want)
		}
	}
}

// A handle is an index into one arena, and two timelines issue the same
// indices, so an object from one would resolve to an unrelated object in the
// other rather than failing. A call that only names an object therefore has
// to refuse one from elsewhere — and refuse it before asking the library,
// because absorbing first and failing afterwards would already have merged
// the two timelines.
func TestAnObjectFromAnotherTimelineIsRefused(t *testing.T) {
	track, err := otio.NewTrack("V1", "Video")
	if err != nil {
		t.Fatal(err)
	}
	mine, err := otio.NewClip("Mine")
	if err != nil {
		t.Fatal(err)
	}
	if err := track.AppendChild(mine.Node); err != nil {
		t.Fatal(err)
	}

	elsewhere, err := otio.NewTrack("V2", "Video")
	if err != nil {
		t.Fatal(err)
	}
	defer elsewhere.Close()
	theirs, err := otio.NewClip("Theirs")
	if err != nil {
		t.Fatal(err)
	}
	if err := elsewhere.AppendChild(theirs.Node); err != nil {
		t.Fatal(err)
	}

	for what, body := range map[string]func() error{
		"DetachChild":   func() error { return track.DetachChild(theirs.Node) },
		"NeighborsOf":   func() error { _, _, err := track.NeighborsOf(theirs.Node, otio.NeighborGapPolicyNever); return err },
		"FlattenTracks": func() error { _, err := otio.FlattenTracks([]otio.Node{track.Node, elsewhere.Node}); return err },
		"IndexOfChild":  func() error { _, err := track.IndexOfChild(theirs.Node); return err },
		"RangeOfChild":  func() error { _, err := track.RangeOfChild(theirs.Node); return err },
	} {
		err := body()
		if err == nil {
			t.Fatalf("%s took an object from another timeline", what)
		}
		if !errors.Is(err, otio.ErrOtherTimeline) {
			t.Fatalf("%s refused with %v, not ErrOtherTimeline", what, err)
		}
	}

	// What the refusal is protecting, and the only assertion that tells a
	// refusal apart from an absorb that failed afterwards: the two timelines
	// are still independent, so releasing this one leaves the other whole.
	track.Close()
	if count, err := elsewhere.ChildCount(); err != nil {
		t.Fatalf("the other timeline after this one was released: %v", err)
	} else if count != 1 {
		t.Fatalf("the other track holds %d children", count)
	}
	if name, err := theirs.Name(); err != nil {
		t.Fatalf("the other timeline's clip after this one was released: %v", err)
	} else if name != "Theirs" {
		t.Fatalf("the other timeline's clip is named %q", name)
	}
}

// Nothing means nothing, wherever an object is optional: the check on an
// object argument must not turn a nil into a stray handle.
func TestAnOptionalObjectLeftOutIsStillNothing(t *testing.T) {
	timeline, err := otio.NewTimeline("Cut")
	if err != nil {
		t.Fatal(err)
	}
	defer timeline.Close()
	if err := timeline.SetTracks(nil); err != nil {
		t.Fatalf("clearing the tracks with nil: %v", err)
	}

	track, err := otio.NewTrack("V1", "Video")
	if err != nil {
		t.Fatal(err)
	}
	clip, err := otio.NewClip("A")
	if err != nil {
		t.Fatal(err)
	}
	if err := clip.SetSourceRange(otio.TimeRange{
		StartTime: otio.RationalTime{Value: 0, Rate: 24},
		Duration:  otio.RationalTime{Value: 24, Rate: 24},
	}); err != nil {
		t.Fatal(err)
	}
	if err := track.AppendChild(clip.Node); err != nil {
		t.Fatal(err)
	}
	if err := otio.Remove(track.Node, otio.RationalTime{Value: 0, Rate: 24}, false, nil); err != nil {
		t.Fatalf("removing with no fill template: %v", err)
	}
	if count, err := track.ChildCount(); err != nil {
		t.Fatal(err)
	} else if count != 0 {
		t.Fatalf("the track still holds %d children", count)
	}
	track.Close()
}

// Upstream's Timeline() builds an empty stack named "tracks" in its
// constructor, so a caller can append to a fresh timeline's tracks without
// making one first. A timeline from here arrives the same way.
func TestANewTimelineArrivesWithItsTracks(t *testing.T) {
	timeline, err := otio.NewTimeline("Cut")
	if err != nil {
		t.Fatal(err)
	}

	tracks, err := timeline.Tracks()
	if err != nil {
		t.Fatalf("a fresh timeline has no tracks: %v", err)
	}
	stack, ok := tracks.AsStack()
	if !ok {
		kind, _ := tracks.SchemaKind()
		t.Fatalf("the tracks are held as a %v", kind)
	}
	if name, err := stack.Name(); err != nil {
		t.Fatal(err)
	} else if name != "tracks" {
		t.Fatalf("the stack is named %q", name)
	}
	if owner, err := stack.Parent(); err != nil {
		t.Fatal(err)
	} else if !owner.Equals(timeline.Node) {
		t.Fatal("the stack does not belong to the timeline")
	}

	// Appending straight to it works, which is the point of building it.
	track, err := otio.NewTrack("V1", "Video")
	if err != nil {
		t.Fatal(err)
	}
	if err := stack.AppendChild(track.Node); err != nil {
		t.Fatalf("appending to a fresh timeline's tracks: %v", err)
	}
	if count, err := stack.ChildCount(); err != nil {
		t.Fatal(err)
	} else if count != 1 {
		t.Fatalf("the stack holds %d children", count)
	}
}

// Replacing a timeline's tracks leaves the old stack in the document rather
// than destroying it, and stops it claiming a timeline that has disowned it.
func TestReplacingTheTracksLeavesTheOldStackParentless(t *testing.T) {
	timeline, err := otio.NewTimeline("Cut")
	if err != nil {
		t.Fatal(err)
	}
	original, err := timeline.Tracks()
	if err != nil {
		t.Fatal(err)
	}

	replacement, err := otio.NewStack("mine")
	if err != nil {
		t.Fatal(err)
	}
	if err := timeline.SetTracks(&replacement.Node); err != nil {
		t.Fatal(err)
	}

	// The displaced stack is still there and still usable, so a caller who
	// kept hold of it can put it somewhere else.
	displaced, ok := original.AsStack()
	if !ok {
		t.Fatal("the displaced tracks are no longer a stack")
	}
	if name, err := displaced.Name(); err != nil {
		t.Fatalf("the displaced stack is unreadable: %v", err)
	} else if name != "tracks" {
		t.Fatalf("the displaced stack is named %q", name)
	}

	// But it no longer belongs to the timeline, which now holds another one.
	if owner, err := displaced.Parent(); err == nil {
		t.Fatalf("the displaced stack still claims parent %v", owner)
	} else if !errors.Is(err, otio.ErrNoValue) {
		t.Fatalf("asking the displaced stack for its parent: %v", err)
	}
	if owner, err := replacement.Parent(); err != nil {
		t.Fatal(err)
	} else if !owner.Equals(timeline.Node) {
		t.Fatal("the replacement does not belong to the timeline")
	}
}

// Upstream's setter leaves an empty stack rather than nothing, and its own
// test_timeline.py asserts that tl.tracks is still a Stack afterwards.
func TestClearingTheTracksLeavesAnEmptyStack(t *testing.T) {
	timeline, err := otio.NewTimeline("Cut")
	if err != nil {
		t.Fatal(err)
	}
	if err := timeline.SetTracks(nil); err != nil {
		t.Fatal(err)
	}

	tracks, err := timeline.Tracks()
	if err != nil {
		t.Fatalf("a timeline whose tracks were cleared has none: %v", err)
	}
	stack, ok := tracks.AsStack()
	if !ok {
		t.Fatal("the tracks are not a stack")
	}
	if count, err := stack.ChildCount(); err != nil {
		t.Fatal(err)
	} else if count != 0 {
		t.Fatalf("the fresh stack holds %d children", count)
	}

	// And it is usable straight away, like the one a new timeline arrives with.
	track, err := otio.NewTrack("V1", "Video")
	if err != nil {
		t.Fatal(err)
	}
	if err := stack.AppendChild(track.Node); err != nil {
		t.Fatalf("appending to the replacement stack: %v", err)
	}
}
