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

	tracks, err := timeline.Tracks()
	if err != nil {
		log.Fatal(err)
	}
	stack, _ := tracks.AsStack()
	track, err := otio.NewTrack("V1", "Video")
	if err != nil {
		log.Fatal(err)
	}
	if err := stack.AppendChild(track.Node); err != nil {
		log.Fatal(err)
	}

	// A cut of two clips: one whose media is a file beside the program, and
	// one whose media is on the web.
	for _, source := range []struct{ name, url string }{
		{"A001C003", "shot.mov"},
		{"A001C004", "https://example.com/remote.mov"},
	} {
		media, err := otio.NewExternalReference("", source.url)
		if err != nil {
			log.Fatal(err)
		}
		clip, err := otio.NewClip(source.name)
		if err != nil {
			log.Fatal(err)
		}
		if err := clip.SetMediaReference("DEFAULT_MEDIA", media.Node); err != nil {
			log.Fatal(err)
		}
		if err := clip.SetActiveMediaReferenceKey("DEFAULT_MEDIA"); err != nil {
			log.Fatal(err)
		}
		if err := track.AppendChild(clip.Node); err != nil {
			log.Fatal(err)
		}
	}

	// Every clip whose media is a file has the file copied into the bundle
	// and its reference pointed at the copy. Media that is not a file would
	// stop the write, so it is made missing instead. FormatOTIOD writes the
	// same layout as a directory.
	options := &otio.WriteOptions{BundleMediaPolicy: otio.BundleMediaPolicyMissingIfNotFile}
	if err := otio.WriteToFile(otio.FormatOTIOZ, timeline.Node, "cut.otioz", options); err != nil {
		log.Fatal(err)
	}

	// Unpacked, with each reference made absolute, the media is ready to use.
	read := &otio.ReadOptions{BundleExtractPath: "cut", BundleAbsoluteMediaPaths: true}
	root, err := otio.ReadFromFile(otio.FormatOTIOZ, "cut.otioz", read)
	if err != nil {
		log.Fatal(err)
	}
	defer root.Close()

	clips, err := root.FindClips()
	if err != nil {
		log.Fatal(err)
	}
	for _, clip := range otio.Filter(clips, otio.Node.AsClip) {
		media, err := clip.MediaReference("")
		if err != nil {
			log.Fatal(err)
		}
		if external, ok := media.AsExternalReference(); ok {
			url, _ := external.TargetURL()
			fmt.Println(url)
		} else {
			fmt.Println("missing")
		}
	}
}
