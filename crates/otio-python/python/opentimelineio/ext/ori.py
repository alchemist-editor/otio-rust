# SPDX-License-Identifier: Apache-2.0
"""ORI Annotations: the ORI project's review annotations on a timeline.

**An otio-rust extension, not part of OpenTimelineIO.** It does nothing
until it is enabled, and enabling it enables timeline sync, which it is
built on: every function here but :func:`enable` and :func:`is_enabled`
raises :class:`RuntimeError` until then. See ADR 0012 in the otio-rust
repository.

The schemas belong to the ASWF's ORI project
(https://github.com/richardssam/ORIAnnotations). An annotation is a list of
commands (``PaintStart.1``, ``PaintPoint.1``, ``PaintEnd.1``,
``TextAnnotation.1`` and the rest), kept on a one-frame clip in a track named
``Annotations``, with ``metadata["clip_guid"]`` naming the annotated clip by
its sync GUID. Commands go in and come out as JSON text, a list of ORI
objects exactly as a file holds them; :func:`add_commands` and the other
writers also take a Python list of dicts.

ORI's own Python plugin, ``otio_event_plugin``, runs on these bindings
unchanged, with or without this extension. Use this module for the
annotation track; use ORI's plugin for its classes.
"""

import builtins
import json

from .. import _otio

__all__ = [
    "ID",
    "enable",
    "is_enabled",
    "find_track",
    "ensure_track",
    "find_clip",
    "ensure_clip",
    "list",
    "list_for_clip",
    "list_at_frame",
    "describe",
    "commands",
    "add_commands",
    "replace_commands",
    "remove_commands",
    "set_commands",
    "merge_commands",
    "replace_by_kind",
    "merge_duplicates",
    "owner_of",
    "send_partial",
    "read_partial",
    "commit",
    "fold",
    "new_uuid",
    "timestamp_now",
    "pixels_to_ori",
    "ori_to_pixels",
    "aspect_half",
    "ori_to_bounds",
    "bounds_to_ori",
]

#: The id ORI Annotations is enabled by.
ID = "ori-annotations"


def enable():
    """Turn ORI Annotations on, and timeline sync with it. Enabling it again
    does nothing."""
    _otio._ext_enable(ID)


def is_enabled():
    """Whether ORI Annotations is on."""
    return _otio._ext_is_enabled(ID)


def _json(commands):
    return commands if isinstance(commands, str) else json.dumps(commands)


def find_track(timeline):
    """The annotation track of ``timeline``, the first track whose name
    contains "annotation" in any case, or ``None``."""
    return _otio._ori_find_track(timeline)


def ensure_track(timeline):
    """The annotation track of ``timeline``, first adding a video track named
    ``Annotations`` on top of its stack if it has none."""
    return _otio._ori_ensure_track(timeline)


def find_clip(track, clip_guid, frame):
    """The annotation clip on ``track`` for the clip whose sync GUID is
    ``clip_guid``, at the whole source frame ``frame``, or ``None``."""
    return _otio._ori_find_clip(track, clip_guid, frame)


def ensure_clip(track, clip_guid, time):
    """The annotation clip on ``track`` for ``clip_guid`` at ``time``, a
    :class:`~opentimelineio.opentime.RationalTime`, first adding one frame
    long at the end of the track if there is none, as ORI adds one."""
    return _otio._ori_ensure_clip(track, clip_guid, time)


def list(timeline):  # noqa: A001 - the name every SDK gives it
    """The object holding each annotation on ``timeline``: an annotation
    clip, or an ``AnnotationEffect.1``."""
    return _otio._ori_list(timeline)


def list_for_clip(timeline, clip_guid):
    """The object holding each annotation of the clip whose sync GUID is
    ``clip_guid``."""
    return _otio._ori_list(timeline, clip_guid)


def list_at_frame(timeline, clip_guid, frame):
    """The object holding each annotation of ``clip_guid`` at the whole
    source frame ``frame``. An ``AnnotationEffect.1`` covers every frame."""
    return _otio._ori_list(timeline, clip_guid, frame)


def describe(holder):
    """What ``holder`` says about its annotation, as a dict: ``form``,
    ``clip_guid``, ``clip_name``, ``frame``, ``note``, ``status`` and
    ``command_count``. Raises :class:`TypeError` for an object that holds
    none."""
    return json.loads(_otio._ori_describe(holder))


def commands(holder, pretty=False):
    """The commands ``holder`` holds, as JSON text."""
    return _otio._ori_commands(holder, pretty)


def add_commands(clip, commands):
    """Add ``commands`` at the end of the annotation clip ``clip``'s."""
    _otio._ori_add_commands(clip, _json(commands))


def replace_commands(clip, uuid, commands):
    """Replace every command of ``clip`` whose ``uuid`` is ``uuid``, such as
    one whole stroke, with ``commands``, where the first was. Returns whether
    any had it; if none did, nothing changes."""
    return _otio._ori_replace_commands(clip, uuid, _json(commands))


