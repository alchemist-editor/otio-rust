package main

import (
	"fmt"
	"log"
	"strings"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
)

// second makes one second of picture, named.
func second(name string) otio.Clip {
	clip, err := otio.NewClip(name)
	if err != nil {
		log.Fatal(err)
	}
	if err := clip.SetSourceRange(otio.TimeRange{
		StartTime: otio.RationalTime{Value: 0, Rate: 24},
		Duration:  otio.RationalTime{Value: 24, Rate: 24},
	}); err != nil {
		log.Fatal(err)
	}
	return clip
}

func show(track otio.Track) {
	children, err := track.Children()
	if err != nil {
		log.Fatal(err)
	}
	names := make([]string, 0, len(children))
	for _, child := range children {
		name, _ := child.Name()
		names = append(names, name)
	}
	duration, _ := track.Duration()
	fmt.Printf("%s - %v frames\n", strings.Join(names, " "), duration.Value)
}

func main() {
	track, err := otio.NewTrack("V1", "Video")
	if err != nil {
		log.Fatal(err)
	}
	for _, name := range []string{"A", "B", "C"} {
		if err := track.AppendChild(second(name).Node); err != nil {
			log.Fatal(err)
		}
	}
	show(track)

	// Insert makes room: everything from the insertion point onwards moves
	// later, and the track gets longer.
	at := otio.RationalTime{Value: 24, Rate: 24}
	if err := otio.Insert(second("D").Node, track.Node, at, false, nil); err != nil {
		log.Fatal(err)
	}
	show(track)

	// Overwrite does not: it lays an item over a span and whatever was in
	// that span gives way. The track is the same length afterwards.
	over := otio.TimeRange{
		StartTime: otio.RationalTime{Value: 48, Rate: 24},
		Duration:  otio.RationalTime{Value: 24, Rate: 24},
	}
	if err := otio.Overwrite(second("E").Node, track.Node, over, false, nil); err != nil {
		log.Fatal(err)
	}
	show(track)
}
