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

A :class:`Session` shares timelines with other programs through Alchemist's
Furnace server. It owns no socket: open one with any WebSocket library, hand
the session each frame that arrives, and send each frame it gives back.
"""

import json
import time

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
    "Session",
    "handle",
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


class Session:
    """A sync session, speaking Furnace's protocol v2.

    ``token`` is the token the server issued. ``identity`` is a dictionary of
    who the client is: ``displayName``, ``app``, ``appVersion``,
    ``credential``, ``deviceId``, ``clientKind``, ``formFactor``,
    ``deviceName`` and ``capabilities``, any of which may be left out.
    ``encodings`` lists the compression to accept (``"br"``, ``"gzip"``);
    every kind unless given, and none for an empty list.

    The session owns no socket. Once one is open, call :meth:`connect`, send
    every frame :meth:`poll_transmit` returns, hand every frame that arrives
    to :meth:`receive`, and act on what :meth:`poll_event` returns, passing
    timeline and joined events to :func:`handle`.

    A refusal raises :class:`ValueError` (a client without the lease, or JSON
    that is not what was expected) or :class:`RuntimeError` (a hook script
    that stopped or failed); a broken connection raises
    :class:`ConnectionError`, after which close the socket, call
    :meth:`disconnected`, and connect again.
    """

    def __init__(self, token="", identity=None, encodings=None):
        settings = {"token": token}
        if identity is not None:
            settings["identity"] = dict(identity)
        if encodings is not None:
            settings["encodings"] = list(encodings)
        self._session = _otio._SyncSession(json.dumps(settings))

    def connect(self):
        """Start joining on a new connection: call it once the socket is
        open, and again after a reconnect. The hello is the next frame to
        send."""
        self._session.connect()

    def receive(self, data, binary=None):
        """Take in a frame that arrived: ``str`` for a text frame, ``bytes``
        for a binary one, unless ``binary`` says otherwise."""
        if isinstance(data, str):
            data = data.encode("utf-8")
            binary = False if binary is None else binary
        elif binary is None:
            binary = True
        self._session.receive(bytes(data), binary)

    def poll_transmit(self, now=None):
        """The next frame to send, or ``None``: ``str`` for a text frame and
        ``bytes`` for a binary one. ``now`` is the time in Unix milliseconds,
        the clock's unless given. Call it until ``None`` after every other
        call."""
        if now is None:
            now = int(time.time() * 1000)
        frame = self._session.poll_transmit(now)
        if frame is None:
            return None
        data, binary = frame
        return bytes(data) if binary else bytes(data).decode("utf-8")

    def poll_event(self):
        """The next event as a dictionary, or ``None``. Its ``"type"`` is
        ``joined``, ``timeline``, ``playhead``, ``selection``, ``roster``,
        ``leader``, ``review_relay``, ``review_committed``,
        ``review_rejected``, ``error``, ``ended`` or ``message``."""
        event = self._session.poll_event()
        return None if event is None else json.loads(event)

    def disconnected(self):
        """The socket closed: the session waits for :meth:`connect`, keeping
        its timelines and unacknowledged review commits."""
        self._session.disconnected()

    def leave(self):
        """Leave the session: the goodbye is the next frame to send."""
        self._session.leave()

    @property
    def state(self):
        """Where the session stands, as a dictionary: ``joined``, ``ended``,
        ``sessionId``, ``selfId``, ``selfUserId``, ``role``, ``leader``,
        ``peers``, ``activeSequence``, ``sequences``, ``pendingReviews``,
        ``version`` and ``encodings``."""
        return json.loads(self._session.state())

    def timeline(self, sequence_id):
        """The latest snapshot of ``sequence_id`` the session sent or
        received, as JSON text, or ``None``."""
        return self._session.timeline(sequence_id)

    def may_send(self, channel):
        """Whether the session may send on ``channel`` now."""
        return self._session.may_send(channel)

    def publish(self, timeline, sequence_id):
        """Send ``timeline`` as the sequence ``sequence_id``, returning the
        patches from the version sent before as a JSON list. Only the host
        may. A timeline that has not changed sends nothing and returns
        ``[]``, unless it switches the active sequence."""
        return self._session.publish(timeline, sequence_id)

    def set_playhead(self, sequence_id, frame, playing=False, rate=1.0):
        """Move this client's playhead in ``sequence_id`` to ``frame``."""
        self._session.set_playhead(sequence_id, float(frame), playing, float(rate))

    def set_selection(self, sequence_id, guids):
        """Select the objects in ``sequence_id`` with these GUIDs."""
        self._session.set_selection(sequence_id, list(guids))

    def commit_review(self, commit):
        """Commit a review comment, a dictionary with ``anchor``,
        ``bodyMarkdown``, ``annotationCommands`` and optionally
        ``clientMutationId``, returning its id. It is sent again after every
        reconnect until the server acknowledges or refuses it."""
        return self._session.commit_review(json.dumps(commit))

    def cancel_review(self, client_mutation_id):
        """Stop sending a review commit; whether it was waiting."""
        return self._session.cancel_review(client_mutation_id)

    def set_leader(self, participant):
        """Make ``participant`` the playback leader. Only the host may."""
        self._session.set_leader(participant)

    def set_muted(self, participant, muted):
        """Mute or unmute ``participant``. Only the host may."""
        self._session.set_muted(participant, muted)

    def send(self, channel, kind, payload):
        """Send a message of type ``kind`` on ``channel``. ``payload`` is
        JSON text, or anything :func:`json.dumps` takes."""
        if not isinstance(payload, str):
            payload = json.dumps(payload)
        self._session.send(channel, kind, payload)

    def register_channel(self, name, delivery):
        """Register the channel ``name``, for another extension's messages,
        delivered ``"latest_wins"``, ``"ordered"`` or ``"transient"``."""
        self._session.register_channel(name, delivery)


def handle(timeline, event):
    """Act on ``event``, from :meth:`Session.poll_event`, in ``timeline``,
    the program's copy of the timeline it names, and return what it did as a
    dictionary: ``handled`` is ``applied``, ``replaced``, ``rejected`` (with
    the script's ``message``), ``joined`` or ``ignored``.

    A timeline event runs ``sync_before_apply``, whose scripts may refuse
    it, applies its patches in place so the timeline and the objects that
    survive stay the same objects, falling back to the whole snapshot if
    the patches do not fit (``replaced``), links media on what it inserted,
    then runs ``sync_after_apply``. A joined event runs ``sync_joined``.
    Every other event is ``ignored``.
    """
    if not isinstance(event, str):
        event = json.dumps(event)
    return json.loads(_otio._sync_handle(timeline, event))
