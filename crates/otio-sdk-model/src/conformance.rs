//! What every SDK must agree on, written once.
//!
//! Each generated SDK has its own hand-written tests, and those catch a
//! binding that is broken. They cannot catch one that works and quietly
//! disagrees with the others about what the library does, because each suite
//! only knows its own language. Six of the seven targets hide the document
//! and carry a forwarding runtime written by hand for their language, and a
//! disagreement between those runtimes is the likeliest bug this project has
//! left.
//!
//! So the behaviour at the edges is stated here, as data, and every backend
//! renders it into its own test framework the way it renders a call site.
//! A scenario added here reaches every SDK by being written; a backend that
//! cannot render one fails generation, and the rendered tests are committed
//! beside the SDK and checked for drift like the rest of it.
//!
//! Three things keep this from turning into a programming language:
//!
//! 1. **The vocabulary is fixed and small.** A [`Step`] is one of a handful
//!    of things a scenario can do, with no conditionals and no loops. A
//!    behaviour that does not fit is a hand-written test in the backend that
//!    needs it, not a new kind of step that every backend then has to learn.
//! 2. **A failure is named by its kind, never by its message.** Go returns an
//!    `error`, Swift and C# throw, Zig has an error union. [`Failure`] says
//!    which status the library reported, or that the binding refused before
//!    asking it, and each backend maps that to its idiom once. A scenario
//!    that mentions a sentence is testing the wrong thing.
//! 3. **What applies where is in the data.** Zig keeps the document visible,
//!    so a scenario about what hiding it obliges a binding to do means nothing
//!    there. [`Applies`] says so per scenario, and it is one property of a
//!    target — hides the document, or does not — rather than a judgement each
//!    backend makes for itself.
//!
//! [`check`] holds the scenarios to those rules, and to one more: a scenario
//! every target runs has to mean the same thing whether the document is
//! hidden or not.

use std::collections::BTreeMap;

/// Which targets a scenario is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applies {
    /// Every target. The scenario is about the C ABI's own semantics, which
    /// no binding may change.
    Every,
    /// Only the targets that hide the document. The scenario is about what
    /// hiding it obliges a binding to do: moving an object into a timeline
    /// when it is placed there, and forwarding the handles that pointed into
    /// the timeline it came from.
    HiddenDocument,
}

/// One named behaviour, and the steps that show it.
#[derive(Debug, Clone, Copy)]
pub struct Scenario {
    /// Its name, in `snake_case`, which each backend spells as a test name.
    pub name: &'static str,
    /// What it shows and why it matters, for the rendered test's comment.
    pub docs: &'static str,
    /// Which targets run it.
    pub applies: Applies,
    /// What it does, in order. The first failed expectation fails the test.
    pub steps: &'static [Step],
}

/// A schema an object built by a scenario has, which a typed language needs
/// to declare the variable holding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A `Timeline`.
    Timeline,
    /// The `Stack` a timeline holds its tracks in.
    Stack,
    /// A `Track`.
    Track,
    /// A `Clip`.
    Clip,
}

impl Kind {
    /// The schema's name, as the SDKs spell the type.
    #[must_use]
    pub const fn schema(self) -> &'static str {
        match self {
            Self::Timeline => "Timeline",
            Self::Stack => "Stack",
            Self::Track => "Track",
            Self::Clip => "Clip",
        }
    }
}

