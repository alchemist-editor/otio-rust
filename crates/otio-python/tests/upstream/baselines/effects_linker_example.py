# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""This file is here to support the test_effects_linker unittest.

It holds an adapter that reads a small timeline full of effects, an effects
linker and a post_effects_linker hook script. In practice these would be
separate plugins.
"""


def read_from_string(input_str):
    import opentimelineio as otio

    tl = otio.schema.Timeline(name=input_str)
    track = otio.schema.Track(name="V1")
    track.effects.append(otio.schema.Effect(effect_name="TrackGlow"))
    tl.tracks.append(track)

    clip = otio.schema.Clip(name="shot_1")
    clip.effects.append(otio.schema.Effect(effect_name="Blur"))
    clip.effects.append(otio.schema.Effect(effect_name="Leave Me"))
    clip.effects.append(otio.schema.LinearTimeWarp(time_scalar=2.0))
    track.append(clip)

    track.append(
        otio.schema.Transition(
            transition_type=otio.schema.TransitionTypes.Custom,
            effect=otio.schema.Effect(effect_name="Wipe"),
        )
    )
    track.append(otio.schema.Clip(name="shot_2"))
    return tl


def link_effect(in_effect, parent, effects_linker_argument_map):
    """Example linker: maps by effect_name, keeps what it does not know."""
    import opentimelineio as otio

    if in_effect.effect_name == "Leave Me":
        return None
    if isinstance(in_effect, otio.schema.TimeEffect):
        return None

    linked = otio.schema.Effect(
        name=in_effect.name,
        effect_name=in_effect.effect_name,
        effect_id="test.fx." + in_effect.effect_name.lower(),
        context=(
            "transition"
            if isinstance(parent, otio.schema.Transition)
            else "filter"
        ),
    )
    linked.metadata["linked_on"] = parent.name or type(parent).__name__
    linked.metadata.update(effects_linker_argument_map)
    return linked


def hook_function(in_timeline, argument_map=None):
    # record what the linker produced, to prove the hook ran after it
    ids = []
    for item in in_timeline.find_children():
        effect = getattr(item, "effect", None)
        if effect is not None:
            ids.append(effect.effect_id)
        for e in getattr(item, "effects", []):
            ids.append(e.effect_id)
    in_timeline.metadata["post_effects_linker"] = {
        "effect_ids": ids,
        "argument_map": dict(argument_map or {}),
    }
    return in_timeline
