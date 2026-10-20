import opentimelineio as otio
from opentimelineio.ext import sync

# The frame Furnace sends a viewer joining a session whose host has shared
# one timeline: the welcome, holding the timeline as it is.
WELCOME = (
    '{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{'
    '"selfId":"p_2","role":"viewer","roster":[{"id":"p_1","displayName":"Host","role":"host",'
    '"joinedAt":0,"connectionState":"connected","muted":false}],"session":{"id":"s_1","createdAt":0},'
    '"snapshot":{"sequenceId":"main","generation":1,"otioJson":{"OTIO_SCHEMA":"Timeline.1",'
    '"name":"Coastline","metadata":{"sync":{"guid":"t-1"}},"tracks":{"OTIO_SCHEMA":"Stack.1",'
    '"metadata":{"sync":{"guid":"s-1"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1",'
    '"kind":"Video","metadata":{"sync":{"guid":"v-1"}},"children":[{"OTIO_SCHEMA":"Clip.2",'
    '"name":"Waves","metadata":{"sync":{"guid":"c-1"}}}]}]}}}}}'
)


def send(session):
    # A real program writes each frame to its WebSocket to Furnace: text
    # frames as text, binary ones as binary.
    while (frame := session.poll_transmit()) is not None:
        kind = "text" if isinstance(frame, str) else "binary"
        print(f"send a {kind} frame of {len(frame)} characters or bytes")


sync.enable()

# The session owns no socket. Open one to Furnace, then connect: the hello
# is the first frame to send.
session = sync.Session(token="join-code", identity={"displayName": "Ada"})
session.connect()
send(session)

# Hand the session every frame that arrives. This is the one Furnace sends
# back.
session.receive(WELCOME)

# The program's copy of the timeline starts empty. Handling the events puts
# the host's timeline in it, and keeps it current from then on.
timeline = otio.schema.Timeline()
while (event := session.poll_event()) is not None:
    print(event["type"], sync.handle(timeline, event)["handled"])
print("the viewer now shows", timeline.name)

# Everyone in the session sees where this viewer's playhead is.
session.set_playhead("main", 24, playing=True)
send(session)

session.leave()
send(session)
