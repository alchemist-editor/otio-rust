package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"time"

	otio "github.com/alchemist-editor/otio-rust/sdk/go"
	"github.com/alchemist-editor/otio-rust/sdk/go/ext/sync"
)

// The frame Furnace sends back to a viewer joining a session whose host
// shares one timeline.
const welcome = `{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","role":"viewer","roster":[{"id":"p_1","displayName":"Host","role":"host","joinedAt":0,"connectionState":"connected","muted":false}],"session":{"id":"s_1","createdAt":0},"snapshot":{"sequenceId":"main","generation":1,"otioJson":{"OTIO_SCHEMA":"Timeline.1","name":"Coastline","metadata":{"sync":{"guid":"t-1"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"s-1"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"v-1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Waves","metadata":{"sync":{"guid":"c-1"}}}]}]}}}}}`

// send stands in for the network. A real program writes the frame to its
// WebSocket to Furnace, as a binary or a text message.
func send(frame []byte, binary bool) {
	kind := "text"
	if binary {
		kind = "binary"
	}
	fmt.Printf("send %d bytes (%s)\n", len(frame), kind)
}

// sendAll sends every frame the session has waiting.
func sendAll(session uint64) {
	for {
		frame, binary, err := sync.SessionPollTransmit(session, time.Now().UnixMilli())
		if errors.Is(err, otio.ErrNoValue) {
			return
		}
		if err != nil {
			log.Fatal(err)
		}
		send(frame, binary)
	}
}

func main() {
	if err := sync.Enable(); err != nil {
		log.Fatal(err)
	}

	// The session owns no socket. Connect once the program's own is open.
	session, err := sync.SessionNew(`{"token":"join-code","identity":{"displayName":"Ada"}}`)
	if err != nil {
		log.Fatal(err)
	}
	if err := sync.SessionConnect(session); err != nil {
		log.Fatal(err)
	}
	sendAll(session)

	// This is the frame Furnace sends back.
	if err := sync.SessionReceive(session, []byte(welcome), false); err != nil {
		log.Fatal(err)
	}

	// The program's copy of the timeline starts empty; the events fill it.
	timeline, err := otio.FromJSON(`{"OTIO_SCHEMA":"Timeline.1"}`)
	if err != nil {
		log.Fatal(err)
	}
	defer timeline.Close()
	for {
		event, err := sync.SessionPollEvent(session)
		if errors.Is(err, otio.ErrNoValue) {
			break
		}
		if err != nil {
			log.Fatal(err)
		}
		var kind struct {
			Type string `json:"type"`
		}
		if err := json.Unmarshal([]byte(event), &kind); err != nil {
			log.Fatal(err)
		}
		handled, err := sync.Handle(timeline, event)
		if err != nil {
			log.Fatal(err)
		}
		fmt.Println(kind.Type, handled)
	}
	name, err := timeline.Name()
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println("the timeline arrived:", name)

	// Moving the playhead tells everyone else where this viewer is.
	if err := sync.SessionSetPlayhead(session, "main", 24, true, 1); err != nil {
		log.Fatal(err)
	}
	sendAll(session)

	// Leaving queues a goodbye; send it before closing the session.
	if err := sync.SessionLeave(session); err != nil {
		log.Fatal(err)
	}
	sendAll(session)
	if err := sync.SessionClose(session); err != nil {
		log.Fatal(err)
	}
}
