package main

import (
	"fmt"
	"log"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
)

func main() {
	// An object is built on its own and put together with the others
	// afterwards. Nothing has to exist before the thing it goes into.
	timeline, err := otio.NewTimeline("Cut")
	if err != nil {
		log.Fatal(err)
	}
	// A timeline is released when it is collected, so this is not required.
	// It is worth doing anyway: it frees the whole thing at once, at a
	// moment you chose.
	defer timeline.Close()

	stack, err := otio.NewStack("tracks")
	if err != nil {
		log.Fatal(err)
	}
	track, err := otio.NewTrack("V1", "Video")
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
		clip, err := otio.NewClip(name)
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
		// Appending moves the clip into the timeline's arena. That is
		// bookkeeping this package does for you, not something to hold.
		if err := track.AppendChild(clip.Node); err != nil {
			log.Fatal(err)
		}
	}

	// Three seconds of picture, written as canonical OpenTimelineIO JSON.
	duration, _ := track.Duration()
	fmt.Println(duration.ToSeconds())
	if err := otio.Save(timeline.Node, "cut.otio"); err != nil {
		log.Fatal(err)
	}
}
