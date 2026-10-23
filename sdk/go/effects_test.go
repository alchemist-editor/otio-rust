// Tests for the effects schema this library prototypes (ADR 0013): the
// fields an Effect.2 carries, a transition's effect, and the reference
// effects linker.

package otio_test

import (
	"strings"
	"testing"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
)

func TestAnEffectCarriesWhatItIsAndWhoMadeIt(t *testing.T) {
	effect, err := otio.NewEffect("blur", "Gaussian Blur")
	if err != nil {
		t.Fatal(err)
	}
	defer effect.Close()
	for _, set := range []func() error{
		func() error { return effect.SetEffectID("com.example.blur") },
		func() error { return effect.SetEffectVersion("2") },
		func() error { return effect.SetHost("Example") },
		func() error { return effect.SetHostVersion("1.0") },
		func() error { return effect.SetContext("clip") },
		func() error { return effect.SetParameters(`{"radius": 12}`) },
	} {
		if err := set(); err != nil {
			t.Fatal(err)
		}
	}
	if id, err := effect.EffectID(); err != nil || id != "com.example.blur" {
		t.Fatalf("the effect id is %q, %v", id, err)
	}
	if context, err := effect.Context(); err != nil || context != "clip" {
		t.Fatalf("the context is %q, %v", context, err)
	}
	parameters, err := effect.Parameters()
	if err != nil || !strings.Contains(parameters, `"radius":12`) {
		t.Fatalf("the parameters are %q, %v", parameters, err)
	}
	written, err := effect.ToJSON(4)
	if err != nil || !strings.Contains(written, `"OTIO_SCHEMA": "Effect.2"`) {
		t.Fatalf("the effect writes as %q, %v", written, err)
	}
}

func TestATransitionHoldsTheEffectItPerforms(t *testing.T) {
	transition, err := otio.NewTransition("dissolve", "SMPTE_Dissolve")
	if err != nil {
		t.Fatal(err)
	}
	defer transition.Close()
	if _, err := transition.Effect(); err == nil {
		t.Fatal("a new transition has an effect")
	}
	effect, err := otio.NewEffect("dissolve", "Cross Dissolve")
	if err != nil {
		t.Fatal(err)
	}
	if err := transition.SetEffect(&effect.Node); err != nil {
		t.Fatal(err)
	}
	held, err := transition.Effect()
	if err != nil {
		t.Fatal(err)
	}
	if _, ok := held.AsEffect(); !ok {
		t.Fatal("the transition's effect is not an effect")
	}
	if err := transition.SetEffect(nil); err != nil {
		t.Fatal(err)
	}
	if _, err := transition.Effect(); err == nil {
		t.Fatal("clearing the effect left one")
	}
}

func TestTheReferenceLinkerRunsOnlyWhenNamed(t *testing.T) {
	clip, err := otio.NewClip("shot")
	if err != nil {
		t.Fatal(err)
	}
	defer clip.Close()
	effect, err := otio.NewEffect("blur", "Gaussian Blur")
	if err != nil {
		t.Fatal(err)
	}
	if err := clip.AppendEffect(effect.Node); err != nil {
		t.Fatal(err)
	}
	written, err := otio.WriteToBytes(otio.FormatOTIOJSON, clip.Node, nil)
	if err != nil {
		t.Fatal(err)
	}

	effectID := func(options *otio.ReadOptions) string {
		root, err := otio.ReadFromBytes(otio.FormatOTIOJSON, written, options)
		if err != nil {
			t.Fatal(err)
		}
		defer root.Close()
		item, _ := root.AsItem()
		first, err := item.EffectAt(0)
		if err != nil {
			t.Fatal(err)
		}
		linked, _ := first.AsEffect()
		id, err := linked.EffectID()
		if err != nil {
			t.Fatal(err)
		}
		return id
	}
	if id := effectID(nil); id != "" {
		t.Fatalf("an ordinary read linked the effect to %q", id)
	}
	if id := effectID(&otio.ReadOptions{EffectsLinker: "otio_fx_registry"}); id != "otio.fx.gaussian_blur" {
		t.Fatalf("the reference linker linked the effect to %q", id)
	}
	options := &otio.ReadOptions{EffectsLinker: "otio_fx_registry", DoNotLinkEffects: true}
	if id := effectID(options); id != "" {
		t.Fatalf("asked not to link, the read linked the effect to %q", id)
	}
}
