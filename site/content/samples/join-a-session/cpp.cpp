#include <cstdint>
#include <iostream>
#include <string>
#include <vector>

#include <opentimelineio/ext/sync.hpp>
#include <opentimelineio/otio.hpp>

namespace sync = otio::ext::sync;

// What Furnace sends a viewer joining a session whose host shared one
// timeline.
const char *const WELCOME = R"({"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","role":"viewer","roster":[{"id":"p_1","displayName":"Host","role":"host","joinedAt":0,"connectionState":"connected","muted":false}],"session":{"id":"s_1","createdAt":0},"snapshot":{"sequenceId":"main","generation":1,"otioJson":{"OTIO_SCHEMA":"Timeline.1","name":"Coastline","metadata":{"sync":{"guid":"t-1"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"s-1"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"v-1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Waves","metadata":{"sync":{"guid":"c-1"}}}]}]}}}}})";

// Sends every frame the session has waiting. A real program writes each one
// to its WebSocket to Furnace; this one only says what it would send.
void send_frames(std::uint64_t session) {
    while (const auto frame = sync::session_poll_transmit(session, 0)) {
        std::cout << "send " << frame->bytes.size() << " bytes as "
                  << (frame->binary ? "binary" : "text") << "\n";
    }
}

// An event's type. A real program reads the event with its JSON library.
std::string type_of(const std::string &event) {
    const std::string key = R"("type":")";
    const std::size_t start = event.find(key) + key.size();
    return event.substr(start, event.find('"', start) - start);
}

int main() {
    sync::enable();

    // A session owns no socket. Connect once yours is open: the hello is
    // the first frame to send.
    const std::uint64_t session =
        sync::session_new(R"({"token":"join-code","identity":{"displayName":"Ada"}})");
    sync::session_connect(session);
    send_frames(session);

    // The frame Furnace sends back.
    const std::string welcome = WELCOME;
    sync::session_receive(session, std::vector<std::uint8_t>(welcome.begin(), welcome.end()), false);

    // The program's own copy of the timeline starts empty. Each event goes
    // to handle, which applies what it can and says what it did.
    const otio::Timeline timeline =
        otio::from_json(R"({"OTIO_SCHEMA":"Timeline.1"})").as<otio::Timeline>().value();
    while (const auto event = sync::session_poll_event(session)) {
        std::cout << "event " << type_of(*event) << ": " << sync::handle(timeline, *event) << "\n";
    }
    std::cout << "joined, showing " << timeline.name() << "\n";

    // Move the playhead, and send what that says to everyone else.
    sync::session_set_playhead(session, "main", 24, true, 1);
    send_frames(session);

    // Leaving queues a goodbye to send before the socket closes. Closing
    // the session afterwards releases it.
    sync::session_leave(session);
    send_frames(session);
    sync::session_close(session);
    return 0;
}
