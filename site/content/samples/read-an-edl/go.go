package main

import (
	"fmt"
	"log"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
)

func main() {
	// An EDL never says what rate its timecode is at, so this has to be
	// right: a file read at the wrong rate puts every event in the wrong
	// place rather than failing.
	document, err := otio.ReadFromFile(otio.FormatCMX3600, "cut.edl", &otio.ReadOptions{Rate: 24})
	if err != nil {
		log.Fatal(err)
	}
	defer document.Close()

	root, err := document.Root()
	if err != nil {
		log.Fatal(err)
	}

	clips, err := root.FindClips()
	if err != nil {
		log.Fatal(err)
	}
	for _, clip := range otio.Filter(clips, otio.Node.AsClip) {
		name, _ := clip.Name()
		fmt.Println(name)
	}
}