/// One thing a scenario does.
///
/// Every object is named by a variable, which the step that builds it
/// introduces. Each of those steps also names the `timeline` the object is
/// built in. A target that keeps the document visible builds it in the
/// document of that name, making the document the first time the name
/// appears. A target that hides the document ignores the name: every object
/// starts out on its own and joins another when it is appended to it, and
/// [`check`] makes sure the two readings agree wherever a scenario runs on
/// both.
#[derive(Debug, Clone, Copy)]
pub enum Step {
    /// Builds a timeline, which arrives holding an empty stack of tracks.
    NewTimeline {
        /// The variable it is held in.
        var: &'static str,
        /// The timeline it is built in.
        timeline: &'static str,
        /// Its name.
        name: &'static str,
    },
    /// Names the stack a timeline holds its tracks in.
    Tracks {
        /// The variable the stack is held in.
        var: &'static str,
        /// The timeline to ask.
        of: &'static str,
    },
    /// Builds a track.
    NewTrack {
        /// The variable it is held in.
        var: &'static str,
        /// The timeline it is built in.
        timeline: &'static str,
        /// Its name.
        name: &'static str,
        /// Its kind, such as `"Video"`.
        kind: &'static str,
    },
    /// Builds a clip.
    NewClip {
        /// The variable it is held in.
        var: &'static str,
        /// The timeline it is built in.
        timeline: &'static str,
        /// Its name.
        name: &'static str,
    },
    /// Sets an item's source range: `start` and `duration`, both at `rate`.
    SetSourceRange {
        /// The item.
        item: &'static str,
        /// Where the range starts.
        start: f64,
        /// How long it is.
        duration: f64,
        /// The rate of both.
        rate: f64,
    },
    /// Appends a child to a composition.
    Append {
        /// The composition.
        parent: &'static str,
        /// What is appended to it.
        child: &'static str,
    },
    /// Removes an object from its timeline altogether. Every handle to it is
    /// stale afterwards.
    Remove {
        /// The object.
        var: &'static str,
    },
    /// Releases the timeline an object belongs to, as the language releases
    /// anything: `Close`, `dispose`, `deinit`. Nothing in that timeline may
    /// be used afterwards, and nothing outside it is touched.
    Release {
        /// An object in the timeline to release.
        var: &'static str,
    },
    /// Asks something and insists on the answer.
    Expect(Expect),
    /// Tries something that must not work, and insists on how it fails.
    Refused {
        /// What is tried.
        attempt: Attempt,
        /// How it must fail.
        failure: Failure,
    },
}

/// A question with one right answer.
#[derive(Debug, Clone, Copy)]
pub enum Expect {
    /// The object's name.
    Name {
        /// The object.
        var: &'static str,
        /// What it must be.
        is: &'static str,
    },
    /// How many children a composition has.
    ChildCount {
        /// The composition.
        var: &'static str,
        /// How many it must have.
        is: usize,
    },
    /// An item's duration, as a value at a rate. Compared exactly: every
    /// scenario's times are whole frames.
    Duration {
        /// The item.
        var: &'static str,
        /// The value it must have.
        value: f64,
        /// At this rate.
        rate: f64,
    },
    /// An object written as OpenTimelineIO JSON, indented by four spaces.
    ///
    /// This is the strongest check there is: the file every SDK writes for
    /// the same edits must be the same file, byte for byte.
    Json {
        /// The object to write.
        var: &'static str,
        /// What it must write.
        is: &'static str,
    },
}

/// Something a scenario tries that must fail.
#[derive(Debug, Clone, Copy)]
pub enum Attempt {
    /// Asks an object its name.
    Name {
        /// The object.
        var: &'static str,
    },
    /// Appends a child to a composition.
    Append {
        /// The composition.
        parent: &'static str,
        /// What is appended to it.
        child: &'static str,
    },
    /// Detaches a child from a composition, which only names the child: it
    /// has to be there already.
    Detach {
        /// The composition.
        parent: &'static str,
        /// The child.
        child: &'static str,
    },
    /// Flattens several tracks into one, which is handed them as a list.
    FlattenTracks {
        /// The tracks.
        tracks: &'static [&'static str],
    },
    /// Inserts an item into a composition at `time`, at `rate`, without
    /// removing transitions, handing it a fill template. It is the call that
    /// moves two objects, the item and the template.
    Insert {
        /// What is inserted.
        item: &'static str,
        /// Where it is inserted.
        composition: &'static str,
        /// When, in frames.
        time: f64,
        /// The rate of `time`.
        rate: f64,
        /// What fills any space the insert leaves.
        fill_template: &'static str,
    },
}

/// How an attempt fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The library refused, with this `OtioStatus`, named as its Rust
    /// variant: `"StaleHandle"`, `"CoreError"`.
    Status(&'static str),
    /// The binding refused before asking the library, because an object
    /// belongs to another timeline than the one the call is made in.
    ///
    /// This carries no status on purpose. A binding that let the foreign
    /// object through to the library would get *some* status back, and a
    /// scenario that accepted any failure could not tell that apart from a
    /// refusal — the defect that merged two timelines in #38 read as a pass
    /// in exactly that way.
    OtherTimeline,
}

