// Tests for turning on extensions, which are otio-rust's own work and not
// part of OpenTimelineIO.
//
// Enabling is process-wide and cannot be undone, so nothing here asserts that
// an extension starts off: another test in the same binary may have enabled it.

package otio_test

import (
	"errors"
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
