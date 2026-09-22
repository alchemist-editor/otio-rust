# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""The schemas a .otio file is made of."""

from .. _otio import (  # noqa
    Box2d,
    Clip,
    Color,
    Effect,
    ExternalReference,
    FreezeFrame,
    Gap,
    GeneratorReference,
    ImageSequenceReference,
    LinearTimeWarp,
    MissingFramePolicy,
    Marker,
    MissingReference,
    NeighborGapPolicy,
    SerializableCollection,
    Stack,
    TimeEffect,
    Timeline,
    Track,
    Transition,
    V2d,
)

from .. core._core_utils import _add_mutable_sequence_methods

from . schemadef import (
    SchemaDef
)

MarkerColor = Color  # for backwards compatibility, as upstream does

# Upstream's collection holds its children without parenting them. Here a
# collection is the parent of what it holds, as a composition is, so it needs
# the same care over a slice assignment that fails part way.
_add_mutable_sequence_methods(
    SerializableCollection, side_effecting_insertions=True
)

# Upstream nests these inside the classes they belong to. A PyO3 class cannot
# be declared inside another, so they are exported flat and put back here.
Track.NeighborGapPolicy = NeighborGapPolicy
ImageSequenceReference.MissingFramePolicy = MissingFramePolicy


class _TrackKind:
    """The two values a track's ``kind`` conventionally takes."""

    Video = "Video"
    Audio = "Audio"


Track.Kind = _TrackKind


class _TransitionTypes:
    """The transition types upstream names.

    ``Custom`` is the escape hatch: a transition whose type is anything else
    is still read, written and round-tripped, it just has no agreed meaning.
    """

    SMPTE_Dissolve = "SMPTE_Dissolve"
    Custom = "Custom_Transition"


Transition.Type = _TransitionTypes
TrackKind = _TrackKind


def timeline_from_clips(clips):
    """Convenience for making a single track timeline from a list of clips."""

    trck = Track(children=clips)
    return Timeline(tracks=[trck])

__all__ = [
    'Box2d',
    'Clip',
    'Effect',
    'ExternalReference',
    'FreezeFrame',
    'Gap',
    'GeneratorReference',
    'ImageSequenceReference',
    'LinearTimeWarp',
    'Marker',
    'MarkerColor',
    'MissingReference',
    'SerializableCollection',
    'Stack',
    'TimeEffect',
    'Timeline',
    'Track',
    'TrackKind',
    'Transition',
    'SchemaDef',
    'timeline_from_clips',
    'V2d',
]
