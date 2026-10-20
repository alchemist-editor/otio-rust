import { init, Timeline, deserializeJsonFromString } from "@alchemist-edit/otio";
import * as sync from "@alchemist-edit/otio/ext/sync";

// What Furnace sends a viewer joining a session whose host shared one
// timeline.
const WELCOME =
  '{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","role":"viewer","roster":[{"id":"p_1","displayName":"Host","role":"host","joinedAt":0,"connectionState":"connected","muted":false}],"session":{"id":"s_1","createdAt":0},"snapshot":{"sequenceId":"main","generation":1,"otioJson":{"OTIO_SCHEMA":"Timeline.1","name":"Coastline","metadata":{"sync":{"guid":"t-1"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"s-1"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"v-1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Waves","metadata":{"sync":{"guid":"c-1"}}}]}]}}}}}';

await init();
sync.enable();

// A session owns no socket. A real program writes each frame to its
// WebSocket to Furnace; this one only says what it would send.
const send = (session: number): void => {
  for (let frame = sync.sessionPollTransmit(session, Date.now()); frame !== undefined;
    frame = sync.sessionPollTransmit(session, Date.now())) {
    console.log(`send ${frame.bytes.length} bytes as ${frame.binary ? "binary" : "text"}`);
  }
};

const session = sync.sessionNew('{"token":"join-code","identity":{"displayName":"Ada"}}');
sync.sessionConnect(session);
send(session);

// The frame Furnace sends back.
sync.sessionReceive(session, new TextEncoder().encode(WELCOME), false);

// The program's own copy of the timeline, empty until the host's arrives.
const timeline = deserializeJsonFromString('{"OTIO_SCHEMA": "Timeline.1"}');
if (!(timeline instanceof Timeline)) {
  throw new Error("not a timeline");
}
for (let event = sync.sessionPollEvent(session); event !== undefined; event = sync.sessionPollEvent(session)) {
  const { type } = JSON.parse(event) as { type: string };
  console.log(`${type}: ${sync.handle(timeline, event)}`);
}
console.log(`joined, watching ${timeline.name}`);

// Moving the playhead is one more frame to send.
sync.sessionSetPlayhead(session, "main", 24, true, 1);
send(session);

// Say goodbye before closing the socket.
sync.sessionLeave(session);
send(session);
sync.sessionClose(session);