/// Every scenario, in the order the rendered tests run them.
pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "building_a_timeline_writes_this_json",
        docs: "Three clips in a video track, built one call at a time, write the same OpenTimelineIO \
               JSON in every language.",
        applies: Applies::Every,
        steps: &[
            Step::NewTimeline {
                var: "timeline",
                timeline: "cut",
                name: "Cut",
            },
            Step::Tracks {
                var: "stack",
                of: "timeline",
            },
            Step::NewTrack {
                var: "track",
                timeline: "cut",
                name: "V1",
                kind: "Video",
            },
            Step::Append {
                parent: "stack",
                child: "track",
            },
            Step::NewClip {
                var: "a",
                timeline: "cut",
                name: "A",
            },
            Step::SetSourceRange {
                item: "a",
                start: 0.0,
                duration: 24.0,
                rate: 24.0,
            },
            Step::Append {
                parent: "track",
                child: "a",
            },
            Step::NewClip {
                var: "b",
                timeline: "cut",
                name: "B",
            },
            Step::SetSourceRange {
                item: "b",
                start: 24.0,
                duration: 48.0,
                rate: 24.0,
            },
            Step::Append {
                parent: "track",
                child: "b",
            },
            Step::Expect(Expect::ChildCount {
                var: "track",
                is: 2,
            }),
            Step::Expect(Expect::Duration {
                var: "track",
                value: 72.0,
                rate: 24.0,
            }),
            Step::Expect(Expect::Json {
                var: "timeline",
                is: include_str!("conformance/building_a_timeline.json"),
            }),
        ],
    },
    Scenario {
        name: "a_removed_object_s_handle_is_stale",
        docs: "Removing an object makes every handle to it stale: asking it anything fails with a \
               stale-handle status rather than reaching whatever takes its slot next.",
        applies: Applies::Every,
        steps: &[
            Step::NewTrack {
                var: "track",
                timeline: "cut",
                name: "V1",
                kind: "Video",
            },
            Step::NewClip {
                var: "clip",
                timeline: "cut",
                name: "doomed",
            },
            Step::Append {
                parent: "track",
                child: "clip",
            },
            Step::Remove { var: "clip" },
            Step::Refused {
                attempt: Attempt::Name { var: "clip" },
                failure: Failure::Status("StaleHandle"),
            },
            Step::Expect(Expect::Name {
                var: "track",
                is: "V1",
            }),
        ],
    },
    Scenario {
        name: "naming_an_object_from_another_timeline_is_refused",
        docs: "A call that only names an object refuses one from another timeline before asking the \
               library, and the refusal leaves both timelines independent: releasing the one that \
               refused leaves the other whole. Asserting only that the call failed would pass \
               against a binding that merged the two timelines first and failed afterwards.",
        applies: Applies::Every,
        steps: &[
            Step::NewTrack {
                var: "mine",
                timeline: "here",
                name: "V1",
                kind: "Video",
            },
            Step::NewClip {
                var: "ours",
                timeline: "here",
                name: "Ours",
            },
            Step::Append {
                parent: "mine",
                child: "ours",
            },
            Step::NewTrack {
                var: "elsewhere",
                timeline: "there",
                name: "V2",
                kind: "Video",
            },
            Step::NewClip {
                var: "theirs",
                timeline: "there",
                name: "Theirs",
            },
            Step::Append {
                parent: "elsewhere",
                child: "theirs",
            },
            Step::Refused {
                attempt: Attempt::Detach {
                    parent: "mine",
                    child: "theirs",
                },
                failure: Failure::OtherTimeline,
            },
            Step::Release { var: "mine" },
            Step::Expect(Expect::ChildCount {
                var: "elsewhere",
                is: 1,
            }),
            Step::Expect(Expect::Name {
                var: "theirs",
                is: "Theirs",
            }),
        ],
    },
    Scenario {
        name: "a_list_drawn_from_two_timelines_is_refused",
        docs: "A call handed a list refuses one whose objects come from two timelines, rather than \
               merging them, and both timelines stay independent.",
        applies: Applies::Every,
        steps: &[
            Step::NewTrack {
                var: "first",
                timeline: "here",
                name: "V1",
                kind: "Video",
            },
            Step::NewTrack {
                var: "second",
                timeline: "there",
                name: "V2",
                kind: "Video",
            },
            Step::Refused {
                attempt: Attempt::FlattenTracks {
                    tracks: &["first", "second"],
                },
                failure: Failure::OtherTimeline,
            },
            Step::Release { var: "first" },
            Step::Expect(Expect::Name {
                var: "second",
                is: "V2",
            }),
        ],
    },
    Scenario {
        name: "an_object_built_on_its_own_joins_the_track_it_is_appended_to",
        docs: "Upstream's `track.append(Clip(\"shot\"))`: a clip built on its own moves into the \
               track's timeline when it is appended, and the handle the caller already holds keeps \
               working there.",
        applies: Applies::HiddenDocument,
        steps: &[
            Step::NewTrack {
                var: "track",
                timeline: "cut",
                name: "V1",
                kind: "Video",
            },
            Step::NewClip {
                var: "clip",
                timeline: "alone",
                name: "shot",
            },
            Step::Append {
                parent: "track",
                child: "clip",
            },
            Step::Expect(Expect::ChildCount {
                var: "track",
                is: 1,
            }),
            Step::Expect(Expect::Name {
                var: "clip",
                is: "shot",
            }),
        ],
    },
    Scenario {
        name: "handles_forward_through_every_move",
        docs: "A clip appended to a track, which is then appended to a timeline's stack, has moved \
               twice. The handle to it still answers, and releasing the timeline it ended up in \
               releases it too.",
        applies: Applies::HiddenDocument,
        steps: &[
            Step::NewClip {
                var: "clip",
                timeline: "first",
                name: "shot",
            },
            Step::NewTrack {
                var: "track",
                timeline: "second",
                name: "V1",
                kind: "Video",
            },
            Step::Append {
                parent: "track",
                child: "clip",
            },
            Step::NewTimeline {
                var: "timeline",
                timeline: "third",
                name: "Cut",
            },
            Step::Tracks {
                var: "stack",
                of: "timeline",
            },
            Step::Append {
                parent: "stack",
                child: "track",
            },
            Step::Expect(Expect::Name {
                var: "clip",
                is: "shot",
            }),
            Step::Expect(Expect::ChildCount {
                var: "stack",
                is: 1,
            }),
        ],
    },
    Scenario {
        name: "an_object_with_a_parent_is_refused_and_both_timelines_stay_whole",
        docs: "Appending a clip that is already in another timeline's track is refused with the \
               core's own status, as upstream refuses it, and the refusal moves nothing: \
               releasing the timeline the clip is in leaves the track that refused it whole. A \
               binding that moved the clip's timeline in first and let the library refuse \
               afterwards would fail the same way and have merged the two, so releasing one \
               would release both (#75).",
        applies: Applies::HiddenDocument,
        steps: &[
            Step::NewTrack {
                var: "first",
                timeline: "here",
                name: "T1",
                kind: "Video",
            },
            Step::NewTrack {
                var: "second",
                timeline: "there",
                name: "T2",
                kind: "Video",
            },
            Step::NewClip {
                var: "clip",
                timeline: "alone",
                name: "C",
            },
            Step::Append {
                parent: "first",
                child: "clip",
            },
            Step::Refused {
                attempt: Attempt::Append {
                    parent: "second",
                    child: "clip",
                },
                failure: Failure::Status("CoreError"),
            },
            Step::Release { var: "first" },
            Step::Expect(Expect::Name {
                var: "second",
                is: "T2",
            }),
            Step::Expect(Expect::ChildCount {
                var: "second",
                is: 0,
            }),
        ],
    },
    Scenario {
        name: "a_stale_object_is_refused_and_both_timelines_stay_whole",
        docs: "Appending a clip whose handle has gone stale, because it was removed from the \
               timeline it was in, is refused with the stale-handle status the core gives, and \
               the refusal moves nothing: releasing that timeline leaves the track that refused \
               the clip whole. A binding that moved the stale clip's timeline in first and let \
               the library refuse afterwards would have merged the two, so releasing one would \
               release both.",
        applies: Applies::HiddenDocument,
        steps: &[
            Step::NewTrack {
                var: "first",
                timeline: "here",
                name: "T1",
                kind: "Video",
            },
            Step::NewTrack {
                var: "second",
                timeline: "there",
                name: "T2",
                kind: "Video",
            },
            Step::NewClip {
                var: "clip",
                timeline: "alone",
                name: "C",
            },
            Step::Append {
                parent: "first",
                child: "clip",
            },
            Step::Remove { var: "clip" },
            Step::Refused {
                attempt: Attempt::Append {
                    parent: "second",
                    child: "clip",
                },
                failure: Failure::Status("StaleHandle"),
            },
            Step::Release { var: "first" },
            Step::Expect(Expect::Name {
                var: "second",
                is: "T2",
            }),
            Step::Expect(Expect::ChildCount {
                var: "second",
                is: 0,
            }),
        ],
    },
    Scenario {
        name: "a_call_moving_two_objects_checks_both_before_moving_either",
        docs: "Inserting a live clip into a track with a stale fill template is refused with the \
               stale-handle status, and the refusal moves neither object: releasing the track \
               that refused the insert leaves the clip's own timeline whole. A binding that \
               moved the clip in and only then found the template stale would fail the same way \
               with the clip's timeline already merged into the track's, so releasing the track \
               would take the clip with it (#91).",
        applies: Applies::HiddenDocument,
        steps: &[
            Step::NewClip {
                var: "clip",
                timeline: "first",
                name: "C",
            },
            Step::NewTrack {
                var: "second",
                timeline: "second",
                name: "T2",
                kind: "Video",
            },
            Step::NewTrack {
                var: "third",
                timeline: "third",
                name: "T3",
                kind: "Video",
            },
            Step::NewClip {
                var: "filler",
                timeline: "fourth",
                name: "F",
            },
            Step::Append {
                parent: "third",
                child: "filler",
            },
            Step::Remove { var: "filler" },
            Step::Refused {
                attempt: Attempt::Insert {
                    item: "clip",
                    composition: "second",
                    time: 0.0,
                    rate: 24.0,
                    fill_template: "filler",
                },
                failure: Failure::Status("StaleHandle"),
            },
            Step::Release { var: "second" },
            Step::Expect(Expect::Name {
                var: "clip",
                is: "C",
            }),
            Step::Expect(Expect::Name {
                var: "third",
                is: "T3",
            }),
        ],
    },
];

