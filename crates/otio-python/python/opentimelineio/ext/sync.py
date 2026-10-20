# SPDX-License-Identifier: Apache-2.0
"""Timeline sync: sharing a timeline between programs while it is edited.

**An otio-rust extension, not part of OpenTimelineIO.** It does nothing
until it is enabled: every function here but :func:`enable` and
:func:`is_enabled` raises :class:`RuntimeError` until then. See ADR 0011 in
the otio-rust repository.

Snapshots and patches are JSON text, as Furnace sends them. A snapshot holds
``sequenceId``, ``generation`` and the timeline as ``otioJson``; patches are
a JSON list. Functions that work on a timeline in memory take the OTIO object
itself (a :class:`~opentimelineio.schema.Timeline`, or any object for
:func:`guid` and :func:`set_guid`) and act on it in place.

Text that is not JSON, not a snapshot or not a list of patches raises
:class:`ValueError`.
"""

from .. import _otio

__all__ = [
    "ID",
    "enable",
    "is_enabled",
    "new_guid",
    "guid",
    "set_guid",
    "stamp",
    "find",
    "diff",
    "diff_stats",
    "apply",
    "apply_to",
    "begin",
    "commit",
    "rollback",
    "start_journal",
    "stop_journal",
    "commit_journal",
]

#: The id sync is enabled by.
ID = "sync"


def enable():
    """Turn timeline sync on. Enabling it again does nothing."""
    _otio._ext_enable(ID)


def is_enabled():
    """Whether timeline sync is on."""
    return _otio._ext_is_enabled(ID)


def new_guid():
    """A new GUID, a version 4 UUID."""
    return _otio._sync_new_guid()


def guid(node):
    """The GUID ``node`` carries in ``metadata["sync"]["guid"]``, or
    ``None`` for an object that carries none."""
    return _otio._sync_guid(node)


def set_guid(node, guid):
    """Give ``node`` the GUID ``guid``, replacing any it had."""
    _otio._sync_set_guid(node, guid)


def stamp(timeline):
    """Give every object sync addresses under ``timeline`` a GUID, if it
    lacks one or shares one with an object met earlier, and return how many
    it gave one to."""
    return _otio._sync_stamp(timeline)


def find(timeline, guid):
    """The object under ``timeline`` that carries the GUID ``guid``, or
    ``None`` when none does.

    Raises :class:`ValueError` when two do; :func:`stamp` first to rule that
    out.
    """
    return _otio._sync_find(timeline, guid)


def diff(old_snapshot, new_snapshot, pretty=False):
    """The patches that take the snapshot ``old_snapshot`` to the snapshot
    ``new_snapshot``, as Furnace's ``otiodiff`` makes them, as a JSON list.

    With ``pretty``, the list is indented as Furnace's golden files are;
    otherwise it is compact. A change too large for patches, or one between
    sequences or back in generation, is one ``replace_timeline`` patch;
    :func:`diff_stats` says why.
    """
    return _otio._sync_diff(old_snapshot, new_snapshot, pretty)


def diff_stats(old_snapshot, new_snapshot):
    """What :func:`diff` does with the same snapshots, as a JSON object:
    ``insertOps``, ``moveOps``, ``removeOps``, ``setPropertyOps``,
    ``totalOps``, ``nodeCount``, ``fellBack`` and ``fallbackReason``."""
    return _otio._sync_diff_stats(old_snapshot, new_snapshot)


def apply(timeline, patches):
    """Apply the JSON list of patches ``patches`` to the timeline JSON
    ``timeline``, as Furnace's ``otiodiff`` does, and return the patched
    timeline as compact JSON."""
    return _otio._sync_apply(timeline, patches)


def apply_to(timeline, patches):
    """Apply the JSON list of patches ``patches`` to ``timeline`` in place.

    ``timeline`` stays the same object, as does every synced object in it
    that the patches keep; one they remove is gone afterwards. On failure
    the timeline is left as it was.
    """
    _otio._sync_apply_to(timeline, patches)


def begin(timeline, sequence_id, generation):
    """Start a transaction on ``timeline``, which is ``sequence_id`` at
    ``generation``: stamp any missing GUIDs, and return the timeline as it
    is as a snapshot.

    The snapshot is the transaction. Keep it, edit the timeline, and pass it
    to :func:`commit` for the patches, or to :func:`rollback` to undo the
    edits.
    """
    return _otio._sync_begin(timeline, sequence_id, generation)


def commit(timeline, before):
    """End the transaction :func:`begin` returned as ``before``, returning
    the patches that take the timeline as it was to the timeline as it is,
    the next generation, as a compact JSON list.

    An edit that changed nothing returns ``[]``.
    """
    return _otio._sync_commit(timeline, before)


def rollback(timeline, before):
    """End the transaction :func:`begin` returned as ``before``, putting the
    timeline back as it was.

    The timeline and every synced object that was in it stay the same
    objects.
    """
    _otio._sync_rollback(timeline, before)


def start_journal(timeline):
    """Start recording which objects any edit to the document ``timeline``
    is in touches.

    The journal records edits anywhere in that document; the timeline only
    says which. Starting it again does nothing.
    """
    _otio._sync_start_journal(timeline)


def stop_journal(timeline):
    """Stop recording edits to ``timeline``'s document, and forget what was
    recorded since the last commit."""
    _otio._sync_stop_journal(timeline)


def commit_journal(timeline):
    """Take what the journal recorded since it started or was last
    committed, run the ``document_changed`` hook on the document's root
    with it if anything changed, and return whether anything did.

    The hook's arguments are ``touched``, the GUIDs of the synced objects
    that were added or may have changed, and ``root_changed``.
    """
    return _otio._sync_commit_journal(timeline)
