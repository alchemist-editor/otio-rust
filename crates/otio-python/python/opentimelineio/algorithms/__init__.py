# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Algorithms for OTIO objects."""

# flake8: noqa
from .track_algo import (
    track_trimmed_to_range,
    track_with_expanded_transitions
)

from .stack_algo import (
    flatten_stack,
    top_clip_at_time,
)

from .filter import (
    filtered_composition,
    filtered_with_sequence_context
)
from .timeline_algo import (
    timeline_trimmed_to_range
)

# Upstream's C++ editing algorithms (`otio::algo` in editAlgorithm.h), which
# upstream's own Python package does not bind. They keep the C++ names, and
# its parameter names and defaults in snake case.
from .. _otio import algo as _algo

ReferencePoint = _algo.ReferencePoint
overwrite = _algo.overwrite
insert = _algo.insert
trim = _algo.trim
slice = _algo.slice
slip = _algo.slip
slide = _algo.slide
ripple = _algo.ripple
roll = _algo.roll
fill = _algo.fill
remove = _algo.remove
