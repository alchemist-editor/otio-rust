#!/usr/bin/env python3
"""ORI's own end state for each of its recordings, for `tests/ori_replay.rs`.

Replays each recording into ORI's headless ``SyncManager`` the way ORI's
``sync_recorder`` player does: the player answers the joiner's discovery as
master, sends the recording's first ``STATE_SNAPSHOT`` addressed to the
joiner, then sends every message that is not a ``LiveSession.1`` one, in
order. Writes the joiner's timelines, active timeline, playback state and
display state to ``<out>/<recording>.json``, with each timeline read and
written again by upstream OpenTimelineIO so the comparison is of content.

    python3 reference.py <ORIAnnotations>/python recordings expected

Needs ``opentimelineio`` and ORI's ``otio_sync_core`` (from the checkout
named by the first argument).
"""

import copy
import json
import os
import sys

SELF = "otio-rust-replay"
PLAYER = "ori-recording-player"
SESSION = "replay"


class Network:
    """ORI's network protocol, handing the manager one message at a time."""

    def __init__(self):
        self.inbox = []

    def send_payload(self, payload):
        pass

    def receive_payloads(self):
        out, self.inbox = self.inbox, []
        return out

    def stop(self):
        pass


def load(path):
    """The first snapshot and the replayed messages, as ORI's player reads them."""
    snapshot = None
    events = []
    with open(path, encoding="utf-8") as recording:
        for line in recording:
            line = line.strip()
            if not line:
                continue
            envelope = json.loads(line)["payload"]
            inner = envelope.get("payload", {})
            if inner.get("command_schema") == "LiveSession.1":
                if inner.get("command", {}).get("event") == "STATE_SNAPSHOT" and snapshot is None:
                    snapshot = envelope
                continue
            events.append(envelope)
    return snapshot, events


def from_player(envelope):
    envelope = copy.deepcopy(envelope)
    envelope["source_guid"] = PLAYER
    envelope["session"] = SESSION
    return envelope


def canonical(timeline):
    import opentimelineio as otio

    text = json.dumps(timeline) if isinstance(timeline, dict) else otio.adapters.write_to_string(timeline)
    return json.loads(otio.adapters.write_to_string(otio.adapters.read_from_string(text, "otio_json")))


def without_timestamp(state):
    if not isinstance(state, dict):
        return state
    return {key: value for key, value in state.items() if key != "sync_timestamp"}


def replay(path):
    from otio_sync_core.manager import STATE_SYNCED, SyncManager

    snapshot, events = load(path)
    network = Network()
    manager = SyncManager(session_id=SESSION, self_guid=SELF, network=network, app_name="replay")
    manager.start_session()
    network.inbox.append({
        "session": SESSION,
        "source_guid": PLAYER,
        "schema": "SYNC_REVIEW_1.0",
        "payload": {
            "command_schema": "LiveSession.1",
            "command": {"event": "I_AM_MASTER", "payload": {"master_guid": PLAYER}},
        },
    })
    manager.tick()
    snapshot = from_player(snapshot)
    snapshot["payload"]["command"]["payload"]["target_guid"] = SELF
    network.inbox.append(snapshot)
    manager.tick()
    assert manager.status == STATE_SYNCED, manager.status
    for event in events:
        network.inbox.append(from_player(event))
        manager.tick()
    state = manager.export_state()
    return {
        "timelines": {guid: canonical(timeline) for guid, timeline in state["timelines"].items()},
        "active_timeline_guid": state.get("active_timeline_guid"),
        "playback_state": without_timestamp(state.get("playback_state")),
        "display_state": without_timestamp(state.get("display_state")),
    }


def main():
    ori_python, recordings, out = sys.argv[1:4]
    sys.path.insert(0, ori_python)
    os.makedirs(out, exist_ok=True)
    for name in sorted(os.listdir(recordings)):
        if not name.endswith(".jsonl"):
            continue
        state = replay(os.path.join(recordings, name))
        with open(os.path.join(out, name[: -len(".jsonl")] + ".json"), "w", encoding="utf-8") as expected:
            json.dump(state, expected, indent=1, sort_keys=True)
            expected.write("\n")


if __name__ == "__main__":
    main()
