package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"time"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
	"github.com/alchemist-editor/otio-rust/sdk/go/ext/ori"
	"github.com/alchemist-editor/otio-rust/sdk/go/ext/sync"
)

// The cut both sides have: one shot, which sync knows by the GUID main/shot.
const cut = `{"OTIO_SCHEMA":"Timeline.1","name":"review","metadata":{"sync":{"guid":"main"}},
 "tracks":{"OTIO_SCHEMA":"Stack.1","children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video",
  "children":[{"OTIO_SCHEMA":"Clip.2","name":"shot","metadata":{"sync":{"guid":"main/shot"}},
   "source_range":{"OTIO_SCHEMA":"TimeRange.1",
    "start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},
    "duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}}]}]}}`

// What Furnace tells a reviewer who joins: who they are, and that they may comment.
const welcome = `{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","selfUserId":"u_bea","role":"viewer","roster":[],"session":{"id":"s_1","createdAt":0}}}`

// Where the reviewer drew: frame 10 of the shot, as the program shows it.
const anchor = `{"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/shot",
 "sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},
 "capturedAt":1787742000000}`

// One stroke: a PaintStart, its points and a PaintEnd sharing a uuid.
const stroke = `[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0.25,0.25,1]},
 {"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1",
  "x":[-0.5,0,0.5],"y":[0.2,0,-0.2],"size":[0.01,0.01,0.01],"alpha":[1,1,1]}},
 {"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}]`

// What Furnace tells the host when it opens the session.
const hostWelcome = `{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_1","selfUserId":"u_ari","role":"host","roster":[],"session":{"id":"s_1","createdAt":0}}}`

// The comment as Furnace relays it once it has stored it, to everyone in
// the session. A real host reads this from its own socket.
const relay = `{"v":2,"type":"review_commit","sessionId":"s_1","senderId":"p_2","seq":1,"ts":0,"order":2,"payload":{"clientMutationId":"m1","commentId":"c1","authorUserId":"u_bea","authorPresenceId":"p_2","serverOrder":1,"revision":1,"createdAt":0,"anchor":` + anchor + `,"bodyMarkdown":"Too dark here.","annotationCommands":` + stroke + `}}`

// sendAll stands in for the network: a real program writes every frame the
// session has waiting to its WebSocket to Furnace.
func sendAll(session uint64) {
	for {
		frame, _, err := sync.SessionPollTransmit(session, time.Now().UnixMilli())
		if errors.Is(err, otio.ErrNoValue) {
			return
		}
		if err != nil {
			log.Fatal(err)
		}
		fmt.Printf("send %d bytes\n", len(frame))
	}
}

// events is every event the session has for the program now.
func events(session uint64) []string {
	var all []string
	for {
		event, err := sync.SessionPollEvent(session)
		if errors.Is(err, otio.ErrNoValue) {
			return all
		}
		if err != nil {
			log.Fatal(err)
		}
		all = append(all, event)
	}
}

func main() {
	// ORI Annotations is an extension; enabling it enables sync too.
	if err := ori.Enable(); err != nil {
		log.Fatal(err)
	}

	// The reviewer joins with the right to comment.
	reviewer, err := sync.SessionNew(`{"token":"join-code","identity":{"capabilities":["review.comment.create","review.annotation.create"]}}`)
	if err != nil {
		log.Fatal(err)
	}
	if err := sync.SessionConnect(reviewer); err != nil {
		log.Fatal(err)
	}
	sendAll(reviewer)
	if err := sync.SessionReceive(reviewer, []byte(welcome), false); err != nil {
		log.Fatal(err)
	}
	events(reviewer)

	// Commit the stroke with a note. The session sends it again after every
	// reconnect until Furnace stores it, so it is not lost.
	seen, err := otio.FromJSON(cut)
	if err != nil {
		log.Fatal(err)
	}
	defer seen.Close()
	id, err := ori.Commit(reviewer, seen, anchor, "Too dark here.", stroke, "")
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println("committed", id)
	sendAll(reviewer)

	// The host hears the stored comment and folds it into its timeline.
	host, err := sync.SessionNew(`{"token":"host-token","identity":{"capabilities":["review.commit.receive"]}}`)
	if err != nil {
		log.Fatal(err)
	}
	if err := sync.SessionConnect(host); err != nil {
		log.Fatal(err)
	}
	sendAll(host)
	for _, frame := range []string{hostWelcome, relay} {
		if err := sync.SessionReceive(host, []byte(frame), false); err != nil {
			log.Fatal(err)
		}
	}
	timeline, err := otio.FromJSON(cut)
	if err != nil {
		log.Fatal(err)
	}
	defer timeline.Close()
	for _, event := range events(host) {
		// A stroke still being drawn: draw it over the picture, then drop it.
		if partial, err := ori.ReadPartial(event); err == nil {
			fmt.Println("drawing", partial)
			continue
		}
		var kind struct {
			Type string `json:"type"`
		}
		if err := json.Unmarshal([]byte(event), &kind); err != nil || kind.Type != "review_relay" {
			continue
		}
		clip, already, err := ori.Fold(timeline, event)
		if err != nil {
			log.Fatal(err)
		}
		about, err := ori.Describe(clip)
		if err != nil {
			log.Fatal(err)
		}
		fmt.Println("folded", about, "already there:", already)
		owner, err := ori.OwnerOf(clip, "s1")
		if err != nil {
			log.Fatal(err)
		}
		fmt.Println("the stroke belongs to", owner)
	}

	for _, session := range []uint64{reviewer, host} {
		if err := sync.SessionClose(session); err != nil {
			log.Fatal(err)
		}
	}
}
