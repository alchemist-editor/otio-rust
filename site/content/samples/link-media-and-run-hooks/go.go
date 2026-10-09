package main

import (
	"fmt"
	"log"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
)

// A cut of two clips, as an .otio file would hold it.
const cut = `{
  "OTIO_SCHEMA": "Track.1",
  "name": "V1",
  "kind": "Video",
  "children": [
    {"OTIO_SCHEMA": "Clip.2", "name": "A"},
    {"OTIO_SCHEMA": "Clip.2", "name": "B"}
  ]
}`

func main() {
	// A media linker is handed each clip as it is read, with the arguments
	// the read was given, and answers with the media the clip should use.
	// An empty Node leaves the clip as it was.
	err := otio.RegisterMediaLinker("proxies", func(clip otio.Clip, arguments otio.Metadata) (otio.Node, error) {
		name, err := clip.Name()
		if err != nil {
			return otio.Node{}, err
		}
		root, err := arguments.GetString("root")
		if err != nil {
			return otio.Node{}, err
		}
		proxy, err := otio.NewExternalReference(name, root+"/"+name+".mov")
		return proxy.Node, err
	})
	if err != nil {
		log.Fatal(err)
	}
	defer otio.UnregisterMediaLinker("proxies")

	// A hook script is handed the whole result, and answers with what the
	// read goes on with: here the same object, stamped.
	err = otio.RegisterHookScript("stamp", func(target otio.Node, arguments otio.Metadata) (otio.Node, error) {
		who, err := arguments.GetString("who")
		if err != nil {
			return otio.Node{}, err
		}
		return target, target.Metadata().SetString("read_by", who)
	})
	if err != nil {
		log.Fatal(err)
	}
	defer otio.UnregisterHookScript("stamp")
	if err := otio.AttachHookScript("post_adapter_read", "stamp"); err != nil {
		log.Fatal(err)
	}
	defer otio.DetachHookScript("post_adapter_read", "stamp")

	// The read names the linker, and carries both sets of arguments as JSON.
	track, err := otio.ReadFromBytes(otio.FormatOTIOJSON, []byte(cut), &otio.ReadOptions{
		MediaLinker:          "proxies",
		MediaLinkerArguments: `{"root": "/proxies"}`,
		HookArguments:        `{"who": "the conform"}`,
	})
	if err != nil {
		log.Fatal(err)
	}
	defer track.Close()

	who, _ := track.Metadata().GetString("read_by")
	fmt.Println("read by", who)
	clips, err := track.FindClips()
	if err != nil {
		log.Fatal(err)
	}
	for _, node := range clips {
		clip, _ := node.AsClip()
		media, _ := clip.MediaReference("")
		external, _ := media.AsExternalReference()
		url, _ := external.TargetURL()
		name, _ := clip.Name()
		fmt.Println(name, "->", url)
	}
}
