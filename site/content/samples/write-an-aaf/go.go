package main

import (
	"fmt"
	"log"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
)

func main() {
	timeline, err := otio.NewTimeline("Cut")
	if err != nil {
		log.Fatal(err)
	}
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

	oneSecond := otio.TimeRange{
		StartTime: otio.RationalTime{Value: 0, Rate: 24},
		Duration:  otio.RationalTime{Value: 24, Rate: 24},
	}

	// An AAF clip is cut from media of a known length, so each clip's media
	// says how much of it there is. A new clip has no media at all, so its
	// reference goes in under upstream's key and is made the active one.
	for _, name := range []string{"A001C003", "A001C004"} {
		media, err := otio.NewExternalReference("", "file:///media/"+name+".mov")
		if err != nil {
			log.Fatal(err)
		}
		if err := media.SetAvailableRange(oneSecond); err != nil {
			log.Fatal(err)
		}

		clip, err := otio.NewClip(name)
		if err != nil {
			log.Fatal(err)
		}
		if err := clip.SetMediaReference("DEFAULT_MEDIA", media.Node); err != nil {
			log.Fatal(err)
		}
		if err := clip.SetActiveMediaReferenceKey("DEFAULT_MEDIA"); err != nil {
			log.Fatal(err)
		}
		if err := clip.SetSourceRange(oneSecond); err != nil {
			log.Fatal(err)
		}
		if err := track.AppendChild(clip.Node); err != nil {
			log.Fatal(err)
		}
	}

	// Every clip needs a MobID, from its metadata, its media's metadata or
	// the AAF its media names. A cut built from scratch has none, so let the
	// writer make them up rather than refuse the clip.
	options := &otio.WriteOptions{AAFUseEmptyMobIds: true}
	if err := otio.WriteToFile(otio.FormatAAF, timeline.Node, "cut.aaf", options); err != nil {
		log.Fatal(err)
	}

	root, err := otio.ReadFromFile(otio.FormatAAF, "cut.aaf", nil)
	if err != nil {
		log.Fatal(err)
	}
	defer root.Close()

	clips, err := root.FindClips()
	if err != nil {
		log.Fatal(err)
	}
	for _, clip := range otio.Filter(clips, otio.Node.AsClip) {
		name, _ := clip.Name()
		fmt.Println(name)
	}
}
