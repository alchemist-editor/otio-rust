#!/usr/bin/env python3
"""ORI's own peer on a RabbitMQ session, driven over stdin for `tests/ori_live.rs`.

Runs ORI's headless ``SyncManager`` over its ``RabbitMQNetwork`` the way ORI's
sync viewer runs it: ticking, rediscovering while no master answers, electing
itself after the discovery timeout, and answering state requests as master.
Reads one JSON command per line on stdin and answers each with one JSON line
on stdout:

* ``{"op": "status"}`` -> ``{"status", "is_master", "master"}``
* ``{"op": "load", "timeline": {...}}`` registers and announces a timeline
* ``{"op": "set", "target", "path", "value"}`` sets a property
* ``{"op": "playback", "state": {...}, "timeline": guid}`` sends playback
* ``{"op": "state"}`` -> the timelines (as upstream OpenTimelineIO writes
  them), active timeline and playback state
* ``{"op": "quit"}`` leaves the session

    python3 live_peer.py <ORIAnnotations>/python <amqp-url> <session> <guid>
"""

import json
import select
import sys
import time

DISCOVERY_TIMEOUT = 2.0


def canonical(timeline):
    import opentimelineio as otio

    return json.loads(otio.adapters.write_to_string(timeline))


def main():
    ori_python, url, session, guid = sys.argv[1:5]
    sys.path.insert(0, ori_python)
    import opentimelineio as otio
    from otio_sync_core import authority
    from otio_sync_core.manager import STATE_DISCOVERING, SyncManager
    from otio_sync_core.rabbitmq_network import RabbitMQNetwork

    network = RabbitMQNetwork(host=url, session_id=session, self_guid=guid)
    if not network.wait_until_ready(10.0):
        raise SystemExit("the broker did not bind a queue")
    manager = SyncManager(session_id=session, self_guid=guid, network=network, app_name="ori-live-peer")
    manager.start_session()
    discovery = time.monotonic() + DISCOVERY_TIMEOUT

    def answer(value):
        sys.stdout.write(json.dumps(value) + "\n")
        sys.stdout.flush()

    def claim(channel):
        if channel in authority.LEASE_CHANNELS:
            manager.claim_category(channel)

    while True:
        for action, data in manager.tick():
            if action == "state_request_received":
                manager.send_state_snapshot(data)
        if manager.status == STATE_DISCOVERING and time.monotonic() > discovery:
            manager.elect_self_as_master()
        ready, _, _ = select.select([sys.stdin], [], [], 0.05)
        if not ready:
            continue
        line = sys.stdin.readline()
        if not line:
            break
        command = json.loads(line)
        op = command["op"]
        if op == "status":
            answer({"status": manager.status, "is_master": manager.is_master, "master": manager.master_guid})
        elif op == "load":
            timeline = otio.adapters.read_from_string(json.dumps(command["timeline"]), "otio_json")
            manager.register_timeline(timeline)
            claim(authority.CHANNEL_STRUCTURE)
            tl_guid = timeline.metadata["sync"]["guid"]
            answer({"guid": tl_guid, "sent": manager.broadcast_add_timeline(tl_guid)})
        elif op == "set":
            claim(authority.CHANNEL_STRUCTURE)
            manager.set_property(command["target"], command["path"], command["value"])
            answer({"ok": True})
        elif op == "playback":
            claim(authority.CHANNEL_POSITION)
            answer({"sent": manager.broadcast_playback_state(command["state"], command.get("timeline"))})
        elif op == "state":
            answer({
                "timelines": {key: canonical(value) for key, value in manager.timelines.items()},
                "active_timeline_guid": manager.active_timeline_guid,
                "playback_state": manager.playback_state,
            })
        elif op == "quit":
            break
        else:
            answer({"error": f"unknown op {op}"})
    manager.close()


if __name__ == "__main__":
    main()
