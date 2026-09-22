package main

import (
	"log"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
)

func main() {
	// An EDL never says what rate its timecode is at, so this has to be
	// right: a file read at the wrong rate puts every event in the wrong
	// place rather than failing.
	root, err := otio.ReadFromFile(otio.FormatCMX3600, "cut.edl", &otio.ReadOptions{Rate: 24})
	if err != nil {
		log.Fatal(err)
	}
	defer root.Close()

	// Nothing happens in between. The timeline an EDL parses to is the same
	// timeline FCP X writes out, so converting is a read and a write: the
	// object model is the interchange, and the file formats are two ways of
	// spelling it.
	//
	// Writing starts at the object it is given, so handing it the root
	// writes the whole file.
	if err := otio.WriteToFile(otio.FormatFcpxXML, root, "cut.fcpxml", nil); err != nil {
		log.Fatal(err)
	}
}
