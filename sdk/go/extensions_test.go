// Tests for turning on extensions, which are otio-rust's own work and not
// part of OpenTimelineIO.
//
// Enabling is process-wide and cannot be undone, so nothing here asserts that
// an extension starts off: another test in the same binary may have enabled it.

package otio_test

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
	"github.com/alchemist-editor/otio-rust/sdk/go/ext/sync"
)

func TestSyncExtensionEnables(t *testing.T) {
	if err := sync.Enable(); err != nil {
		t.Fatal(err)
	}

	on, err := sync.IsEnabled()
	if err != nil {
		t.Fatal(err)
	}
	if !on {
		t.Error("sync.IsEnabled is false after sync.Enable")
	}
	on, err = otio.IsExtensionEnabled("sync")
	if err != nil {
		t.Fatal(err)
	}
	if !on {
		t.Error(`otio.IsExtensionEnabled("sync") is false after sync.Enable`)
	}

	err = otio.EnableExtension("no-such-extension")
	var failure *otio.Error
	if !errors.As(err, &failure) {
		t.Fatalf("enabling an unknown extension returned %v, not an *otio.Error", err)
	}
	if !strings.Contains(failure.Message, "no-such-extension") {
		t.Errorf("the error %q does not name the extension", failure.Message)
	}
}

// The cut the timeline sync samples use, as an .otio file would hold it.
// Nothing in it has a GUID yet.
const syncCut = `{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "cut",
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "Clip.2", "name": "A"},
      {"OTIO_SCHEMA": "Clip.2", "name": "B"}]}]}
}`

// enableSync turns sync on for a test, which may run alone.
func enableSync(t *testing.T) {
	t.Helper()
	if err := sync.Enable(); err != nil {
		t.Fatal(err)
	}
}

// openTimeline reads a timeline, failing the test if it cannot.
func openTimeline(t *testing.T, json string) otio.Node {
	t.Helper()
	timeline, err := otio.FromJSON(json)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(timeline.Close)
	return timeline
}

// nameOf answers an object's name, failing the test if it cannot.
func nameOf(t *testing.T, node otio.Node) string {
	t.Helper()
	name, err := node.Name()
	if err != nil {
		t.Fatal(err)
	}
	return name
}

// sameJSON says whether two JSON texts hold the same value, numbers compared
// by value.
func sameJSON(t *testing.T, left, right string) bool {
	t.Helper()
	var one, other any
	for _, side := range []struct {
		text string
		into *any
	}{{left, &one}, {right, &other}} {
		decoder := json.NewDecoder(strings.NewReader(side.text))
		decoder.UseNumber()
		if err := decoder.Decode(side.into); err != nil {
			t.Fatal(err)
		}
	}
	return reflect.DeepEqual(normalized(one), normalized(other))
}

// normalized turns every json.Number into a float64, so 1 and 1.0 compare
// equal.
func normalized(value any) any {
	switch value := value.(type) {
	case json.Number:
		number, err := value.Float64()
		if err != nil {
			return value.String()
		}
		return number
	case map[string]any:
		for key, inner := range value {
			value[key] = normalized(inner)
		}
	case []any:
		for index, inner := range value {
			value[index] = normalized(inner)
		}
	}
	return value
}

