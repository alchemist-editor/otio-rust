#include <cstdint>
#include <iostream>
#include <string>
#include <vector>

#include <opentimelineio/otio.hpp>

// A clip with a blur and a dissolve into the next one, as an editor that
// knows nothing of the registry writes them: named, and nothing more.
static const std::string cut = R"({
  "OTIO_SCHEMA": "Track.1",
  "name": "V1",
  "kind": "Video",
  "children": [
    {"OTIO_SCHEMA": "Clip.2", "name": "A", "effects": [
      {"OTIO_SCHEMA": "Effect.1", "name": "blur", "effect_name": "Gaussian Blur"}
    ]},
    {"OTIO_SCHEMA": "Transition.2", "name": "dissolve",
     "transition_type": "SMPTE_Dissolve",
     "effect": {"OTIO_SCHEMA": "Effect.2", "effect_name": "Cross Dissolve"}},
    {"OTIO_SCHEMA": "Clip.2", "name": "B"}
  ]
})";

// Prints what an effect was called and what the linker made of it.
static void show(const std::string &holder, const otio::SerializableObject &node) {
    const otio::Effect effect = *node.as<otio::Effect>();
    std::cout << holder << ": " << effect.effect_name() << " -> " << effect.effect_id() << "\n";
}

int main() {
    // otio_fx_registry is built in, but runs only when a read names it.
    otio::ReadOptions options = otio::read_options_default();
    options.effects_linker = "otio_fx_registry";
    const otio::SerializableObject track = otio::read_from_bytes(
        otio::Format::OTIO_JSON, std::vector<std::uint8_t>(cut.begin(), cut.end()), options);

    const otio::Clip clip = *track.child_at(0).as<otio::Clip>();
    show("A", clip.effect_at(0));

    // A transition's effect is optional.
    const otio::Transition transition = *track.child_at(1).as<otio::Transition>();
    if (const auto effect = transition.effect()) {
        show("dissolve", *effect);
    }
    return 0;
}