/// A way a scenario breaks the rules this module is written to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The scenario.
    pub scenario: &'static str,
    /// What is wrong with it.
    pub message: String,
}

/// Holds every scenario to the rules, returning what breaks them.
///
/// A variable must be introduced before it is used and never twice, and a
/// [`Step::Tracks`] must ask a timeline. And a scenario that every target
/// runs must mean the same thing on both sides of the document fork. When the
/// document is visible, the objects built with the same `timeline` name share
/// one from the start; when it is hidden, each starts apart and they meet only
/// by being appended. So in a scenario for every target an append may not
/// cross timelines, a refusal for another timeline's object must be one on
/// both readings, and releasing a timeline must take the same objects with it
/// on both. Anywhere the two readings differed, the scenario would test two
/// different things and pass on both.
#[must_use]
pub fn check(scenarios: &[Scenario]) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut names = Vec::new();
    for scenario in scenarios {
        if names.contains(&scenario.name) {
            problems.push(Problem {
                scenario: scenario.name,
                message: "two scenarios have this name".to_string(),
            });
        }
        names.push(scenario.name);
        let mut report = |message: String| {
            problems.push(Problem {
                scenario: scenario.name,
                message,
            });
        };
        check_one(scenario, &mut report);
    }
    problems
}

/// What [`check`] knows about one variable as it walks a scenario.
struct Var {
    kind: Kind,
    /// The timeline it was built in, when the document is visible.
    visible: &'static str,
}