// Furnace's golden corpus: diffing every case gives its patches byte for
// byte, the stats say which cases fell back, and applying the patches to the
// old timeline gives the new one.
func TestSyncGoldenCorpus(t *testing.T) {
	enableSync(t)
	fixtures := filepath.Join("..", "..", "extensions", "sync", "otio-ext-sync", "tests", "fixtures", "otiodiff")
	entries, err := os.ReadDir(fixtures)
	if err != nil {
		t.Fatal(err)
	}
	fellBack := map[string]bool{
		"generation_regression":            true,
		"guidless_transition":              true,
		"metadata_sync_threshold_fallback": true,
		"sequence_changed":                 true,
	}
	read := func(path string) string {
		t.Helper()
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		return string(data)
	}
	cases := 0
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		cases++
		name := entry.Name()
		directory := filepath.Join(fixtures, name)
		oldText := read(filepath.Join(directory, "old.json"))
		newText := read(filepath.Join(directory, "new.json"))

		patches, err := sync.Diff(oldText, newText, true)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if want := read(filepath.Join(directory, "patches.json")); patches+"\n" != want {
			t.Errorf("%s: the patches differ from patches.json:\n%s", name, patches)
		}

		stats, err := sync.DiffStats(oldText, newText)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if want := fmt.Sprintf(`"fellBack":%t`, fellBack[name]); !strings.Contains(stats, want) {
			t.Errorf("%s: the stats %s do not say %s", name, stats, want)
		}

		var oldSnapshot, newSnapshot struct {
			OtioJSON json.RawMessage `json:"otioJson"`
		}
		if err := json.Unmarshal([]byte(oldText), &oldSnapshot); err != nil {
			t.Fatal(err)
		}
		if err := json.Unmarshal([]byte(newText), &newSnapshot); err != nil {
			t.Fatal(err)
		}
		compact, err := sync.Diff(oldText, newText, false)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		applied, err := sync.Apply(string(oldSnapshot.OtioJSON), compact)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if !sameJSON(t, applied, string(newSnapshot.OtioJSON)) {
			t.Errorf("%s: applying the patches does not give the new timeline:\n%s", name, applied)
		}
	}
	if cases != 11 {
		t.Errorf("ran %d cases of the golden corpus, not 11", cases)
	}
}

// A transaction's patches carry an edit to another copy of the timeline, in
// place: the object the other copy held is the one that changed.
func TestSyncTransactionCarriesAnEdit(t *testing.T) {
	enableSync(t)
	editor := openTimeline(t, syncCut)
	before, err := sync.Begin(editor, "main", 1)
	if err != nil {
		t.Fatal(err)
	}
	stamped, err := editor.ToJSON(0)
	if err != nil {
		t.Fatal(err)
	}
	viewer := openTimeline(t, stamped)

	if err := editor.SetName("fine cut"); err != nil {
		t.Fatal(err)
	}
	patches, err := sync.Commit(editor, before)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(patches, `"path":"/name"`) {
		t.Errorf("the patches do not rename the timeline: %s", patches)
	}
	if err := sync.ApplyTo(viewer, patches); err != nil {
		t.Fatal(err)
	}
	if name := nameOf(t, viewer); name != "fine cut" {
		t.Errorf("the viewer's timeline is called %q after applying, not \"fine cut\"", name)
	}

	// Both copies name the timeline by the same GUID, and stamping finds
	// nothing left to do.
	guid, err := sync.Guid(editor)
	if err != nil {
		t.Fatal(err)
	}
	if guid == "" {
		t.Fatal("the timeline has an empty GUID after Begin")
	}
	found, err := sync.Find(viewer, guid)
	if err != nil {
		t.Fatal(err)
	}
	if found != viewer {
		t.Error("Find by the timeline's GUID does not answer the viewer's timeline")
	}
	if _, ok := found.AsTimeline(); !ok {
		t.Error("what Find answered is not a timeline")
	}
	count, err := sync.Stamp(viewer)
	if err != nil {
		t.Fatal(err)
	}
	if count != 0 {
		t.Errorf("stamping a stamped timeline gave %d objects a GUID", count)
	}
	if _, err := sync.Find(viewer, "nobody"); !errors.Is(err, otio.ErrNoValue) {
		t.Errorf("Find with an unknown GUID returned %v, not ErrNoValue", err)
	}
	fresh, err := sync.NewGuid()
	if err != nil {
		t.Fatal(err)
	}
	if fresh == "" {
		t.Error("NewGuid answered an empty GUID")
	}
}

