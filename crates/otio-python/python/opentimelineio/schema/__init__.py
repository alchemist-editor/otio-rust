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
    Stack,
    Timeline,
    Track,
    Transition,
    V2d,
)

MarkerColor = Color  # for backwards compatibility, as upstream does

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
    'Stack',
    'Timeline',
    'Track',
    'TrackKind',
    'Transition',
    'timeline_from_clips',
    'V2d',
]
