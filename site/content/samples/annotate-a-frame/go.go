package main

import (
	"encoding/json"
	"fmt"
	"log"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
	"github.com/alchemist-editor/otio-rust/sdk/go/ext/ori"
)

// A cut with one shot, which sync knows by the GUID `shot`.
const cut = `{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "review",
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "Clip.2", "name": "shot",
       "metadata": {"sync": {"guid": "shot"}}}]}]}
}`

func main() {
	// ORI Annotations is an extension; enabling it enables sync too.
	if err := ori.Enable(); err != nil {
		log.Fatal(err)
	}
	timeline, err := otio.FromJSON(cut)
	if err != nil {
		log.Fatal(err)
	}
	defer timeline.Close()

	// The Annotations track, and the clip on it for frame 10 of the shot.
	track, err := ori.EnsureTrack(timeline)
	if err != nil {
		log.Fatal(err)
	}
	frame, err := ori.EnsureClip(track, "shot", 10, 24)
	if err != nil {
		log.Fatal(err)
	}

	// ORI's coordinates are height-normalised with the origin at the
	// centre, so convert from the pixels of the 1920x1080 picture.
	var xs, ys, sizes, alphas []float64
	for _, pixel := range [][2]float64{{200, 200}, {960, 540}, {1720, 880}} {
		x, y, err := ori.PixelsToOri(1920, 1080, pixel[0], pixel[1])
		if err != nil {
			log.Fatal(err)
		}
		xs, ys = append(xs, x), append(ys, y)
		sizes, alphas = append(sizes, 0.01), append(alphas, 1)
	}

	// A stroke is a PaintStart, its points and a PaintEnd sharing one uuid.
	// Fields left out take the defaults ORI's classes give them.
	uuid, err := ori.NewUUID()
	if err != nil {
		log.Fatal(err)
	}
	stroke, err := json.Marshal([]map[string]any{
		{"OTIO_SCHEMA": "PaintStart.1", "uuid": uuid, "rgba": []float64{1, 0.25, 0.25, 1},
			"participant_hash": "reviewer-1"},
		{"OTIO_SCHEMA": "PaintPoint.1", "uuid": uuid, "points": map[string]any{
			"OTIO_SCHEMA": "PaintVertices.1", "x": xs, "y": ys, "size": sizes, "alpha": alphas}},
		{"OTIO_SCHEMA": "PaintEnd.1", "uuid": uuid},
	})
	if err != nil {
		log.Fatal(err)
	}
	if err := ori.AddCommands(frame, string(stroke)); err != nil {
		log.Fatal(err)
	}

	// Read them back: every annotation on the shot.
	holders, err := ori.ListForClip(timeline, "shot")
	if err != nil {
		log.Fatal(err)
	}
	for _, holder := range holders {
		about, err := ori.Describe(holder)
		if err != nil {
			log.Fatal(err)
		}
		fmt.Println(about)
	}

	// The file is plain OpenTimelineIO with ORI's schemas in it.
	written, err := timeline.ToJSON(4)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println(written)
}
