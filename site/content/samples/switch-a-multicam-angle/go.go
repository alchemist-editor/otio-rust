package main

import (
	"fmt"
	"log"
	"os"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
)

// Two cameras on one interview, and an edit with two cuts on camera A.
const edit = `{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "interview",
  "multicams": {
    "mc1": {"OTIO_SCHEMA": "Multicam.1", "name": "Interview", "children": [
      {"OTIO_SCHEMA": "Stack.1", "name": "A", "children": [
        {"OTIO_SCHEMA": "Track.1", "name": "A V", "kind": "Video", "children": [
          {"OTIO_SCHEMA": "Clip.2", "name": "A_take",
           "source_range": {"OTIO_SCHEMA": "TimeRange.1",
             "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 0, "rate": 24},
             "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 240, "rate": 24}}}]}]},
      {"OTIO_SCHEMA": "Stack.1", "name": "B", "children": [
        {"OTIO_SCHEMA": "Track.1", "name": "B V", "kind": "Video", "children": [
          {"OTIO_SCHEMA": "Clip.2", "name": "B_take",
           "source_range": {"OTIO_SCHEMA": "TimeRange.1",
             "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 0, "rate": 24},
             "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 240, "rate": 24}}}]}]}]}
  },
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "MulticamClip.1", "name": "cut 1", "multicam": "mc1", "angle": "A",
       "source_range": {"OTIO_SCHEMA": "TimeRange.1",
         "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 0, "rate": 24},
         "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 48, "rate": 24}}},
      {"OTIO_SCHEMA": "MulticamClip.1", "name": "cut 2", "multicam": "mc1", "angle": "A",
       "source_range": {"OTIO_SCHEMA": "TimeRange.1",
         "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 48, "rate": 24},
         "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 48, "rate": 24}}}]}]}
}`

func main() {
	read, err := otio.FromJSON(edit)
	if err != nil {
		log.Fatal(err)
	}
	defer read.Close()
	timeline, ok := read.AsTimeline()
	if !ok {
		log.Fatal("the edit is not a timeline")
	}
	tracks, err := timeline.Tracks()
	if err != nil {
		log.Fatal(err)
	}
	track, err := tracks.ChildAt(0)
	if err != nil {
		log.Fatal(err)
	}
	cuts, err := track.Children()
	if err != nil {
		log.Fatal(err)
	}

	// Switch the second cut to camera B. It keeps its place and its range.
	if err := otio.MulticamClipSetAngle(cuts[1], "B", 0); err != nil {
		log.Fatal(err)
	}

	// What each cut plays: a track of one angle.
	for _, cut := range cuts {
		played, err := otio.MulticamResolve(timeline.Node, cut, "")
		if err != nil {
			log.Fatal(err)
		}
		cutName, _ := cut.Name()
		playedName, _ := played.Name()
		fmt.Printf("%s plays %s\n", cutName, playedName)
	}
	if report, err := otio.MulticamValidate(timeline.Node); err != nil || report != "" {
		log.Fatalf("the multicams do not resolve: %v %s", err, report)
	}
	written, err := timeline.ToJSON(4)
	if err != nil {
		log.Fatal(err)
	}
	if err := os.WriteFile("edit.otio", []byte(written), 0o644); err != nil {
		log.Fatal(err)
	}

	// For a reader without multicam: each cut becomes the material it plays.
	lost, err := otio.MulticamFlatten(timeline.Node)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Printf("flattening lost: %q\n", lost)
}
