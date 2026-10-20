package main

import (
	"encoding/json"
	"fmt"
	"log"

	"github.com/alchemist-editor/otio-rust/sdk/go/ext/sync"
)

// Two versions of one sequence, as Furnace sends them: in the second, clip B
// has moved in front of clip A. Every object carries its GUID in
// metadata.sync.guid, which is how a patch names it.
const before = `{"sequenceId": "main", "generation": 1, "otioJson": {
  "OTIO_SCHEMA": "Timeline.1", "name": "cut", "metadata": {"sync": {"guid": "cut"}},
  "tracks": {"OTIO_SCHEMA": "Stack.1", "metadata": {"sync": {"guid": "tracks"}}, "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "metadata": {"sync": {"guid": "v1"}},
     "children": [
       {"OTIO_SCHEMA": "Clip.2", "name": "A", "metadata": {"sync": {"guid": "a"}}},
       {"OTIO_SCHEMA": "Clip.2", "name": "B", "metadata": {"sync": {"guid": "b"}}}]}]}}}`

const after = `{"sequenceId": "main", "generation": 2, "otioJson": {
  "OTIO_SCHEMA": "Timeline.1", "name": "cut", "metadata": {"sync": {"guid": "cut"}},
  "tracks": {"OTIO_SCHEMA": "Stack.1", "metadata": {"sync": {"guid": "tracks"}}, "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "metadata": {"sync": {"guid": "v1"}},
     "children": [
       {"OTIO_SCHEMA": "Clip.2", "name": "B", "metadata": {"sync": {"guid": "b"}}},
       {"OTIO_SCHEMA": "Clip.2", "name": "A", "metadata": {"sync": {"guid": "a"}}}]}]}}}`

func main() {
	if err := sync.Enable(); err != nil {
		log.Fatal(err)
	}

	patches, err := sync.Diff(before, after, true)
	if err != nil {
		log.Fatal(err)
	}
	stats, err := sync.DiffStats(before, after)
	if err != nil {
		log.Fatal(err)
	}
	var summary struct {
		FellBack bool `json:"fellBack"`
	}
	if err := json.Unmarshal([]byte(stats), &summary); err != nil {
		log.Fatal(err)
	}

	// One move, of A to after B: a patch names objects by GUID.
	fmt.Println(patches)
	fmt.Println("fell back to the whole timeline:", summary.FellBack)
}