fn check_one(scenario: &Scenario, report: &mut impl FnMut(String)) {
    let mut vars: BTreeMap<&'static str, Var> = BTreeMap::new();
    // When the document is hidden, which objects share a timeline: each
    // starts on its own, and an append joins the two sides.
    let mut hidden = Joined::default();
    let every = scenario.applies == Applies::Every;

    let introduce = |vars: &mut BTreeMap<&'static str, Var>,
                     report: &mut dyn FnMut(String),
                     var: &'static str,
                     kind: Kind,
                     visible: &'static str| {
        if vars.contains_key(var) {
            report(format!("`{var}` is introduced twice"));
        }
        vars.insert(var, Var { kind, visible });
    };
    let known = |vars: &BTreeMap<&'static str, Var>, report: &mut dyn FnMut(String), var: &str| {
        if !vars.contains_key(var) {
            report(format!("`{var}` is used before it is introduced"));
        }
    };

    for step in scenario.steps {
        match *step {
            Step::NewTimeline { var, timeline, .. } => {
                introduce(&mut vars, report, var, Kind::Timeline, timeline);
                hidden.add(var);
            }
            Step::NewTrack { var, timeline, .. } => {
                introduce(&mut vars, report, var, Kind::Track, timeline);
                hidden.add(var);
            }
            Step::NewClip { var, timeline, .. } => {
                introduce(&mut vars, report, var, Kind::Clip, timeline);
                hidden.add(var);
            }
            Step::Tracks { var, of } => {
                known(&vars, report, of);
                let timeline = vars.get(of).map(|of| (of.kind, of.visible));
                if let Some((kind, visible)) = timeline {
                    if kind != Kind::Timeline {
                        report(format!(
                            "`{of}` is asked for its tracks but is not a timeline"
                        ));
                    }
                    introduce(&mut vars, report, var, Kind::Stack, visible);
                    hidden.add(var);
                    hidden.join(var, of);
                }
            }
            Step::SetSourceRange { item, .. } => known(&vars, report, item),
            Step::Append { parent, child } => {
                known(&vars, report, parent);
                known(&vars, report, child);
                if every && !same_visible(&vars, parent, child) {
                    report(format!(
                        "`{child}` is appended to `{parent}` from another timeline, which only a \
                         target that hides the document can do; mark the scenario \
                         `Applies::HiddenDocument`"
                    ));
                }
                hidden.join(parent, child);
            }
            Step::Remove { var } => known(&vars, report, var),
            Step::Release { var } => {
                known(&vars, report, var);
                // Releasing a timeline has to release the same objects on
                // both readings, or what survives it differs between them.
                if every {
                    let differ = vars
                        .keys()
                        .find(|other| same_visible(&vars, var, other) != hidden.same(var, other));
                    if let Some(other) = differ {
                        report(format!(
                            "releasing `{var}` would take `{other}` with it on one reading of \
                             the document fork and not the other; append it first, or build it \
                             in another timeline"
                        ));
                    }
                }
            }
            Step::Expect(expect) => {
                let var = match expect {
                    Expect::Name { var, .. }
                    | Expect::ChildCount { var, .. }
                    | Expect::Duration { var, .. }
                    | Expect::Json { var, .. } => var,
                };
                known(&vars, report, var);
            }
            Step::Refused { attempt, failure } => {
                let objects: Vec<&'static str> = match attempt {
                    Attempt::Name { var } => vec![var],
                    Attempt::Append { parent, child } | Attempt::Detach { parent, child } => {
                        vec![parent, child]
                    }
                    Attempt::FlattenTracks { tracks } => tracks.to_vec(),
                    Attempt::Insert {
                        item,
                        composition,
                        fill_template,
                        ..
                    } => vec![item, composition, fill_template],
                };
                for var in &objects {
                    known(&vars, report, var);
                }
                if failure == Failure::OtherTimeline {
                    let apart_visible = objects
                        .windows(2)
                        .any(|pair| !same_visible(&vars, pair[0], pair[1]));
                    let apart_hidden = objects
                        .windows(2)
                        .any(|pair| !hidden.same(pair[0], pair[1]));
                    if !apart_hidden || (every && !apart_visible) {
                        report(
                            "expects a refusal for another timeline's object, but every object \
                             in the attempt shares one timeline"
                                .to_string(),
                        );
                    }
                }
            }
        }
    }
}