// Rolling back undoes a transaction's edits, and the journal reports edits
// once each.
func TestSyncRollbackAndJournal(t *testing.T) {
	enableSync(t)
	timeline := openTimeline(t, syncCut)
	before, err := sync.Begin(timeline, "main", 1)
	if err != nil {
		t.Fatal(err)
	}
	if err := timeline.SetName("scrapped"); err != nil {
		t.Fatal(err)
	}
	if err := sync.Rollback(timeline, before); err != nil {
		t.Fatal(err)
	}
	if name := nameOf(t, timeline); name != "cut" {
		t.Errorf("the timeline is called %q after rolling back, not \"cut\"", name)
	}

	changed := func() bool {
		t.Helper()
		changed, err := sync.CommitJournal(timeline)
		if err != nil {
			t.Fatal(err)
		}
		return changed
	}
	if err := sync.StartJournal(timeline); err != nil {
		t.Fatal(err)
	}
	if changed() {
		t.Error("the journal reports a change before any edit")
	}
	if err := timeline.SetName("journalled"); err != nil {
		t.Fatal(err)
	}
	if !changed() {
		t.Error("the journal does not report a rename")
	}
	if changed() {
		t.Error("the journal reports the rename twice; a commit starts a fresh record")
	}
	if err := sync.StopJournal(timeline); err != nil {
		t.Fatal(err)
	}
}

// Before sync is enabled every call but Enable and IsEnabled is refused.
// Enabling is process-wide and cannot be undone, so this runs the check in a
// fresh copy of the test binary, where nothing has enabled it yet.
func TestSyncRefusedUntilEnabled(t *testing.T) {
	const child = "OTIO_GO_SYNC_REFUSAL_CHILD"
	if os.Getenv(child) == "1" {
		_, err := sync.NewGuid()
		var failure *otio.Error
		if !errors.As(err, &failure) || failure.Status != otio.StatusPluginError {
			t.Fatalf("NewGuid before Enable returned %v, not a plugin error", err)
		}
		if !strings.Contains(failure.Message, "otio_ext_sync_enable") {
			t.Errorf("the refusal %q does not say how to enable sync", failure.Message)
		}
		return
	}
	command := exec.Command(os.Args[0], "-test.run=^TestSyncRefusedUntilEnabled$", "-test.count=1")
	command.Env = append(os.Environ(), child+"=1")
	if output, err := command.CombinedOutput(); err != nil {
		t.Fatalf("the check in a fresh process failed: %v\n%s", err, output)
	}
}

// What the server sends a viewer joining a session whose host shared one
// timeline.
const syncViewerWelcome = `{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","role":"viewer","roster":[{"id":"p_1","displayName":"Host","role":"host","joinedAt":0,"connectionState":"connected","muted":false}],"session":{"id":"s_1","createdAt":0},"snapshot":{"sequenceId":"main","generation":1,"otioJson":{"OTIO_SCHEMA":"Timeline.1","name":"Coastline","metadata":{"sync":{"guid":"t-1"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"s-1"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"v-1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Waves","metadata":{"sync":{"guid":"c-1"}}}]}]}}}}}`

// What the server sends the host of a new session.
const syncHostWelcome = `{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_1","role":"host","roster":[],"session":{"id":"s_1","createdAt":0}}}`

// newSession makes a session from settings, failing the test if it cannot.
func newSession(t *testing.T, settings string) uint64 {
	t.Helper()
	session, err := sync.SessionNew(settings)
	if err != nil {
		t.Fatal(err)
	}
	return session
}

// nextFrame answers the next frame a session has to send and whether it is
// binary, or ok false when there is none.
func nextFrame(t *testing.T, session uint64) (frame string, binary bool, ok bool) {
	t.Helper()
	data, binary, err := sync.SessionPollTransmit(session, 0)
	if errors.Is(err, otio.ErrNoValue) {
		return "", false, false
	}
	if err != nil {
		t.Fatal(err)
	}
	return string(data), binary, true
}

