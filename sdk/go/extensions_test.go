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