def remove_commands(clip, uuids):
    """Remove every command of ``clip`` whose ``uuid`` is in ``uuids``, and
    return how many were removed."""
    return _otio._ori_remove_commands(clip, builtins.list(uuids))


def set_commands(clip, commands):
    """Replace all of ``clip``'s commands with ``commands``, as ORI's
    ``REPLACE_ANNOTATION_COMMANDS`` does. An empty list clears it."""
    _otio._ori_set_commands(clip, _json(commands))


def merge_commands(clip, commands):
    """Merge ``commands`` into ``clip``'s by uuid, as Alchemist's review
    merges a comment's strokes: a uuid ``clip`` has is replaced where it
    stands, a new one goes at the end. Returns ``(replaced, added)``, the
    number of uuids of each."""
    return _otio._ori_merge_commands(clip, _json(commands))


def replace_by_kind(clip, commands):
    """Replace ``clip``'s commands of each kind ``commands`` has (strokes,
    text, ...) with ``commands``, keeping the kinds it lacks, as ORI's
    ``REPLACE_ANNOTATION_COMMANDS`` reads. An empty list clears all of
    them. Returns how many commands were removed."""
    return _otio._ori_replace_by_kind(clip, _json(commands))


def merge_duplicates(timeline):
    """Fold every annotation clip that repeats an earlier one's clip and
    source frame into the first, and return how many were removed."""
    return _otio._ori_merge_duplicates(timeline)


def owner_of(clip, uuid):
    """The user id of the review comment that added the command ``uuid`` to
    ``clip``, or ``None``."""
    return _otio._ori_owner_of(clip, uuid)


def send_partial(session, partial):
    """Send a stroke in progress to ``session``'s peers: a dict with
    ``clip_guid``, ``frame``, ``fps`` and ``events``, the commands drawn so
    far. Peers draw it and drop it; it never reaches a timeline. ORI's wire
    carries it; Furnace's does not yet, and the session raises
    :class:`NotImplementedError` when it would send it."""
    _otio._ori_send_partial(session._session, _json(partial))


def read_partial(event):
    """The stroke in progress ``event`` carries, an event dict from
    :meth:`opentimelineio.ext.sync.Session.poll_event`, as a dict, or
    ``None`` when it carries none."""
    partial = _otio._ori_read_partial(_json(event))
    return None if partial is None else json.loads(partial)


def commit(session, timeline, anchor, body_markdown, commands, client_mutation_id=""):
    """Commit ``commands`` with the note ``body_markdown`` as a review
    comment on ``session``, drawn where ``anchor`` says: a dict with
    ``sequenceId``, ``capturedGeneration``, ``clipGuid``, ``sourceTime``,
    ``capturedSequenceTime`` and ``capturedAt``. Raises
    :class:`PermissionError` if a command replaces one another user's
    comment added to ``timeline``. Returns the commit's client mutation
    id."""
    return _otio._ori_commit(
        session._session,
        timeline,
        _json(anchor),
        body_markdown,
        _json(commands),
        client_mutation_id,
    )


def fold(timeline, event):
    """Fold the review comment a ``review_relay`` event carries into
    ``timeline``: its strokes merge by uuid into the annotation clip at its
    anchor, and the comment is recorded in the clip's
    ``metadata["alchemist"]["comments"]``. Returns ``(clip, already)``,
    ``already`` being whether this revision of the comment was there."""
    return _otio._ori_fold(timeline, _json(event))


def new_uuid():
    """A new uuid for a command."""
    return _otio._sync_new_guid()


def timestamp_now():
    """The current time in ISO 8601, for a command's ``timestamp``."""
    return _otio._ori_timestamp_now()


def pixels_to_ori(width, height, px, py):
    """The pixel position ``px``, ``py`` of an image ``width`` by ``height``,
    counted from the top left, as ORI's ``(x, y)``: height-normalised, Y up,
    origin at the centre."""
    return _otio._ori_pixels_to_ori(width, height, px, py)


def ori_to_pixels(width, height, x, y):
    """ORI's ``x``, ``y`` as an unrounded pixel position of an image
    ``width`` by ``height``, counted from the top left."""
    return _otio._ori_ori_to_pixels(width, height, x, y)


def aspect_half(width, height):
    """Half the width of an image in ORI's units: 8/9 for 16:9, and when the
    height is not positive."""
    return _otio._ori_aspect_half(width, height)


def ori_to_bounds(x, y, bounds):
    """ORI's ``x``, ``y`` in OpenTimelineIO image space within ``bounds``, a
    :class:`~opentimelineio.schema.Box2d` such as a reference's
    ``available_image_bounds``."""
    return _otio._ori_ori_to_bounds(x, y, _box(bounds))


def bounds_to_ori(x, y, bounds):
    """The image-space point ``x``, ``y`` within ``bounds`` as ORI's
    ``(x, y)``. The inverse of :func:`ori_to_bounds`."""
    return _otio._ori_bounds_to_ori(x, y, _box(bounds))


def _box(bounds):
    return (bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y)