// drainFrames answers every frame a session has to send, as text.
func drainFrames(t *testing.T, session uint64) []string {
	t.Helper()
	var frames []string
	for {
		frame, _, ok := nextFrame(t, session)
		if !ok {
			return frames
		}
		frames = append(frames, frame)
	}
}

// drainEvents answers every event a session has waiting.
func drainEvents(t *testing.T, session uint64) []string {
	t.Helper()
	var events []string
	for {
		event, err := sync.SessionPollEvent(session)
		if errors.Is(err, otio.ErrNoValue) {
			return events
		}
		if err != nil {
			t.Fatal(err)
		}
		events = append(events, event)
	}
}

// handled passes an event to Handle on timeline and answers what it did.
func handled(t *testing.T, timeline otio.Node, event string) string {
	t.Helper()
	result, err := sync.Handle(timeline, event)
	if err != nil {
		t.Fatal(err)
	}
	return result
}

// A viewer joins, receives the host's timeline into its own copy, and sends
// its playhead.
func TestSyncSessionViewerJoins(t *testing.T) {
	enableSync(t)
	session := newSession(t, `{"token":"join-code","identity":{"displayName":"Ada"},"encodings":[]}`)

	if _, _, ok := nextFrame(t, session); ok {
		t.Error("a session has a frame to send before it connects")
	}
	if err := sync.SessionConnect(session); err != nil {
		t.Fatal(err)
	}
	hello, binary, ok := nextFrame(t, session)
	if !ok {
		t.Fatal("a connected session has no hello to send")
	}
	if binary {
		t.Error("the hello is a binary frame, not a text one")
	}
	if !strings.Contains(hello, `"type":"hello"`) || !strings.Contains(hello, "Ada") {
		t.Errorf("the first frame is not a hello from Ada: %s", hello)
	}
	if frame, _, ok := nextFrame(t, session); ok {
		t.Errorf("a frame follows the hello: %s", frame)
	}

	if err := sync.SessionReceive(session, []byte(syncViewerWelcome), false); err != nil {
		t.Fatal(err)
	}
	events := drainEvents(t, session)
	if len(events) != 2 {
		t.Fatalf("the welcome gave %d events, not 2: %v", len(events), events)
	}
	if !strings.Contains(events[0], `"type":"timeline"`) || !strings.Contains(events[0], `"joining":true`) {
		t.Errorf("the first event is not the joining timeline: %s", events[0])
	}
	if !strings.Contains(events[1], `"type":"joined"`) {
		t.Errorf("the second event is not joined: %s", events[1])
	}

	timeline := openTimeline(t, `{"OTIO_SCHEMA":"Timeline.1"}`)
	if result := handled(t, timeline, events[0]); result != `{"handled":"applied"}` {
		t.Errorf("handling the timeline event gave %s", result)
	}
	if name := nameOf(t, timeline); name != "Coastline" {
		t.Errorf("the timeline is called %q after the timeline event, not \"Coastline\"", name)
	}
	asTimeline, ok := timeline.AsTimeline()
	if !ok {
		t.Fatal("the timeline is not a timeline")
	}
	stack, err := asTimeline.Tracks()
	if err != nil {
		t.Fatal(err)
	}
	tracks, err := stack.Children()
	if err != nil {
		t.Fatal(err)
	}
	if len(tracks) != 1 {
		t.Fatalf("the timeline has %d tracks, not 1", len(tracks))
	}
	clips, err := tracks[0].Children()
	if err != nil {
		t.Fatal(err)
	}
	if len(clips) != 1 || nameOf(t, clips[0]) != "Waves" {
		t.Errorf("the track does not hold just the clip Waves: %d children", len(clips))
	}
	if result := handled(t, timeline, events[1]); result != `{"handled":"joined"}` {
		t.Errorf("handling the joined event gave %s", result)
	}

	state, err := sync.SessionState(session)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(state, `"role":"viewer"`) || !strings.Contains(state, `"joined":true`) {
		t.Errorf("the state does not say a joined viewer: %s", state)
	}
	snapshot, err := sync.SessionTimeline(session, "main")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(snapshot, `"sequenceId":"main"`) {
		t.Errorf("the snapshot of main does not name it: %s", snapshot)
	}
	if _, err := sync.SessionTimeline(session, "other"); !errors.Is(err, otio.ErrNoValue) {
		t.Errorf("the timeline of an unknown sequence returned %v, not ErrNoValue", err)
	}

	may, err := sync.SessionMaySend(session, "timeline")
	if err != nil {
		t.Fatal(err)
	}
	if may {
		t.Error("a viewer may send on the timeline channel")
	}
	_, err = sync.SessionPublish(session, timeline, "main")
	var failure *otio.Error
	if !errors.As(err, &failure) || failure.Status != otio.StatusInvalidArgument {
		t.Errorf("a viewer publishing returned %v, not an invalid argument", err)
	}

	if err := sync.SessionSetPlayhead(session, "main", 12, true, 1); err != nil {
		t.Fatal(err)
	}
	frames := drainFrames(t, session)
	if len(frames) == 0 || !strings.Contains(strings.Join(frames, "\n"), `"type":"presence"`) {
		t.Errorf("moving the playhead sent no presence: %v", frames)
	}

	if err := sync.SessionClose(session); err != nil {
		t.Fatal(err)
	}
	if err := sync.SessionClose(session); err != nil {
		t.Errorf("closing a closed session returned %v", err)
	}
	if _, err := sync.SessionState(session); err == nil {
		t.Error("a closed session still answers its state")
	}
}