fn same_visible(vars: &BTreeMap<&'static str, Var>, left: &str, right: &str) -> bool {
    match (vars.get(left), vars.get(right)) {
        (Some(left), Some(right)) => left.visible == right.visible,
        _ => false,
    }
}

/// Which objects share a timeline when the document is hidden.
#[derive(Default)]
struct Joined {
    parent: BTreeMap<&'static str, &'static str>,
}

impl Joined {
    fn add(&mut self, var: &'static str) {
        self.parent.entry(var).or_insert(var);
    }

    fn root(&self, var: &str) -> Option<&'static str> {
        let mut current = *self.parent.get(var)?;
        while let Some(&next) = self.parent.get(current) {
            if next == current {
                return Some(current);
            }
            current = next;
        }
        Some(current)
    }

    fn join(&mut self, left: &str, right: &str) {
        if let (Some(left), Some(right)) = (self.root(left), self.root(right)) {
            self.parent.insert(right, left);
        }
    }

    fn same(&self, left: &str, right: &str) -> bool {
        match (self.root(left), self.root(right)) {
            (Some(left), Some(right)) => left == right,
            _ => false,
        }
    }
}

/// The variables a scenario introduces, in order, with their kinds.
///
/// A backend for a typed language declares these up front, or at the step
/// that introduces each, and needs the kind to do it.
#[must_use]
pub fn variables(scenario: &Scenario) -> Vec<(&'static str, Kind)> {
    scenario
        .steps
        .iter()
        .filter_map(|step| match *step {
            Step::NewTimeline { var, .. } => Some((var, Kind::Timeline)),
            Step::Tracks { var, .. } => Some((var, Kind::Stack)),
            Step::NewTrack { var, .. } => Some((var, Kind::Track)),
            Step::NewClip { var, .. } => Some((var, Kind::Clip)),
            _ => None,
        })
        .collect()
}

/// The timelines a scenario names, in the order they first appear.
///
/// A target that keeps the document visible makes one document for each.
#[must_use]
pub fn timelines(scenario: &Scenario) -> Vec<&'static str> {
    let mut seen = Vec::new();
    for step in scenario.steps {
        let timeline = match *step {
            Step::NewTimeline { timeline, .. }
            | Step::NewTrack { timeline, .. }
            | Step::NewClip { timeline, .. } => timeline,
            _ => continue,
        };
        if !seen.contains(&timeline) {
            seen.push(timeline);
        }
    }
    seen
}

