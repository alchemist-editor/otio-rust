package main

import (
	"fmt"
	"log"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
)

// A clip with a blur and a dissolve into the next one, as an editor that
// knows nothing of the registry writes them: named, and nothing more.
const cut = `{
  "OTIO_SCHEMA": "Track.1",
  "name": "V1",
  "kind": "Video",
  "children": [
    {"OTIO_SCHEMA": "Clip.2", "name": "A", "effects": [
      {"OTIO_SCHEMA": "Effect.1", "name": "blur", "effect_name": "Gaussian Blur"}
    ]},
    {"OTIO_SCHEMA": "Transition.2", "name": "dissolve",
     "transition_type": "SMPTE_Dissolve",
     "effect": {"OTIO_SCHEMA": "Effect.2", "effect_name": "Cross Dissolve"}},
    {"OTIO_SCHEMA": "Clip.2", "name": "B"}
  ]
}`

// show prints what an effect was called and what the linker made of it.
func show(holder string, node otio.Node) {
	effect, ok := node.AsEffect()
	if !ok {
		log.Fatal("not an effect")
	}
	name, err := effect.EffectName()
	if err != nil {
		log.Fatal(err)
	}
	id, err := effect.EffectID()
	if err != nil {
		log.Fatal(err)
	}
	fmt.Printf("%s: %s -> %s\n", holder, name, id)
}

func main() {
	// otio_fx_registry is built in, but runs only when a read names it.
	options := &otio.ReadOptions{EffectsLinker: "otio_fx_registry"}
	track, err := otio.ReadFromBytes(otio.FormatOTIOJSON, []byte(cut), options)
	if err != nil {
		log.Fatal(err)
	}
	defer track.Close()

	first, err := track.ChildAt(0)
	if err != nil {
		log.Fatal(err)
	}
	clip, _ := first.AsItem()
	blur, err := clip.EffectAt(0)
	if err != nil {
		log.Fatal(err)
	}
	show("A", blur)

	second, err := track.ChildAt(1)
	if err != nil {
		log.Fatal(err)
	}
	transition, _ := second.AsTransition()
	dissolve, err := transition.Effect()
	if err != nil {
		log.Fatal(err)
	}
	show("dissolve", dissolve)
}
