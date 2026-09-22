// Tests for the generated C++ SDK.
//
// These are written by hand, not generated. A generator that also wrote its
// own tests would only prove it is self-consistent; what needs proving is
// that the C++ it writes does what someone reading it would expect, against
// the same library and the same fixtures the Rust tests read.
//
// There is no test framework here because the workspace takes no
// third-party dependencies, and the C interface's own test does the same.
// It prints what it ran and exits non-zero if anything failed.

#include <cmath>
#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <optional>
#include <string>
#include <vector>

#include <opentimelineio/otio.hpp>

namespace {

int failures = 0;

#define CHECK(condition)                                                      \
    do {                                                                      \
        if (!(condition)) {                                                   \
            std::cout << "  FAIL " << __FILE__ << ":" << __LINE__ << ": "     \
                      << #condition << "\n";                                  \
            failures += 1;                                                    \
        }                                                                     \
    } while (0)

#define CHECK_EQ(left, right)                                                 \
    do {                                                                      \
        const auto left_ = (left);                                            \
        const auto right_ = (right);                                          \
        if (!(left_ == right_)) {                                             \
            std::cout << "  FAIL " << __FILE__ << ":" << __LINE__ << ": "     \
                      << #left << " is " << left_ << ", wanted " << right_    \
                      << "\n";                                                \
            failures += 1;                                                    \
        }                                                                     \
    } while (0)

#define CHECK_NEAR(left, right)                                               \
    do {                                                                      \
        const double left_ = (left);                                          \
        const double right_ = (right);                                        \
        if (std::fabs(left_ - right_) > 1e-9) {                               \
            std::cout << "  FAIL " << __FILE__ << ":" << __LINE__ << ": "     \
                      << #left << " is " << left_ << ", wanted " << right_    \
                      << "\n";                                                \
            failures += 1;                                                    \
        }                                                                     \
    } while (0)

/// The repository. CMake says where it is, because `__FILE__` is whatever
/// path the compiler was handed and is relative when the build is driven from
/// the repository root. Walking up from this file is the fallback for a build
/// that compiles it by hand: `<root>/sdk/cpp/tests/tests.cpp`.
std::string repository() {
#ifdef OTIO_REPOSITORY
    return OTIO_REPOSITORY;
#else
    return std::filesystem::absolute(__FILE__)
        .parent_path()
        .parent_path()
        .parent_path()
        .parent_path()
        .string();
#endif
}

/// The EDL the Rust adapter's own tests read, so that the two agree about
/// what is in it.
std::string screening_edl() {
    return repository() + "/crates/otio-cmx3600/tests/data/screening_example.edl";
}

/// A path in a directory of this run's own, so two tests cannot collide.
std::string temporary(const std::string &name) {
    static int counter = 0;
    counter += 1;
    const std::filesystem::path directory =
        std::filesystem::temp_directory_path() / ("otio-cpp-" + std::to_string(counter));
    std::filesystem::create_directories(directory);
    return (directory / name).string();
}

/// The status a call failed with, for a test that wants to name it.
template <class Body>
std::optional<otio::Status> threw(Body body) {
    try {
        body();
    } catch (const otio::Error &error) {
        return error.status();
    }
    return std::nullopt;
}

void the_library_reports_a_version() { CHECK(!otio::version().empty()); }

void an_enum_says_what_the_c_interface_calls_it() {
    CHECK_EQ(std::string(otio::to_string(otio::Format::CMX_3600)), std::string("OTIO_FORMAT_CMX_3600"));
    CHECK_EQ(std::string(otio::to_string(otio::Status::NO_VALUE)), std::string("OTIO_STATUS_NO_VALUE"));
    CHECK_EQ(otio::format_name(otio::Format::CMX_3600), std::string("cmx_3600"));
}

void rates_are_classified() {
    // The drop-frame rate is 30000/1001, which is not the 29.97 people
    // write; asking for the nearest SMPTE rate is what turns one into the
    // other.
    CHECK(!otio::is_drop_frame_rate(29.97));
    CHECK(otio::is_drop_frame_rate(otio::nearest_smpte_timecode_rate(29.97)));
    CHECK(!otio::is_drop_frame_rate(24));
    CHECK(otio::is_smpte_timecode_rate(24));
}

void reading_an_edl_finds_its_clips() {
    otio::Document document = otio::Document::read_from_file(otio::Format::CMX_3600, screening_edl());

    const std::optional<otio::SerializableObject> root = document.root();
    CHECK(root.has_value());
    const std::vector<otio::SerializableObject> clips = root->find_clips();
    CHECK_EQ(clips.size(), std::size_t(9));

    // Every one of them really is a clip, and the library says so.
    std::size_t counted = 0;
    for (const otio::SerializableObject &node : clips) {
        CHECK(node.is<otio::Clip>());
        if (node.as<otio::Clip>().has_value()) {
            counted += 1;
        }
    }
    CHECK_EQ(counted, std::size_t(9));
}

/// The quickstart in `sdk/cpp/README.md` is generated, so nothing compiles
/// it. This is that example, so that it cannot go stale.
void the_quickstart_from_the_readme_runs() {
    otio::Document document = otio::Document::open(screening_edl());

    std::size_t named = 0;
    if (std::optional<otio::SerializableObject> root = document.root()) {
        for (const otio::SerializableObject &node : root->find_clips()) {
            if (std::optional<otio::Clip> clip = node.as<otio::Clip>()) {
                CHECK(!clip->name().empty());
                CHECK(clip->duration().to_seconds() > 0);
                named += 1;
            }
        }
    }
    CHECK_EQ(named, std::size_t(9));
}

void open_works_out_the_format_from_the_name() {
    otio::Document document = otio::Document::open(screening_edl());
    const std::optional<otio::SerializableObject> root = document.root();
    CHECK(root.has_value());
    CHECK(root->name().find("Example_Screening") != std::string::npos);
}

void open_declines_a_suffix_no_format_claims() {
    const std::optional<otio::Status> status =
        threw([] { otio::Document::open("/tmp/nothing.wav"); });
    CHECK(status == otio::Status::NO_VALUE);
}

void a_document_survives_a_round_trip_through_json() {
    otio::Document document = otio::Document::open(screening_edl());
    const std::string text = document.to_json(2);
    CHECK(text.find("Timeline") != std::string::npos);

    otio::Document again = otio::Document::from_json(text);
    const std::optional<otio::SerializableObject> root = again.root();
    CHECK(root.has_value());
    CHECK_EQ(root->find_clips().size(), std::size_t(9));
}

void saving_and_opening_again_keeps_the_clips() {
    otio::Document document = otio::Document::open(screening_edl());
    const std::string path = temporary("round-trip.otio");
    document.save(path);

    otio::Document again = otio::Document::open(path);
    const std::optional<otio::SerializableObject> root = again.root();
    CHECK(root.has_value());
    CHECK_EQ(root->find_clips().size(), std::size_t(9));
}

void writing_bytes_in_every_format_the_library_knows() {
    otio::Document document = otio::Document::open(screening_edl());
    for (const otio::Format format : {otio::Format::OTIO_JSON, otio::Format::CMX_3600}) {
        CHECK(!document.write_to_bytes(format).empty());
    }
}

/// Builds a timeline with one video track holding two clips.
struct Built {
    otio::Timeline timeline;
    otio::Track track;
    std::vector<otio::Clip> clips;
};

Built make_timeline(otio::Document &document) {
    otio::Timeline timeline = document.new_timeline("Assembly");
    otio::Stack stack = document.new_stack("tracks");
    timeline.set_tracks(stack);
    otio::Track track = document.new_track("V1", "Video");
    stack.append_child(track);

    std::vector<otio::Clip> clips;
    const std::vector<std::string> names = {"A", "B"};
    for (std::size_t index = 0; index < names.size(); ++index) {
        otio::Clip clip = document.new_clip(names[index]);
        const otio::RationalTime start(static_cast<double>(index) * 24, 24);
        clip.set_source_range(otio::TimeRange(start, otio::RationalTime(24, 24)));
        track.append_child(clip);
        clips.push_back(clip);
    }
    document.set_root(timeline);
    return Built{timeline, track, clips};
}

void building_a_timeline_from_nothing() {
    otio::Document document = otio::Document::create();
    Built built = make_timeline(document);

    CHECK_EQ(built.track.child_count(), std::size_t(2));
    CHECK_EQ(built.timeline.find_clips().size(), std::size_t(2));
    CHECK_EQ(built.clips[0].name(), std::string("A"));
    CHECK_EQ(built.track.kind(), std::string("Video"));

    // The whole track is as long as the two clips together.
    CHECK_NEAR(built.track.duration().to_seconds(), 2);
}

void a_freshly_built_object_is_enabled() {
    otio::Document document = otio::Document::create();
    otio::Clip clip = document.new_clip("A");
    CHECK(clip.enabled());
    clip.set_enabled(false);
    CHECK(!clip.enabled());
}

void no_value_is_an_answer_and_not_a_failure() {
    otio::Document document = otio::Document::create();
    otio::Clip clip = document.new_clip("untrimmed");

    // An item that uses all of its media has no source range, and that is
    // an answer rather than a failure.
    CHECK(!clip.source_range().has_value());

    const otio::TimeRange span(otio::RationalTime(0, 24), otio::RationalTime(12, 24));
    clip.set_source_range(span);
    CHECK(clip.source_range() == span);

    clip.clear_source_range();
    CHECK(!clip.source_range().has_value());
}

void an_object_knows_which_schemas_it_is() {
    otio::Document document = otio::Document::create();
    otio::Clip clip = document.new_clip("A");

    CHECK(clip.is_a(otio::NodeKind::CLIP));
    CHECK(clip.is_a(otio::NodeKind::ITEM));
    CHECK(clip.is_a(otio::NodeKind::COMPOSABLE));
    CHECK(clip.is_a(otio::NodeKind::SERIALIZABLE_OBJECT));
    CHECK(!clip.is_a(otio::NodeKind::TRACK));
    CHECK(clip.is<otio::Item>());
    CHECK(!clip.is<otio::Track>());
    CHECK(clip.schema_kind() == otio::NodeKind::CLIP);
    CHECK_EQ(clip.schema_name(), std::string("Clip"));
}

void clearing_children_hands_them_all_back() {
    otio::Document document = otio::Document::create();
    Built built = make_timeline(document);

    const std::vector<otio::SerializableObject> taken = built.track.clear_children();
    CHECK_EQ(taken.size(), built.clips.size());
    CHECK_EQ(built.track.child_count(), std::size_t(0));
    CHECK_EQ(taken[0].name(), std::string("A"));
    CHECK_EQ(taken[1].name(), std::string("B"));
}

void every_child_and_its_range_come_back_together() {
    otio::Document document = otio::Document::create();
    Built built = make_timeline(document);

    const otio::Composition::RangesOfChildrenResult answer = built.track.ranges_of_children();
    CHECK_EQ(answer.nodes.size(), std::size_t(2));
    CHECK_EQ(answer.ranges.size(), std::size_t(2));
    if (answer.ranges.size() == 2) {
        CHECK_NEAR(answer.ranges[0].start_time.to_seconds(), 0);
        CHECK_NEAR(answer.ranges[1].start_time.to_seconds(), 1);
    }
}

void a_stale_handle_is_refused() {
    otio::Document document = otio::Document::create();
    otio::Clip clip = document.new_clip("A");
    document.remove_node(clip);

    const std::optional<otio::Status> status = threw([&] { clip.name(); });
    CHECK(status == otio::Status::STALE_HANDLE);
}

void an_object_of_no_document_fails_rather_than_crashing() {
    const otio::SerializableObject orphan = otio::SerializableObject::none();
    CHECK(orphan.is_none());
    CHECK(orphan.document() == nullptr);
    CHECK(threw([&] { orphan.name(); }).has_value());
}

void an_object_from_another_document_is_refused() {
    otio::Document one = otio::Document::create();
    otio::Document other = otio::Document::create();

    otio::Track track = one.new_track("V1", "Video");
    otio::Clip stranger = other.new_clip("elsewhere");

    const std::optional<otio::Status> status = threw([&] { track.append_child(stranger); });
    CHECK(status == otio::Status::INVALID_ARGUMENT);

    // A call that cannot fail answers rather than throwing, and the answer
    // is no.
    CHECK(!one.contains(stranger));
}

void metadata_goes_in_and_comes_back() {
    otio::Document document = otio::Document::create();
    otio::Clip clip = document.new_clip("A");

    clip.metadata().set_string("reel", "ZZ100");
    clip.metadata().set_int("take", 3);
    clip.metadata().set_bool("circled", true);
    clip.metadata().set_double("gain", 0.5);

    CHECK_EQ(clip.metadata().get_string("reel"), std::string("ZZ100"));
    CHECK_EQ(clip.metadata().get_int("take"), std::int64_t(3));
    CHECK(clip.metadata().get_bool("circled"));
    CHECK_NEAR(clip.metadata().get_double("gain"), 0.5);
    CHECK(clip.metadata().contains("reel"));
    CHECK(!clip.metadata().contains("nothing"));

    // A path is followed, not created: the dictionary has to exist before
    // anything can be written inside it.
    clip.metadata().set_dictionary("cmx_3600");
    clip.metadata().set_string("cmx_3600.reel", "AX");
    CHECK_EQ(clip.metadata().get_string("cmx_3600.reel"), std::string("AX"));

    clip.metadata().clear();
    CHECK(!clip.metadata().contains("reel"));
}

void time_values_compute_without_a_document() {
    const otio::RationalTime time(48, 24);
    CHECK_NEAR(time.to_seconds(), 2);
    CHECK_EQ(time.to_frames(), 48);
    CHECK(time.rescaled_to(48) == otio::RationalTime(96, 48));
    CHECK(otio::RationalTime::duration_from_start_end_time(otio::RationalTime(0, 24), time) == time);
    CHECK(time.is_valid());
    CHECK_EQ(time.to_timecode(), std::string("00:00:02:00"));
    CHECK(otio::RationalTime::from_timecode("00:00:02:00", 24) == time);
}

void an_unreadable_timecode_is_a_failure() {
    const std::optional<otio::Status> status =
        threw([] { otio::RationalTime::from_timecode("not a timecode", 24); });
    CHECK(status == otio::Status::TIME_ERROR);
}

void a_range_answers_about_what_it_covers() {
    const otio::TimeRange span(otio::RationalTime(0, 24), otio::RationalTime(24, 24));
    CHECK(span.end_time_exclusive() == otio::RationalTime(24, 24));
    CHECK(span.contains_time(otio::RationalTime(12, 24)));
    CHECK(!span.contains_time(otio::RationalTime(24, 24)));
}

void an_object_built_on_its_own_can_join_a_timeline() {
    otio::Document document = otio::Document::create();
    otio::Track track = document.new_track("V1", "Video");

    // A clip built in a document of its own, as a binding that hides the
    // document would build one.
    otio::Document workshop = otio::Document::create();
    otio::Clip clip = workshop.new_clip("guest");

    const std::vector<std::pair<otio::SerializableObject, otio::SerializableObject>> translated =
        document.absorb(workshop);

    std::optional<otio::SerializableObject> arrived;
    for (const auto &pair : translated) {
        if (pair.first == clip) {
            arrived = pair.second;
        }
    }
    CHECK(arrived.has_value());
    if (!arrived.has_value()) {
        return;
    }
    CHECK(arrived->is<otio::Clip>());
    CHECK(arrived->document() == document.pointer());

    track.append_child(*arrived);
    CHECK_EQ(track.child_count(), std::size_t(1));
    CHECK_EQ(arrived->name(), std::string("guest"));
}

struct Test {
    const char *name;
    void (*body)();
};

const Test tests[] = {
    {"the library reports a version", the_library_reports_a_version},
    {"an enum says what the C interface calls it", an_enum_says_what_the_c_interface_calls_it},
    {"rates are classified", rates_are_classified},
    {"reading an EDL finds its clips", reading_an_edl_finds_its_clips},
    {"the quickstart from the README runs", the_quickstart_from_the_readme_runs},
    {"open works out the format from the name", open_works_out_the_format_from_the_name},
    {"open declines a suffix no format claims", open_declines_a_suffix_no_format_claims},
    {"a document survives a round trip through JSON", a_document_survives_a_round_trip_through_json},
    {"saving and opening again keeps the clips", saving_and_opening_again_keeps_the_clips},
    {"writing bytes in every format the library knows", writing_bytes_in_every_format_the_library_knows},
    {"building a timeline from nothing", building_a_timeline_from_nothing},
    {"a freshly built object is enabled", a_freshly_built_object_is_enabled},
    {"no value is an answer and not a failure", no_value_is_an_answer_and_not_a_failure},
    {"an object knows which schemas it is", an_object_knows_which_schemas_it_is},
    {"clearing children hands them all back", clearing_children_hands_them_all_back},
    {"every child and its range come back together", every_child_and_its_range_come_back_together},
    {"a stale handle is refused", a_stale_handle_is_refused},
    {"an object of no document fails rather than crashing", an_object_of_no_document_fails_rather_than_crashing},
    {"an object from another document is refused", an_object_from_another_document_is_refused},
    {"metadata goes in and comes back", metadata_goes_in_and_comes_back},
    {"time values compute without a document", time_values_compute_without_a_document},
    {"an unreadable timecode is a failure", an_unreadable_timecode_is_a_failure},
    {"a range answers about what it covers", a_range_answers_about_what_it_covers},
    {"an object built on its own can join a timeline", an_object_built_on_its_own_can_join_a_timeline},
};

}  // namespace

int main() {
    for (const Test &test : tests) {
        std::cout << test.name << "\n";
        const int before = failures;
        try {
            test.body();
        } catch (const otio::Error &error) {
            std::cout << "  FAIL threw " << otio::to_string(error.status()) << ": "
                      << error.what() << "\n";
            failures += 1;
        }
        if (failures == before) {
            std::cout << "  ok\n";
        }
    }
    if (failures != 0) {
        std::cout << failures << " failed\n";
        return EXIT_FAILURE;
    }
    std::cout << "all " << (sizeof(tests) / sizeof(tests[0])) << " passed\n";
    return EXIT_SUCCESS;
}
