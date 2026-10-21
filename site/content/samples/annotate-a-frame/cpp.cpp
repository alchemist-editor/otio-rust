#include <iostream>
#include <sstream>
#include <string>

#include <opentimelineio/ext/ori.hpp>
#include <opentimelineio/otio.hpp>

namespace ori = otio::ext::ori;

// A cut with one shot, which sync knows by the GUID `shot`.
const char *const CUT = R"({
  "OTIO_SCHEMA": "Timeline.1",
  "name": "review",
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "Clip.2", "name": "shot",
       "metadata": {"sync": {"guid": "shot"}}}]}]}
})";

int main() {
    // ORI Annotations is an extension; enabling it enables sync too.
    ori::enable();
    const otio::SerializableObject timeline = otio::from_json(CUT);

    // The Annotations track, and the clip on it for frame 10 of the shot.
    const otio::SerializableObject track = ori::ensure_track(timeline);
    const otio::SerializableObject frame = ori::ensure_clip(track, "shot", 10, 24);

    // ORI's coordinates are height-normalised with the origin at the
    // centre, so convert from the pixels of the 1920x1080 picture.
    std::ostringstream xs, ys;
    xs.precision(17);
    ys.precision(17);
    const double pixels[3][2] = {{200, 200}, {960, 540}, {1720, 880}};
    for (int i = 0; i < 3; i++) {
        const auto point = ori::pixels_to_ori(1920, 1080, pixels[i][0], pixels[i][1]);
        xs << (i ? "," : "") << point.x;
        ys << (i ? "," : "") << point.y;
    }

    // A stroke is a PaintStart, its points and a PaintEnd sharing one uuid.
    // Fields left out take the defaults ORI's classes give them.
    const std::string uuid = ori::new_uuid();
    ori::add_commands(frame,
        R"([{"OTIO_SCHEMA":"PaintStart.1","uuid":")" + uuid +
        R"(","rgba":[1,0.25,0.25,1],"participant_hash":"reviewer-1"},)"
        R"({"OTIO_SCHEMA":"PaintPoint.1","uuid":")" + uuid +
        R"(","points":{"OTIO_SCHEMA":"PaintVertices.1","x":[)" + xs.str() + R"(],"y":[)" + ys.str() +
        R"(],"size":[0.01,0.01,0.01],"alpha":[1,1,1]}},)"
        R"({"OTIO_SCHEMA":"PaintEnd.1","uuid":")" + uuid + R"("}])");

    // Read them back: every annotation on the shot.
    for (const otio::SerializableObject &holder : ori::list_for_clip(timeline, "shot")) {
        std::cout << ori::describe(holder) << "\n";
    }

    // The file is plain OpenTimelineIO with ORI's schemas in it.
    std::cout << timeline.to_json(4) << "\n";
}
