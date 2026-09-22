package main

import (
	"fmt"
	"log"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
)

func main() {
	// A document owns the objects in it. Close frees a whole timeline at
	// once, at a moment you chose.
	document := otio.New()
	defer document.Close()

	timeline, err := document.NewTimeline("Cut")
	if err != nil {
		log.Fatal(err)
	}
	stack, err := document.NewStack("tracks")
	if err != nil {
		log.Fatal(err)
	}
	track, err := document.NewTrack("V1", "Video")
	if err != nil {
		log.Fatal(err)
	}
	if err := timeline.SetTracks(&stack.Node); err != nil {
		log.Fatal(err)
	}
	if err := stack.AppendChild(track.Node); err != nil {
		log.Fatal(err)
	}

	for index, name := range []string{"A", "B", "C"} {
		clip, err := document.NewClip(name)
		if err != nil {
			log.Fatal(err)
		}
		span := otio.TimeRange{
			StartTime: otio.RationalTime{Value: float64(index) * 24, Rate: 24},
			Duration:  otio.RationalTime{Value: 24, Rate: 24},
		}
		if err := clip.SetSourceRange(span); err != nil {
			log.Fatal(err)
		}
		if err := track.AppendChild(clip.Node); err != nil {
			log.Fatal(err)
		}
	}

	if err := document.SetRoot(&timeline.Node); err != nil {
		log.Fatal(err)
	}

	// Three seconds of picture, written as canonical OpenTimelineIO JSON.
	duration, _ := track.Duration()
	fmt.Println(duration.ToSeconds())
	if err := document.Save("cut.otio"); err != nil {
		log.Fatal(err)
	}
}
