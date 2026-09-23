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
#include <fstream>
#include <iterator>
#include <iostream>
#include <mutex>
#include <optional>
#include <string>
#include <thread>
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

/// An AAF the Rust adapter's tests read, whose five clips each carry the
/// MobID of the media they were cut from.
std::string colored_clips_aaf() {
    return repository() + "/crates/otio-aaf/tests/data/colored_clips.aaf";
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

void an_aaf_reads_and_writes_back_out() {
    const otio::SerializableObject root =
        otio::read_from_file(otio::Format::AAF, colored_clips_aaf(), std::nullopt);
    CHECK_EQ(root.find_clips().size(), std::size_t(5));

    // A cut read from an AAF keeps each clip's MobID, so it writes back out
    // with no leave to make any up.
    const std::vector<std::uint8_t> written =
        otio::write_to_bytes(otio::Format::AAF, root, std::nullopt);
    const otio::SerializableObject again =
        otio::read_from_bytes(otio::Format::AAF, written, std::nullopt);
    CHECK_EQ(again.find_clips().size(), std::size_t(5));

    // A fixed time and seed write the same file twice.
    otio::WriteOptions fixed{};
    fixed.aaf_time = 1714979289;
    fixed.aaf_id_seed = 59;
    CHECK(otio::write_to_bytes(otio::Format::AAF, root, fixed) ==
          otio::write_to_bytes(otio::Format::AAF, root, fixed));

    otio::ReadOptions nested{};
    nested.aaf_keep_nesting = true;
    CHECK_EQ(otio::read_from_file(otio::Format::AAF, colored_clips_aaf(), nested)
                 .find_clips()
                 .size(),
             std::size_t(5));
    CHECK_EQ(otio::format_name(otio::Format::AAF), std::string("AAF"));
}

/// The active media reference of each clip under `root`, in order.
std::vector<otio::SerializableObject> active_media(const otio::SerializableObject &root) {
    std::vector<otio::SerializableObject> references;
    for (const otio::SerializableObject &clip : root.find_clips()) {
        references.push_back(*clip.as<otio::Clip>()->media_reference());
    }
    return references;
}

/// What a file holds, or nothing if it cannot be read.
std::string contents_of(const std::filesystem::path &path) {
    std::ifstream file(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>());
}

void a_bundle_carries_its_media_with_it() {
    // A cut of two clips: one whose media is a file, named relative to the
    // directory it is in, and one whose media is on the web. A bundle is
    // never written over, so the directory starts empty on every run.
    const std::filesystem::path directory =
        std::filesystem::temp_directory_path() / "otio-cpp-bundles";
    std::filesystem::remove_all(directory);
    std::filesystem::create_directories(directory);
    std::ofstream(directory / "shot.mov", std::ios::binary) << "not really a movie";

    otio::Timeline timeline = otio::Timeline::create("bundled");
    otio::Track track = otio::Track::create("V1", "Video");
    timeline.tracks()->as<otio::Stack>()->append_child(track);
    const std::vector<std::pair<std::string, std::string>> sources = {
        {"local", "shot.mov"}, {"remote", "https://example.com/remote.mov"}};
    for (const auto &source : sources) {
        otio::Clip clip = otio::Clip::create(source.first);
        clip.set_media_reference("DEFAULT_MEDIA",
                                 otio::ExternalReference::create(source.first + " media",
                                                                 source.second));
        clip.set_active_media_reference_key("DEFAULT_MEDIA");
        track.append_child(clip);
    }

    // Upstream's default refuses media that is not a file.
    otio::WriteOptions options{};
    options.bundle_media_base_dir = directory.string();
    CHECK(threw([&] {
              otio::write_to_file(otio::Format::OTIOZ, timeline,
                                  (directory / "refused.otioz").string(), options);
          }) == otio::Status::IO_ERROR);

    options.bundle_media_policy = otio::BundleMediaPolicy::MISSING_IF_NOT_FILE;
    for (const otio::Format format : {otio::Format::OTIOZ, otio::Format::OTIOD}) {
        const std::filesystem::path path =
            directory / (std::string("cut.") + otio::format_name(format));
        otio::write_to_file(format, timeline, path.string(), options);

        // Read as it is, the file's reference points into the bundle and
        // the web one is missing.
        const otio::SerializableObject plain = otio::open(path.string());
        const std::vector<otio::SerializableObject> references = active_media(plain);
        CHECK_EQ(references[0].as<otio::ExternalReference>()->target_url(),
                 std::string("media/shot.mov"));
        CHECK(references[1].is<otio::MissingReference>());

        // With absolute paths it points at a real copy of the media, which
        // an .otioz has to be unpacked to have.
        otio::ReadOptions read{};
        read.bundle_absolute_media_paths = true;
        std::filesystem::path unpacked = path;
        if (format == otio::Format::OTIOZ) {
            unpacked = directory / "unpacked";
            read.bundle_extract_path = unpacked.string();
        }
        const otio::SerializableObject absolute =
            otio::read_from_file(format, path.string(), read);
        const std::string url =
            active_media(absolute)[0].as<otio::ExternalReference>()->target_url();
        CHECK_EQ(url, (unpacked / "media" / "shot.mov").string());
        CHECK_EQ(contents_of(url), std::string("not really a movie"));

        // A bundle is never written over.
        CHECK(threw([&] { otio::write_to_file(format, timeline, path.string(), options); }) ==
              otio::Status::IO_ERROR);
    }

    // Leaving every reference missing bundles no media at all.
    const std::string empty = (directory / "no-media.otioz").string();
    options.bundle_media_policy = otio::BundleMediaPolicy::ALL_MISSING;
    otio::write_to_file(otio::Format::OTIOZ, timeline, empty, options);
    for (const otio::SerializableObject &reference : active_media(otio::open(empty))) {
        CHECK(reference.is<otio::MissingReference>());
    }

    // A bundle lives on disk, so it is not written as bytes.
    CHECK(threw([&] { otio::write_to_bytes(otio::Format::OTIOZ, timeline, std::nullopt); }) ==
          otio::Status::UNSUPPORTED);
    CHECK_EQ(otio::format_name(otio::Format::OTIOD), std::string("otiod"));
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
    const otio::SerializableObject root =
        otio::read_from_file(otio::Format::CMX_3600, screening_edl());

    const std::vector<otio::SerializableObject> clips = root.find_clips();
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
    const otio::SerializableObject root = otio::open(screening_edl());

    std::size_t named = 0;
    for (const otio::SerializableObject &node : root.find_clips()) {
        if (std::optional<otio::Clip> clip = node.as<otio::Clip>()) {
            CHECK(!clip->name().empty());
            CHECK(clip->duration().to_seconds() > 0);
            named += 1;
        }
    }
    CHECK_EQ(named, std::size_t(9));
}

void open_works_out_the_format_from_the_name() {
    const otio::SerializableObject root = otio::open(screening_edl());
    CHECK(root.name().find("Example_Screening") != std::string::npos);
}

void open_declines_a_suffix_no_format_claims() {
    const std::optional<otio::Status> status = threw([] { otio::open("/tmp/nothing.wav"); });
    CHECK(status == otio::Status::NO_VALUE);
}

void a_timeline_survives_a_round_trip_through_json() {
    const otio::SerializableObject root = otio::open(screening_edl());
    const std::string text = root.to_json(2);
    CHECK(text.find("Timeline") != std::string::npos);

    const otio::SerializableObject again = otio::from_json(text);
    CHECK_EQ(again.find_clips().size(), std::size_t(9));
}

void saving_and_opening_again_keeps_the_clips() {
    const otio::SerializableObject root = otio::open(screening_edl());
    const std::string path = temporary("round-trip.otio");
    otio::save(root, path);

    const otio::SerializableObject again = otio::open(path);
    CHECK_EQ(again.find_clips().size(), std::size_t(9));
}

void writing_bytes_in_every_format_the_library_knows() {
    const otio::SerializableObject root = otio::open(screening_edl());
    for (const otio::Format format : {otio::Format::OTIO_JSON, otio::Format::CMX_3600}) {
        CHECK(!otio::write_to_bytes(format, root).empty());
    }
}

/// Builds a timeline with one video track holding two clips.
struct Built {
    otio::Timeline timeline;
    otio::Track track;
    std::vector<otio::Clip> clips;
};

/// Every one of these is built on its own, in an arena of its own, and
/// joins the timeline only when it is appended: five arenas become one, and
/// the handles held here keep working across every move.
Built make_timeline() {
    otio::Timeline timeline = otio::Timeline::create("Assembly");
    otio::Stack stack = otio::Stack::create("tracks");
    timeline.set_tracks(stack);
    otio::Track track = otio::Track::create("V1", "Video");
    stack.append_child(track);

    std::vector<otio::Clip> clips;
    const std::vector<std::string> names = {"A", "B"};
    for (std::size_t index = 0; index < names.size(); ++index) {
        otio::Clip clip = otio::Clip::create(names[index]);
        const otio::RationalTime start(static_cast<double>(index) * 24, 24);
        clip.set_source_range(otio::TimeRange(start, otio::RationalTime(24, 24)));
        track.append_child(clip);
        clips.push_back(clip);
    }
    return Built{timeline, track, clips};
}

void building_a_timeline_from_nothing() {
    Built built = make_timeline();

    CHECK_EQ(built.track.child_count(), std::size_t(2));
    CHECK_EQ(built.timeline.find_clips().size(), std::size_t(2));
    CHECK_EQ(built.clips[0].name(), std::string("A"));
    CHECK_EQ(built.track.kind(), std::string("Video"));

    // The whole track is as long as the two clips together.
    CHECK_NEAR(built.track.duration().to_seconds(), 2);
}

void a_freshly_built_object_is_enabled() {
    otio::Clip clip = otio::Clip::create("A");
    CHECK(clip.enabled());
    clip.set_enabled(false);
    CHECK(!clip.enabled());
}

void no_value_is_an_answer_and_not_a_failure() {
    otio::Clip clip = otio::Clip::create("untrimmed");

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
    otio::Clip clip = otio::Clip::create("A");

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
    Built built = make_timeline();

    const std::vector<otio::SerializableObject> taken = built.track.clear_children();
    CHECK_EQ(taken.size(), built.clips.size());
    CHECK_EQ(built.track.child_count(), std::size_t(0));
    CHECK_EQ(taken[0].name(), std::string("A"));
    CHECK_EQ(taken[1].name(), std::string("B"));
}

void every_child_and_its_range_come_back_together() {
    Built built = make_timeline();

    const otio::Composition::RangesOfChildrenResult answer = built.track.ranges_of_children();
    CHECK_EQ(answer.nodes.size(), std::size_t(2));
    CHECK_EQ(answer.ranges.size(), std::size_t(2));
    if (answer.ranges.size() == 2) {
        CHECK_NEAR(answer.ranges[0].start_time.to_seconds(), 0);
        CHECK_NEAR(answer.ranges[1].start_time.to_seconds(), 1);
    }
}

void a_stale_handle_is_refused() {
    otio::Clip clip = otio::Clip::create("A");
    clip.remove_from_timeline();

    const std::optional<otio::Status> status = threw([&] { clip.name(); });
    CHECK(status == otio::Status::STALE_HANDLE);
}

void an_object_of_no_timeline_fails_rather_than_crashing() {
    const otio::SerializableObject orphan = otio::SerializableObject::none();
    CHECK(orphan.is_none());
    CHECK(orphan.arena() == nullptr);
    CHECK(threw([&] { orphan.name(); }).has_value());
}

/// A handle is an index into one arena, and two timelines issue the same
/// indices, so an object from one would resolve to an unrelated object in
/// the other rather than failing. A call that only names an object therefore
/// has to refuse one from elsewhere — and refuse it before asking the
/// library, because absorbing first and failing afterwards would already
/// have merged the two timelines.
void an_object_from_another_timeline_is_refused() {
    otio::Track track = otio::Track::create("V1", "Video");
    otio::Clip mine = otio::Clip::create("mine");
    track.append_child(mine);

    otio::Track elsewhere = otio::Track::create("V2", "Video");
    otio::Clip stranger = otio::Clip::create("elsewhere");
    elsewhere.append_child(stranger);

    CHECK(threw([&] { track.detach_child(stranger); }) == otio::Status::INVALID_ARGUMENT);
    CHECK(
        threw([&] { (void)track.index_of_child(stranger); }) == otio::Status::INVALID_ARGUMENT);
    CHECK(
        threw([&] { (void)otio::flatten_tracks({track, elsewhere}); })
        == otio::Status::INVALID_ARGUMENT);

    // Even the question that looks harmless is refused. "Is this mine" has
    // an obvious answer for an object from elsewhere, but answering it would
    // mean resolving a handle of another arena against this one, where it
    // names an unrelated object. The refusal is the answer.
    CHECK(threw([&] { (void)track.has_child(stranger); }) == otio::Status::INVALID_ARGUMENT);

    // What the refusal is protecting, and the only assertion that tells a
    // refusal apart from an absorb that failed afterwards: the two timelines
    // are still independent, so releasing this one leaves the other whole.
    track.close();
    CHECK_EQ(elsewhere.child_count(), std::size_t(1));
    CHECK_EQ(stranger.name(), std::string("elsewhere"));
}

/// The refusal above has a type of its own, so a caller can tell it from the
/// library answering `INVALID_ARGUMENT` by catching it rather than by reading
/// its message. It is still an `otio::Error` with that status, so code that
/// caught it before still does.
void the_refusal_of_another_timeline_s_object_has_a_type_of_its_own() {
    otio::Track track = otio::Track::create("V1", "Video");
    otio::Track elsewhere = otio::Track::create("V2", "Video");
    otio::Clip stranger = otio::Clip::create("elsewhere");
    elsewhere.append_child(stranger);

    const auto foreign = [](auto body) {
        try {
            body();
        } catch (const otio::OtherTimelineError &error) {
            return error.status() == otio::Status::INVALID_ARGUMENT;
        } catch (const otio::Error &) {
            return false;
        }
        return false;
    };
    CHECK(foreign([&] { track.detach_child(stranger); }));
    CHECK(foreign([&] { (void)otio::flatten_tracks({track, elsewhere}); }));

    // A list with nothing in it is refused with the same status, but it is
    // not about another timeline, so it is not this refusal.
    CHECK(threw([&] { (void)otio::flatten_tracks({}); }) == otio::Status::INVALID_ARGUMENT);
    CHECK(!foreign([&] { (void)otio::flatten_tracks({}); }));
}

void metadata_goes_in_and_comes_back() {
    otio::Clip clip = otio::Clip::create("A");

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

/// The library hands each call's message back beside its status, rather
/// than leaving it somewhere a second call reads, so nothing here keeps a
/// thread's failures apart by hand. Many threads failing in two different
/// ways at once, and yielding between the throw and the check, must each
/// still carry the sentence their own call wrote: a bad timecode says
/// something of its own about time, and a clip asked for a track's kind
/// says it is not a track.
void every_failure_carries_its_own_message_whatever_thread_it_ran_on() {
    // Built before the threads start, because building edits a timeline and
    // reading one from many threads at once is what the library allows.
    const otio::Clip clip = otio::Clip::create("A");
    const otio::Track track(otio::detail::Adopt{}, clip.arena(), clip.handle());

    std::mutex guard;
    std::vector<std::string> wrong;
    const auto report = [&](const std::string &what) {
        const std::lock_guard<std::mutex> held(guard);
        wrong.push_back(what);
    };
    // An `Error` with no message says the status's own name instead, so a
    // timecode failure that lost its sentence would read as just that.
    const std::string bare = otio::to_string(otio::Status::TIME_ERROR);

    std::vector<std::thread> threads;
    for (int index = 0; index < 200; ++index) {
        threads.emplace_back([&] {
            try {
                otio::RationalTime::from_timecode("nonsense", 24);
                report("timecode: no failure");
            } catch (const otio::Error &error) {
                std::this_thread::yield();
                const std::string message = error.what();
                if (error.status() != otio::Status::TIME_ERROR || message.empty()
                    || message == bare || message.find("not a track") != std::string::npos) {
                    report("timecode: " + message);
                }
            }
        });
        threads.emplace_back([&] {
            try {
                track.kind();
                report("track kind: no failure");
            } catch (const otio::Error &error) {
                std::this_thread::yield();
                const std::string message = error.what();
                if (error.status() != otio::Status::CORE_ERROR
                    || message.find("not a track") == std::string::npos) {
                    report("track kind: " + message);
                }
            }
        });
    }
    for (std::thread &thread : threads) {
        thread.join();
    }
    for (const std::string &what : wrong) {
        std::cout << "  " << what << "\n";
    }
    CHECK(wrong.empty());
}

void a_range_answers_about_what_it_covers() {
    const otio::TimeRange span(otio::RationalTime(0, 24), otio::RationalTime(24, 24));
    CHECK(span.end_time_exclusive() == otio::RationalTime(24, 24));
    CHECK(span.contains_time(otio::RationalTime(12, 24)));
    CHECK(!span.contains_time(otio::RationalTime(24, 24)));
}

/// The whole point of hiding the arena: an object is built on its own and
/// put into a timeline afterwards, the way upstream's own bindings read.
void an_object_built_on_its_own_can_join_a_timeline() {
    otio::Track track = otio::Track::create("V1", "Video");
    otio::Clip clip = otio::Clip::create("guest");
    clip.set_source_range(otio::TimeRange(otio::RationalTime(0, 24), otio::RationalTime(48, 24)));

    track.append_child(clip);

    // The object the caller has held all along still names the clip, which
    // is what moving it had to preserve: its handle was reissued on the way
    // over and the object follows the chain to find it.
    CHECK_EQ(track.child_count(), std::size_t(1));
    CHECK_EQ(clip.name(), std::string("guest"));
    CHECK(track.child_at(0) == clip);
    CHECK_NEAR(track.duration().to_seconds(), 2);

    // And now that it is in, naming it is no longer naming a stranger.
    CHECK_EQ(track.index_of_child(clip), std::size_t(0));
    track.detach_child(clip);
}

/// The edit operations are the other half: they are handed an item that has
/// never been anywhere and a composition that is already somewhere, and the
/// call has to be made where the composition is.
void an_edit_puts_a_newly_built_item_into_a_track() {
    const auto span = [](double start, double length) {
        return otio::TimeRange(otio::RationalTime(start, 24), otio::RationalTime(length, 24));
    };
    const auto shot = [&](const std::string &name) {
        otio::Clip clip = otio::Clip::create(name);
        clip.set_source_range(span(0, 24));
        return clip;
    };

    otio::Track track = otio::Track::create("V1", "Video");
    track.append_child(shot("shot_01"));

    otio::insert(shot("shot_02"), track, otio::RationalTime(24, 24), false);
    otio::overwrite(shot("shot_03"), track, span(0, 24), false);

    CHECK_EQ(track.child_count(), std::size_t(2));
    CHECK_EQ(track.child_at(0).name(), std::string("shot_03"));
    CHECK_EQ(track.child_at(1).name(), std::string("shot_02"));
}

/// An object holds the arena it lives in, so the timeline lasts as long as
/// anything naming it; `close()` ends it sooner. Either way an object that
/// outlives it names nothing rather than dangling. Before the arena was
/// shared this way it was a use-after-free: this test crashed under the
/// address sanitizer rather than failing.
void an_object_outliving_its_timeline_fails_rather_than_crashing() {
    otio::SerializableObject survivor;
    otio::SerializableObject sibling;
    {
        const otio::Clip clip = otio::Clip::create("A");
        survivor = clip;
        sibling = otio::Clip::create("B");
        CHECK(survivor.arena() != nullptr);
        survivor.close();
    }
    CHECK(threw([&] { (void)survivor.name(); }) == otio::Status::NULL_POINTER);
    CHECK(threw([&] { survivor.set_name("B"); }) == otio::Status::NULL_POINTER);
    CHECK(threw([&] { (void)survivor.find_clips(); }) == otio::Status::NULL_POINTER);
    // Asking what schema it is answers "none" rather than reading anything.
    CHECK(!survivor.is<otio::Clip>());
    CHECK(!survivor.is_a(otio::NodeKind::CLIP));
    CHECK(!survivor.as<otio::Clip>().has_value());
    // Two objects of the same closed timeline still compare as themselves.
    CHECK(survivor == survivor);
    CHECK(!(survivor == sibling));
    // The other one was never in it, so it is untouched.
    CHECK_EQ(sibling.name(), std::string("B"));
}

/// The same, for the arena an `absorb` consumed: the C interface frees it
/// itself, so nothing here may free it again, and an object still naming it
/// has to be followed to where its object went rather than left dangling.
void an_object_of_an_absorbed_timeline_follows_it() {
    otio::Timeline timeline = otio::Timeline::create("cut");
    otio::Stack tracks = timeline.tracks().value().as<otio::Stack>().value();
    otio::Track track = otio::Track::create("V1", "Video");
    otio::Clip clip = otio::Clip::create("guest");

    // Three arenas, joined in an order that leaves a chain: the clip's went
    // into the track's, and the track's into the timeline's.
    track.append_child(clip);
    tracks.append_child(track);

    CHECK_EQ(clip.name(), std::string("guest"));
    CHECK_EQ(timeline.find_clips().size(), std::size_t(1));
    CHECK(timeline.find_clips().front() == clip);
    CHECK_EQ(timeline.name(), std::string("cut"));

    // Releasing the timeline releases everything that joined it, and each of
    // them says so rather than reading freed memory.
    timeline.close();
    CHECK(threw([&] { (void)clip.name(); }) == otio::Status::NULL_POINTER);
    CHECK(threw([&] { (void)track.name(); }) == otio::Status::NULL_POINTER);
}

struct Test {
    const char *name;
    void (*body)();
};

const Test tests[] = {
    {"the library reports a version", the_library_reports_a_version},
    {"an enum says what the C interface calls it", an_enum_says_what_the_c_interface_calls_it},
    {"rates are classified", rates_are_classified},
    {"an AAF reads and writes back out", an_aaf_reads_and_writes_back_out},
    {"a bundle carries its media with it", a_bundle_carries_its_media_with_it},
    {"reading an EDL finds its clips", reading_an_edl_finds_its_clips},
    {"the quickstart from the README runs", the_quickstart_from_the_readme_runs},
    {"open works out the format from the name", open_works_out_the_format_from_the_name},
    {"open declines a suffix no format claims", open_declines_a_suffix_no_format_claims},
    {"a timeline survives a round trip through JSON", a_timeline_survives_a_round_trip_through_json},
    {"saving and opening again keeps the clips", saving_and_opening_again_keeps_the_clips},
    {"writing bytes in every format the library knows", writing_bytes_in_every_format_the_library_knows},
    {"building a timeline from nothing", building_a_timeline_from_nothing},
    {"a freshly built object is enabled", a_freshly_built_object_is_enabled},
    {"no value is an answer and not a failure", no_value_is_an_answer_and_not_a_failure},
    {"an object knows which schemas it is", an_object_knows_which_schemas_it_is},
    {"clearing children hands them all back", clearing_children_hands_them_all_back},
    {"every child and its range come back together", every_child_and_its_range_come_back_together},
    {"a stale handle is refused", a_stale_handle_is_refused},
    {"an object of no timeline fails rather than crashing", an_object_of_no_timeline_fails_rather_than_crashing},
    {"an object from another timeline is refused", an_object_from_another_timeline_is_refused},
    {"the refusal of another timeline's object has a type of its own",
     the_refusal_of_another_timeline_s_object_has_a_type_of_its_own},
    {"metadata goes in and comes back", metadata_goes_in_and_comes_back},
    {"time values compute without a document", time_values_compute_without_a_document},
    {"an unreadable timecode is a failure", an_unreadable_timecode_is_a_failure},
    {"every failure carries its own message whatever thread it ran on",
     every_failure_carries_its_own_message_whatever_thread_it_ran_on},
    {"a range answers about what it covers", a_range_answers_about_what_it_covers},
    {"an object built on its own can join a timeline", an_object_built_on_its_own_can_join_a_timeline},
    {"an edit puts a newly built item into a track", an_edit_puts_a_newly_built_item_into_a_track},
    {"an object outliving its timeline fails rather than crashing",
     an_object_outliving_its_timeline_fails_rather_than_crashing},
    {"an object of an absorbed timeline follows it",
     an_object_of_an_absorbed_timeline_follows_it},
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
