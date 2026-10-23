import { init, Clip, Effect, readFromString, Track, Transition } from "@alchemist-edit/otio";

await init();

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
}`;

// otio_fx_registry is built in, but runs only when a read names it.
const track = readFromString("otioJson", cut, { effectsLinker: "otio_fx_registry" });
if (!(track instanceof Track)) throw new Error("the cut is not a track");

/** Prints what an effect was called and what the linker made of it. */
function show(holder: string, effect: Effect): void {
  console.log(`${holder}: ${effect.effectName} -> ${effect.effectId}`);
}

const clip = track.childAt(0);
if (clip instanceof Clip) show("A", clip.effectAt(0));

// A transition's effect is optional.
const transition = track.childAt(1);
if (transition instanceof Transition && transition.effect instanceof Effect) {
  show("dissolve", transition.effect);
}