/// The timeline, by its visible-document name, that an object was built in.
#[must_use]
pub fn timeline_of(scenario: &Scenario, var: &str) -> Option<&'static str> {
    let mut built: BTreeMap<&'static str, &'static str> = BTreeMap::new();
    for step in scenario.steps {
        match *step {
            Step::NewTimeline {
                var: name,
                timeline,
                ..
            }
            | Step::NewTrack {
                var: name,
                timeline,
                ..
            }
            | Step::NewClip {
                var: name,
                timeline,
                ..
            } => {
                built.insert(name, timeline);
            }
            Step::Tracks { var: name, of } => {
                if let Some(&timeline) = built.get(of) {
                    built.insert(name, timeline);
                }
            }
            _ => {}
        }
    }
    built.get(var).copied()
}

/// The scenarios a target runs.
pub fn for_target(hides_document: bool) -> impl Iterator<Item = &'static Scenario> {
    SCENARIOS
        .iter()
        .filter(move |scenario| hides_document || scenario.applies == Applies::Every)
}

/// Writes the scenarios out as JSON, ending in a newline.
///
/// The Rust above is where they are written and checked; this is the same
/// data for review, and for anything that reads the SDK description without
/// linking this crate. Like `sdk/api.json` it is committed, and a scenario
/// changed without regenerating fails the drift check.
#[must_use]
pub fn render(scenarios: &[Scenario]) -> String {
    let mut out = String::from("{\n");
    out.push_str(
        "  \"//\": \"This file is generated from crates/otio-sdk-model/src/conformance.rs.\",\n",
    );
    out.push_str(
        "  \"//\": \"Edit the scenarios there, then run `cargo run -p otio-sdk-gen`.\",\n",
    );
    out.push_str("  \"scenarios\": [");
    for (index, scenario) in scenarios.iter().enumerate() {
        out.push_str(if index == 0 { "\n" } else { ",\n" });
        out.push_str("    {\n");
        out.push_str(&format!(
            "      \"name\": {},\n",
            json_string(scenario.name)
        ));
        out.push_str(&format!(
            "      \"docs\": {},\n",
            json_string(scenario.docs)
        ));
        let applies = match scenario.applies {
            Applies::Every => "every",
            Applies::HiddenDocument => "hidden_document",
        };
        out.push_str(&format!("      \"applies\": \"{applies}\",\n"));
        out.push_str("      \"steps\": [");
        for (index, step) in scenario.steps.iter().enumerate() {
            out.push_str(if index == 0 { "\n" } else { ",\n" });
            out.push_str("        ");
            out.push_str(&step_json(step));
        }
        out.push_str("\n      ]\n    }");
    }
    out.push_str("\n  ]\n}\n");
    out
}

