package main

import (
	"fmt"
	"log"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
	"github.com/alchemist-editor/otio-rust/sdk/go/ext/sync"
)

// A cut, as an .otio file would hold it. Nothing in it has a GUID yet.
const cut = `{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "cut",
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "Clip.2", "name": "A"},
      {"OTIO_SCHEMA": "Clip.2", "name": "B"}]}]}
}`

func main() {
	if err := sync.Enable(); err != nil {
		log.Fatal(err)
	}

	// Beginning a transaction gives every object a GUID, and remembers the
	// timeline as it was: that snapshot is what a second program starts
	// from.
	editor, err := otio.FromJSON(cut)
	if err != nil {
		log.Fatal(err)
	}
	defer editor.Close()
	before, err := sync.Begin(editor, "main", 1)
	if err != nil {
		log.Fatal(err)
	}
	stamped, err := editor.ToJSON(0)
	if err != nil {
		log.Fatal(err)
	}
	viewer, err := otio.FromJSON(stamped)
	if err != nil {
		log.Fatal(err)
	}
	defer viewer.Close()

	// Edit as usual, then commit for the patches that say what changed.
	if err := editor.SetName("fine cut"); err != nil {
		log.Fatal(err)
	}
	patches, err := sync.Commit(editor, before)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println(patches)

	// The other copy applies them in place: what it held still names the
	// same objects.
	if err := sync.ApplyTo(viewer, patches); err != nil {
		log.Fatal(err)
	}
	name, err := viewer.Name()
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println("the viewer now shows", name)
}
