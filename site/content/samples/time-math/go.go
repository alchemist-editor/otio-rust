package main

import (
	"fmt"
	"log"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
)

func main() {
	// A time is a value and a rate, not a number of seconds. Four seconds at
	// 24 is 96 units; the rate travels with it so nothing has to guess later.
	start, err := otio.RationalTimeFromTimecode("01:00:00:00", 24)
	if err != nil {
		log.Fatal(err)
	}
	duration := otio.RationalTimeFromFrames(96, 24)

	end := start.Add(duration)
	timecode, err := end.ToTimecode()
	if err != nil {
		log.Fatal(err)
	}
	fmt.Printf("%s for %g seconds\n", timecode, duration.ToSeconds())

	// Comparison rescales first, so the same instant at two rates is equal.
	fmt.Println(otio.RationalTime{Value: 24, Rate: 24}.Equals(otio.RationalTime{Value: 48, Rate: 48}))
}
