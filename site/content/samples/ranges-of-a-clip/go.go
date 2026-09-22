package main

import (
	"fmt"
	"log"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
)

func frames(span otio.TimeRange) string {
	return fmt.Sprintf("%v for %v", span.StartTime.Value, span.Duration.Value)
}

func main() {
	// Ten seconds of rushes on disk. AvailableRange belongs to the media,
	// not to the clip: it is what the file offers, whoever uses it.
	media, err := otio.NewExternalReference("A001", "file:///A001.mov")
	if err != nil {
		log.Fatal(err)
	}
	if err := media.SetAvailableRange(otio.TimeRange{
		StartTime: otio.RationalTime{Value: 0, Rate: 24},
		Duration:  otio.RationalTime{Value: 240, Rate: 24},
	}); err != nil {
		log.Fatal(err)
	}

	// Three seconds of it, starting two seconds in. A source range is in the
	// media's clock, which is why it starts at 48 rather than at 0.
	clip, err := otio.NewClip("shot")
	if err != nil {
		log.Fatal(err)
	}
	if err := clip.SetMediaReference("DEFAULT_MEDIA", media.Node); err != nil {
		log.Fatal(err)
	}
	if err := clip.SetSourceRange(otio.TimeRange{
		StartTime: otio.RationalTime{Value: 48, Rate: 24},
		Duration:  otio.RationalTime{Value: 72, Rate: 24},
	}); err != nil {
		log.Fatal(err)
	}

	// A second of black in front of it, so the clip does not start the track.
	head, err := otio.NewGap("")
	if err != nil {
		log.Fatal(err)
	}
	if err := head.SetSourceRange(otio.TimeRange{
		StartTime: otio.RationalTime{Value: 0, Rate: 24},
		Duration:  otio.RationalTime{Value: 24, Rate: 24},
	}); err != nil {
		log.Fatal(err)
	}

	track, err := otio.NewTrack("V1", "Video")
	if err != nil {
		log.Fatal(err)
	}
	if err := track.AppendChild(head.Node); err != nil {
		log.Fatal(err)
	}
	if err := track.AppendChild(clip.Node); err != nil {
		log.Fatal(err)
	}

	// The same clip, asked four questions. The first three answer in the
	// media's clock; the last answers in the track's.
	available, _ := clip.AvailableRange()
	trimmed, _ := clip.TrimmedRange()
	visible, _ := clip.VisibleRange()
	inParent, _ := clip.RangeInParent()
	fmt.Println("available:", frames(available))
	fmt.Println("trimmed:  ", frames(trimmed))
	fmt.Println("visible:  ", frames(visible))
	fmt.Println("in parent:", frames(inParent))
}