// A host publishes a timeline, and then only what changed in it.
func TestSyncSessionHostPublishes(t *testing.T) {
	enableSync(t)
	session := newSession(t, `{"token":"host-token","encodings":[]}`)
	if err := sync.SessionConnect(session); err != nil {
		t.Fatal(err)
	}
	drainFrames(t, session)
	if err := sync.SessionReceive(session, []byte(syncHostWelcome), false); err != nil {
		t.Fatal(err)
	}
	events := drainEvents(t, session)
	if len(events) == 0 {
		t.Fatal("the welcome gave no events")
	}
	if !strings.Contains(events[0], `"type":"joined"`) || !strings.Contains(events[0], `"role":"host"`) {
		t.Errorf("the first event is not joined as host: %s", events[0])
	}

	timeline := openTimeline(t, syncCut)
	patches, err := sync.SessionPublish(session, timeline, "main")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(patches, "replace_timeline") {
		t.Errorf("the first publish does not replace the timeline: %s", patches)
	}
	if frames := drainFrames(t, session); !strings.Contains(strings.Join(frames, "\n"), `"type":"snapshot"`) {
		t.Errorf("the first publish sent no snapshot: %v", frames)
	}
	patches, err = sync.SessionPublish(session, timeline, "main")
	if err != nil {
		t.Fatal(err)
	}
	if patches != "[]" {
		t.Errorf("publishing an unchanged timeline gave %s, not []", patches)
	}
	if err := timeline.SetName("fine cut"); err != nil {
		t.Fatal(err)
	}
	patches, err = sync.SessionPublish(session, timeline, "main")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(patches, "set_property") {
		t.Errorf("publishing a rename does not set a property: %s", patches)
	}
	if err := sync.SessionClose(session); err != nil {
		t.Fatal(err)
	}
}

// Settings asking for a compression the session does not speak make no
// session.
func TestSyncSessionRefusesBadSettings(t *testing.T) {
	enableSync(t)
	if _, err := sync.SessionNew(`{"encodings":["zstd"]}`); err == nil {
		t.Error("a session asking for zstd was made")
	}
}