fn step_json(step: &Step) -> String {
    let s = json_string;
    match *step {
        Step::NewTimeline {
            var,
            timeline,
            name,
        } => format!(
            "{{\"step\": \"new_timeline\", \"var\": {}, \"timeline\": {}, \"name\": {}}}",
            s(var),
            s(timeline),
            s(name)
        ),
        Step::Tracks { var, of } => format!(
            "{{\"step\": \"tracks\", \"var\": {}, \"of\": {}}}",
            s(var),
            s(of)
        ),
        Step::NewTrack {
            var,
            timeline,
            name,
            kind,
        } => format!(
            "{{\"step\": \"new_track\", \"var\": {}, \"timeline\": {}, \"name\": {}, \"kind\": {}}}",
            s(var),
            s(timeline),
            s(name),
            s(kind)
        ),
        Step::NewClip {
            var,
            timeline,
            name,
        } => format!(
            "{{\"step\": \"new_clip\", \"var\": {}, \"timeline\": {}, \"name\": {}}}",
            s(var),
            s(timeline),
            s(name)
        ),
        Step::SetSourceRange {
            item,
            start,
            duration,
            rate,
        } => format!(
            "{{\"step\": \"set_source_range\", \"item\": {}, \"start\": {start}, \"duration\": {duration}, \"rate\": {rate}}}",
            s(item)
        ),
        Step::Append { parent, child } => format!(
            "{{\"step\": \"append\", \"parent\": {}, \"child\": {}}}",
            s(parent),
            s(child)
        ),
        Step::Remove { var } => format!("{{\"step\": \"remove\", \"var\": {}}}", s(var)),
        Step::Release { var } => format!("{{\"step\": \"release\", \"var\": {}}}", s(var)),
        Step::Expect(expect) => match expect {
            Expect::Name { var, is } => format!(
                "{{\"step\": \"expect_name\", \"var\": {}, \"is\": {}}}",
                s(var),
                s(is)
            ),
            Expect::ChildCount { var, is } => format!(
                "{{\"step\": \"expect_child_count\", \"var\": {}, \"is\": {is}}}",
                s(var)
            ),
            Expect::Duration { var, value, rate } => format!(
                "{{\"step\": \"expect_duration\", \"var\": {}, \"value\": {value}, \"rate\": {rate}}}",
                s(var)
            ),
            Expect::Json { var, is } => format!(
                "{{\"step\": \"expect_json\", \"var\": {}, \"is\": {}}}",
                s(var),
                s(is)
            ),
        },
        Step::Refused { attempt, failure } => {
            let attempt = match attempt {
                Attempt::Name { var } => format!("{{\"call\": \"name\", \"var\": {}}}", s(var)),
                Attempt::Append { parent, child } => format!(
                    "{{\"call\": \"append\", \"parent\": {}, \"child\": {}}}",
                    s(parent),
                    s(child)
                ),
                Attempt::Detach { parent, child } => format!(
                    "{{\"call\": \"detach\", \"parent\": {}, \"child\": {}}}",
                    s(parent),
                    s(child)
                ),
                Attempt::FlattenTracks { tracks } => {
                    let list: Vec<String> = tracks.iter().map(|track| s(track)).collect();
                    format!(
                        "{{\"call\": \"flatten_tracks\", \"tracks\": [{}]}}",
                        list.join(", ")
                    )
                }
                Attempt::Insert {
                    item,
                    composition,
                    time,
                    rate,
                    fill_template,
                } => format!(
                    "{{\"call\": \"insert\", \"item\": {}, \"composition\": {}, \"time\": {time}, \"rate\": {rate}, \"fill_template\": {}}}",
                    s(item),
                    s(composition),
                    s(fill_template)
                ),
            };
            let failure = match failure {
                Failure::Status(status) => format!("{{\"status\": {}}}", s(status)),
                Failure::OtherTimeline => "{\"other_timeline\": true}".to_string(),
            };
            format!("{{\"step\": \"refused\", \"attempt\": {attempt}, \"failure\": {failure}}}")
        }
    }
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::{Applies, Attempt, Failure, SCENARIOS, Scenario, Step, check};

    #[test]
    fn every_scenario_keeps_the_rules() {
        let problems = check(SCENARIOS);
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn a_scenario_for_everyone_may_not_append_across_timelines() {
        const BAD: &[Scenario] = &[Scenario {
            name: "bad",
            docs: "",
            applies: Applies::Every,
            steps: &[
                Step::NewTrack {
                    var: "track",
                    timeline: "one",
                    name: "V1",
                    kind: "Video",
                },
                Step::NewClip {
                    var: "clip",
                    timeline: "two",
                    name: "c",
                },
                Step::Append {
                    parent: "track",
                    child: "clip",
                },
            ],
        }];
        assert!(!check(BAD).is_empty());
    }

    #[test]
    fn a_scenario_for_everyone_may_not_release_what_only_one_reading_joins() {
        // Visible, both are in "one"; hidden, nothing joined them, so
        // releasing the track would take the clip with it only in Zig.
        const BAD: &[Scenario] = &[Scenario {
            name: "bad",
            docs: "",
            applies: Applies::Every,
            steps: &[
                Step::NewTrack {
                    var: "track",
                    timeline: "one",
                    name: "V1",
                    kind: "Video",
                },
                Step::NewClip {
                    var: "clip",
                    timeline: "one",
                    name: "c",
                },
                Step::Release { var: "track" },
            ],
        }];
        assert!(!check(BAD).is_empty());
    }

    #[test]
    fn a_refusal_for_another_timeline_needs_two_timelines() {
        const BAD: &[Scenario] = &[Scenario {
            name: "bad",
            docs: "",
            applies: Applies::HiddenDocument,
            steps: &[
                Step::NewTrack {
                    var: "track",
                    timeline: "one",
                    name: "V1",
                    kind: "Video",
                },
                Step::Refused {
                    attempt: Attempt::FlattenTracks { tracks: &["track"] },
                    failure: Failure::OtherTimeline,
                },
            ],
        }];
        assert!(!check(BAD).is_empty());
    }
}
