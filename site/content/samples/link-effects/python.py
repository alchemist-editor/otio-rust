import json

import opentimelineio as otio

# A clip with a blur and a dissolve into the next one, as an editor that
# knows nothing of the registry writes them: named, and nothing more.
cut = json.dumps({
    "OTIO_SCHEMA": "Track.1",
    "name": "V1",
    "kind": "Video",
    "children": [
        {"OTIO_SCHEMA": "Clip.2", "name": "A", "effects": [
            {"OTIO_SCHEMA": "Effect.1", "name": "blur",
             "effect_name": "Gaussian Blur"},
        ]},
        {"OTIO_SCHEMA": "Transition.2", "name": "dissolve",
         "transition_type": "SMPTE_Dissolve",
         "effect": {"OTIO_SCHEMA": "Effect.2",
                    "effect_name": "Cross Dissolve"}},
        {"OTIO_SCHEMA": "Clip.2", "name": "B"},
    ],
})

# `otio_fx_registry` ships with the library, but runs only when a read
# names it.
track = otio.adapters.read_from_string(
    cut,
    "otio_json",
    effects_linker_name="otio_fx_registry",
)

blur = track[0].effects[0]
dissolve = track[1].effect
for holder, effect in (("A", blur), ("dissolve", dissolve)):
    print("{}: {} -> {}".format(holder, effect.effect_name, effect.effect_id))

# What each one was, and how faithful the mapping is, are kept beside it.
print("fidelity:", blur.metadata["otio.fx"]["fidelity"])

# An effect can say all of that itself, too.
grade = otio.schema.Effect(
    effect_name="Lift Gamma Gain",
    effect_id="com.example.lgg",
    host="Example Grade",
    context="filter",
    parameters={"gamma": 1.1},
)
print(grade.effect_id, dict(grade.parameters))
