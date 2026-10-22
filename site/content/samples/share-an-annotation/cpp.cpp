#include <cstdint>
#include <iostream>
#include <string>
#include <vector>

#include <opentimelineio/ext/ori.hpp>
#include <opentimelineio/ext/sync.hpp>
#include <opentimelineio/otio.hpp>

namespace ori = otio::ext::ori;
namespace sync = otio::ext::sync;

// The cut both sides have: one shot, which sync knows by the GUID
// main/shot.
const char *const CUT = R"({"OTIO_SCHEMA":"Timeline.1","name":"review","metadata":{"sync":{"guid":"main"}},"tracks":{"OTIO_SCHEMA":"Stack.1","children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","children":[{"OTIO_SCHEMA":"Clip.2","name":"shot","metadata":{"sync":{"guid":"main/shot"}},"source_range":{"OTIO_SCHEMA":"TimeRange.1","start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},"duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}}]}]}})";

// What Furnace tells a reviewer who joins: who they are, and that they may
// comment.
const char *const WELCOME = R"({"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","selfUserId":"u_bea","role":"viewer","roster":[],"session":{"id":"s_1","createdAt":0}}})";

// What Furnace tells the host when it opens the session.
const char *const HOST_WELCOME = R"({"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_1","selfUserId":"u_ari","role":"host","roster":[],"session":{"id":"s_1","createdAt":0}}})";

// Where the reviewer drew: frame 10 of the shot, as the program shows it.
const char *const ANCHOR = R"({"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/shot","sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},"capturedAt":1787742000000})";

// One stroke: a PaintStart, its points and a PaintEnd sharing a uuid.
const char *const STROKE = R"([{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0.25,0.25,1]},{"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1","x":[-0.5,0,0.5],"y":[0.2,0,-0.2],"size":[0.01,0.01,0.01],"alpha":[1,1,1]}},{"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}])";

// The comment as Furnace relays it once it has stored it, to everyone in
// the session. A real host reads this from its own socket.
const char *const RELAY = R"({"v":2,"type":"review_commit","sessionId":"s_1","senderId":"p_2","seq":1,"ts":0,"order":2,"payload":{"clientMutationId":"m1","commentId":"c1","authorUserId":"u_bea","authorPresenceId":"p_2","serverOrder":1,"revision":1,"createdAt":0,"anchor":{"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/shot","sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},"capturedAt":1787742000000},"bodyMarkdown":"Too dark here.","annotationCommands":[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0.25,0.25,1]},{"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1","x":[-0.5,0,0.5],"y":[0.2,0,-0.2],"size":[0.01,0.01,0.01],"alpha":[1,1,1]}},{"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}]}})";

// Stands in for the network: a real program writes every frame the session
// has waiting to its WebSocket to Furnace.
void send_frames(std::uint64_t session) {
    while (const auto frame = sync::session_poll_transmit(session, 0)) {
        std::cout << "send " << frame->bytes.size() << " bytes\n";
    }
}

void receive(std::uint64_t session, const std::string &frame) {
    sync::session_receive(session, std::vector<std::uint8_t>(frame.begin(), frame.end()), false);
}

int main() {
    // ORI Annotations is an extension; enabling it enables sync too.
    ori::enable();

    // The reviewer joins with the right to comment.
    const std::uint64_t reviewer = sync::session_new(
        R"({"token":"join-code","identity":{"capabilities":["review.comment.create","review.annotation.create"]}})");
    sync::session_connect(reviewer);
    send_frames(reviewer);
    receive(reviewer, WELCOME);
    while (sync::session_poll_event(reviewer)) {
    }

    // Commit the stroke with a note. The session sends it again after every
    // reconnect until Furnace stores it, so it is not lost.
    const otio::SerializableObject seen = otio::from_json(CUT);
    std::cout << "committed " << ori::commit(reviewer, seen, ANCHOR, "Too dark here.", STROKE, "")
              << "\n";
    send_frames(reviewer);

    // The host hears the stored comment and folds it into its timeline.
    const std::uint64_t host =
        sync::session_new(R"({"token":"host-token","identity":{"capabilities":["review.commit.receive"]}})");
    sync::session_connect(host);
    send_frames(host);
    receive(host, HOST_WELCOME);
    receive(host, RELAY);
    const otio::SerializableObject timeline = otio::from_json(CUT);
    while (const auto event = sync::session_poll_event(host)) {
        // A stroke still being drawn: draw it over the picture, then drop it.
        if (const auto partial = ori::read_partial(*event)) {
            std::cout << "drawing " << *partial << "\n";
        } else if (event->find(R"("type":"review_relay")") != std::string::npos) {
            const ori::FoldResult folded = ori::fold(timeline, *event);
            std::cout << "folded " << ori::describe(folded.clip)
                      << " already there: " << (folded.already ? "yes" : "no") << "\n";
            std::cout << "the stroke belongs to " << ori::owner_of(folded.clip, "s1").value_or("nobody")
                      << "\n";
        }
    }
    sync::session_close(reviewer);
    sync::session_close(host);
}
