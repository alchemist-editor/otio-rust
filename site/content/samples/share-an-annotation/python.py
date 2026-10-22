import json

import opentimelineio as otio
from opentimelineio.ext import ori, sync

# The cut both sides have: one shot, which sync knows by the GUID main/shot.
CUT = (
    '{"OTIO_SCHEMA":"Timeline.1","name":"review","metadata":{"sync":{"guid":"main"}},'
    '"tracks":{"OTIO_SCHEMA":"Stack.1","children":[{"OTIO_SCHEMA":"Track.1","name":"V1",'
    '"kind":"Video","children":[{"OTIO_SCHEMA":"Clip.2","name":"shot",'
    '"metadata":{"sync":{"guid":"main/shot"}},"source_range":{"OTIO_SCHEMA":"TimeRange.1",'
    '"start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},'
    '"duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}}]}]}}'
)

# What Furnace tells a reviewer who joins: who they are, and that they may
# comment. And what it tells the host when it opens the session.
WELCOME = (
    '{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{'
    '"selfId":"p_2","selfUserId":"u_bea","role":"viewer","roster":[],'
    '"session":{"id":"s_1","createdAt":0}}}'
)
HOST_WELCOME = (
    '{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{'
    '"selfId":"p_1","selfUserId":"u_ari","role":"host","roster":[],'
    '"session":{"id":"s_1","createdAt":0}}}'
)

# Where the reviewer drew: frame 10 of the shot, as the program shows it.
ANCHOR = {
    "sequenceId": "main", "capturedGeneration": 1, "clipGuid": "main/shot",
    "sourceTime": {"value": 10, "rate": 24},
    "capturedSequenceTime": {"value": 10, "rate": 24},
    "capturedAt": 1787742000000,
}

# One stroke: a PaintStart, its points and a PaintEnd sharing a uuid.
STROKE = [
    {"OTIO_SCHEMA": "PaintStart.1", "uuid": "s1", "rgba": [1, 0.25, 0.25, 1]},
    {"OTIO_SCHEMA": "PaintPoint.1", "uuid": "s1", "points": {
        "OTIO_SCHEMA": "PaintVertices.1", "x": [-0.5, 0, 0.5], "y": [0.2, 0, -0.2],
        "size": [0.01] * 3, "alpha": [1] * 3}},
    {"OTIO_SCHEMA": "PaintEnd.1", "uuid": "s1"},
]

# The comment as Furnace relays it once it has stored it, to everyone in the
# session. A real host reads this from its own socket.
RELAY = json.dumps({
    "v": 2, "type": "review_commit", "sessionId": "s_1", "senderId": "p_2",
    "seq": 1, "ts": 0, "order": 2,
    "payload": {
        "clientMutationId": "m1", "commentId": "c1", "authorUserId": "u_bea",
        "authorPresenceId": "p_2", "serverOrder": 1, "revision": 1, "createdAt": 0,
        "anchor": ANCHOR, "bodyMarkdown": "Too dark here.", "annotationCommands": STROKE,
    },
})


def send(session):
    # Stands in for the network: a real program writes every frame the
    # session has waiting to its WebSocket to Furnace.
    while (frame := session.poll_transmit()) is not None:
        print("send", len(frame), "characters")


# ORI Annotations is an extension; enabling it enables sync too.
ori.enable()

# The reviewer joins with the right to comment.
reviewer = sync.Session(
    token="join-code",
    identity={"capabilities": ["review.comment.create", "review.annotation.create"]},
)
reviewer.connect()
send(reviewer)
reviewer.receive(WELCOME)
while reviewer.poll_event() is not None:
    pass

# Commit the stroke with a note. The session sends it again after every
# reconnect until Furnace stores it, so it is not lost.
seen = otio.adapters.read_from_string(CUT, "otio_json")
print("committed", ori.commit(reviewer, seen, ANCHOR, "Too dark here.", STROKE))
send(reviewer)

# The host hears the stored comment and folds it into its timeline.
host = sync.Session(token="host-token", identity={"capabilities": ["review.commit.receive"]})
host.connect()
send(host)
host.receive(HOST_WELCOME)
host.receive(RELAY)
timeline = otio.adapters.read_from_string(CUT, "otio_json")
while (event := host.poll_event()) is not None:
    # A stroke still being drawn: draw it over the picture, then drop it.
    partial = ori.read_partial(event)
    if partial is not None:
        print("drawing", len(partial["events"]), "commands")
    elif event["type"] == "review_relay":
        clip, already = ori.fold(timeline, event)
        print("folded", ori.describe(clip), "already there:", already)
        print("the stroke belongs to", ori.owner_of(clip, "s1"))
