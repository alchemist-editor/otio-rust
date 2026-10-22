import { init, Timeline, deserializeJsonFromString } from "@alchemist-edit/otio";
import * as ori from "@alchemist-edit/otio/ext/ori";
import * as sync from "@alchemist-edit/otio/ext/sync";

// The cut both sides have: one shot, which sync knows by the GUID main/shot.
const CUT = `{"OTIO_SCHEMA":"Timeline.1","name":"review","metadata":{"sync":{"guid":"main"}},
 "tracks":{"OTIO_SCHEMA":"Stack.1","children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video",
  "children":[{"OTIO_SCHEMA":"Clip.2","name":"shot","metadata":{"sync":{"guid":"main/shot"}},
   "source_range":{"OTIO_SCHEMA":"TimeRange.1",
    "start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},
    "duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}}]}]}}`;

// What Furnace tells a reviewer who joins: who they are, and that they may
// comment. And what it tells the host when it opens the session.
const WELCOME = '{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","selfUserId":"u_bea","role":"viewer","roster":[],"session":{"id":"s_1","createdAt":0}}}';
const HOST_WELCOME = '{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_1","selfUserId":"u_ari","role":"host","roster":[],"session":{"id":"s_1","createdAt":0}}}';

// Where the reviewer drew: frame 10 of the shot, as the program shows it.
const anchor = {
  sequenceId: "main", capturedGeneration: 1, clipGuid: "main/shot",
  sourceTime: { value: 10, rate: 24 }, capturedSequenceTime: { value: 10, rate: 24 },
  capturedAt: 1787742000000,
};

// One stroke: a PaintStart, its points and a PaintEnd sharing a uuid.
const stroke = [
  { OTIO_SCHEMA: "PaintStart.1", uuid: "s1", rgba: [1, 0.25, 0.25, 1] },
  { OTIO_SCHEMA: "PaintPoint.1", uuid: "s1", points: {
    OTIO_SCHEMA: "PaintVertices.1", x: [-0.5, 0, 0.5], y: [0.2, 0, -0.2],
    size: [0.01, 0.01, 0.01], alpha: [1, 1, 1] } },
  { OTIO_SCHEMA: "PaintEnd.1", uuid: "s1" },
];

// The comment as Furnace relays it once it has stored it, to everyone in
// the session. A real host reads this from its own socket.
const RELAY = JSON.stringify({
  v: 2, type: "review_commit", sessionId: "s_1", senderId: "p_2", seq: 1, ts: 0, order: 2,
  payload: {
    clientMutationId: "m1", commentId: "c1", authorUserId: "u_bea", authorPresenceId: "p_2",
    serverOrder: 1, revision: 1, createdAt: 0,
    anchor, bodyMarkdown: "Too dark here.", annotationCommands: stroke,
  },
});

// Stands in for the network: a real program writes every frame the session
// has waiting to its WebSocket to Furnace.
const send = (session: number): void => {
  for (let frame = sync.sessionPollTransmit(session, Date.now()); frame !== undefined;
    frame = sync.sessionPollTransmit(session, Date.now())) {
    console.log(`send ${frame.bytes.length} bytes`);
  }
};
const receive = (session: number, frame: string): void =>
  sync.sessionReceive(session, new TextEncoder().encode(frame), false);

await init();
// ORI Annotations is an extension; enabling it enables sync too.
ori.enable();

// The reviewer joins with the right to comment.
const reviewer = sync.sessionNew(JSON.stringify({
  token: "join-code",
  identity: { capabilities: ["review.comment.create", "review.annotation.create"] },
}));
sync.sessionConnect(reviewer);
send(reviewer);
receive(reviewer, WELCOME);
while (sync.sessionPollEvent(reviewer) !== undefined) {
  // Nothing to show yet.
}

// Commit the stroke with a note. The session sends it again after every
// reconnect until Furnace stores it, so it is not lost.
const seen = deserializeJsonFromString(CUT) as Timeline;
const id = ori.commit(reviewer, seen, JSON.stringify(anchor), "Too dark here.", JSON.stringify(stroke), "");
console.log(`committed ${id}`);
send(reviewer);

// The host hears the stored comment and folds it into its timeline.
const host = sync.sessionNew('{"token":"host-token","identity":{"capabilities":["review.commit.receive"]}}');
sync.sessionConnect(host);
send(host);
receive(host, HOST_WELCOME);
receive(host, RELAY);
const timeline = deserializeJsonFromString(CUT) as Timeline;
for (let event = sync.sessionPollEvent(host); event !== undefined; event = sync.sessionPollEvent(host)) {
  // A stroke still being drawn: draw it over the picture, then drop it.
  const partial = ori.readPartial(event);
  if (partial !== undefined) {
    console.log(`drawing ${partial}`);
  } else if ((JSON.parse(event) as { type: string }).type === "review_relay") {
    const { clip, already } = ori.fold(timeline, event);
    console.log(`folded ${ori.describe(clip)} already there: ${already}`);
    console.log(`the stroke belongs to ${ori.ownerOf(clip, "s1")}`);
  }
}
sync.sessionClose(reviewer);
sync.sessionClose(host);
