//! Deciding what each C entry point becomes in TypeScript.
//!
//! A C signature says what a call takes and returns. It does not say which
//! parameter is the object the call is about, which one is the answer, which
//! may be left out, or which three of them are one list. Without those, a
//! generator can only transliterate, and a transliteration of C is not
//! TypeScript.
//!
//! The C ABI is regular enough that all four can be read off its own
//! conventions rather than declared by hand:
//!
//! - A leading `*const OtioDocument` or `*mut OtioDocument` is the document
//!   the call works on, which in TypeScript comes from the receiver.
//! - The `OtioNode` after it is the object the call is about, which is `this`.
//! - Parameters named `out_` are results, delivered through pointers because C
//!   has no other way; TypeScript returns them.
//! - An `out_` pointer beside a `capacity` and an `out_count` is a list, asked
//!   for twice because C cannot grow an array.
//! - The prefix of the name says which type the call belongs to:
//!   `otio_track_kind` is `Track#kind`.
//!
//! So the rules live here, and anything they cannot place is an error that
//! stops generation. That is the part that matters: a function added to the C
//! ABI in a shape nobody anticipated fails the build rather than going quietly
//! missing from the SDK.

use std::collections::{BTreeMap, BTreeSet};

use super::abi::{Abi, Function, Type};

/// A value crossing into a call.
#[derive(Debug, Clone)]
pub enum Input {
    /// A number, of whatever width C spells it with.
    Number {
        /// The TypeScript parameter name.
        name: String,
        /// The C type, which decides how it is marshalled.
        kind: Type,
    },
    /// A `bool`.
    Boolean {
        /// The TypeScript parameter name.
        name: String,
    },
    /// One of the ABI's enums, as a string-literal union.
    Enumeration {
        /// The TypeScript parameter name.
        name: String,
        /// The TypeScript type.
        ts: String,
    },
    /// A struct passed by value.
    Record {
        /// The TypeScript parameter name.
        name: String,
        /// The TypeScript type.
        ts: String,
        /// Whether it arrives as `*const T`, where null means "not given".
        optional: bool,
    },
    /// A string.
    Text {
        /// The TypeScript parameter name.
        name: String,
        /// Whether null is accepted.
        optional: bool,
    },
    /// A block of bytes, spelled in C as a pointer and a length.
    Bytes {
        /// The TypeScript parameter name.
        name: String,
        /// The name of the length parameter that goes with it.
        length: String,
    },
    /// A handle to an object.
    Node {
        /// The TypeScript parameter name.
        name: String,
        /// Whether the none handle is accepted.
        optional: bool,
    },
    /// An array of handles, spelled in C as a pointer and a count.
    NodeList {
        /// The TypeScript parameter name.
        name: String,
        /// The name of the count parameter that goes with it.
        length: String,
    },
}

impl Input {
    /// The parameter's name in TypeScript.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Number { name, .. }
            | Self::Boolean { name }
            | Self::Enumeration { name, .. }
            | Self::Record { name, .. }
            | Self::Text { name, .. }
            | Self::Bytes { name, .. }
            | Self::Node { name, .. }
            | Self::NodeList { name, .. } => name,
        }
    }

    /// Whether the parameter may be left out.
    #[must_use]
    pub fn optional(&self) -> bool {
        matches!(
            self,
            Self::Record { optional: true, .. }
                | Self::Text { optional: true, .. }
                | Self::Node { optional: true, .. }
        )
    }
}

/// A value coming back out of a call.
#[derive(Debug, Clone)]
pub enum Output {
    /// A number.
    Number {
        /// The name the C ABI gave it, without its `out_`.
        name: String,
        /// The C type.
        kind: Type,
    },
    /// A `bool`.
    Boolean {
        /// The name the C ABI gave it, without its `out_`.
        name: String,
    },
    /// One of the ABI's enums.
    Enumeration {
        /// The name the C ABI gave it, without its `out_`.
        name: String,
        /// The TypeScript type.
        ts: String,
    },
    /// A struct.
    Record {
        /// The name the C ABI gave it, without its `out_`.
        name: String,
        /// The TypeScript type.
        ts: String,
    },
    /// An owned buffer, read as text.
    Text {
        /// The name the C ABI gave it, without its `out_`.
        name: String,
    },
    /// An owned buffer, read as bytes.
    Bytes {
        /// The name the C ABI gave it, without its `out_`.
        name: String,
    },
    /// A handle to an object.
    Node {
        /// The name the C ABI gave it, without its `out_`.
        name: String,
    },
    /// A whole new document.
    Document {
        /// The name the C ABI gave it, without its `out_`.
        name: String,
    },
}

impl Output {
    /// The result's name, which becomes a property name when there are
    /// several of them.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Number { name, .. }
            | Self::Boolean { name }
            | Self::Enumeration { name, .. }
            | Self::Record { name, .. }
            | Self::Text { name }
            | Self::Bytes { name }
            | Self::Node { name }
            | Self::Document { name } => name,
        }
    }
}

/// What the call is about, and where TypeScript finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Receiver {
    /// Nothing: a free function.
    None,
    /// A document, and nothing else: a method on `Document`.
    Document,
    /// A document reached through one of the arguments, which is how a free
    /// function that edits a timeline finds the arena it is editing.
    Borrowed(String),
    /// A document and a node: a method on one of the object classes.
    Node,
    /// A struct passed by value: a method on one of the value classes.
    Value(String),
}

/// One entry point, as TypeScript will spell it.
#[derive(Debug, Clone)]
pub struct Member {
    /// The C entry point this came from.
    pub symbol: String,
    /// Its doc comment.
    pub doc: Vec<String>,
    /// The TypeScript type it hangs off, or the empty string for a free
    /// function.
    pub owner: String,
    /// What it is called in TypeScript.
    pub name: String,
    /// The object it is grouped under, for a free function that has one.
    pub namespace: Option<String>,
    /// Where the call finds the thing it is about.
    pub receiver: Receiver,
    /// Whether it is a static member rather than an instance one.
    pub statik: bool,
    /// Whether it builds a new object, and so becomes the class's constructor.
    pub constructs: bool,
    /// Whether the call edits the document it is given.
    ///
    /// The C ABI says so in its signature: a call that edits takes
    /// `*mut OtioDocument` and a call that only asks takes `*const`. It
    /// matters because an object passed to an editing call has to be brought
    /// into the receiver's document first, and doing that to an object passed
    /// to a question would be a side effect nobody asked for.
    pub mutates: bool,
    /// Which parameter of the C entry point the receiver came from, if any.
    ///
    /// The emitter needs the position rather than the name: the receiver is
    /// the first handle the call takes, which is not always the parameter
    /// immediately after the document.
    pub receiver_at: Option<usize>,
    /// Its arguments, in order.
    pub inputs: Vec<Input>,
    /// Its results.
    pub outputs: Vec<Output>,
    /// Whether the results come back as a list, asked for in two passes.
    pub list: bool,
    /// Whether the call can answer "there is nothing", which is `undefined`
    /// rather than a thrown error.
    pub no_value: bool,
    /// Whether the call reports failure through `OtioStatus`.
    pub fallible: bool,
    /// What it returns directly, when it does not use out-parameters.
    pub returns: Type,
}

/// The whole SDK, planned.
#[derive(Debug, Clone, Default)]
pub struct Sdk {
    /// The classes, keyed by their TypeScript name.
    pub classes: BTreeMap<String, Class>,
    /// Members belonging to no class.
    pub free: Vec<Member>,
    /// Members the low-level layer binds but the classes do not expose.
    ///
    /// These are not missing: `raw.ts` is the whole C ABI, faithfully, and
    /// something has to call `otio_document_new`. What they are is not part of
    /// the surface, because the hand-written layers above put them to work in
    /// a shape the rules here would not have produced.
    pub internal: Vec<Member>,
    /// Entry points deliberately left off the generated surface, with the
    /// reason, so the exhaustiveness check can account for every one of them.
    pub handled_elsewhere: BTreeMap<String, String>,
}

/// The object classes, in the order they are declared, each with its base.
///
/// The shape follows upstream OpenTimelineIO's schema hierarchy, which is what
/// `OtioNodeKind` reports and what a user coming from the Python or C++ API
/// expects to find.
pub const HIERARCHY: &[(&str, Option<&str>)] = &[
    ("Node", None),
    ("Composable", Some("Node")),
    ("Item", Some("Composable")),
    ("Composition", Some("Item")),
    ("Track", Some("Composition")),
    ("Stack", Some("Composition")),
    ("Clip", Some("Item")),
    ("Gap", Some("Item")),
    ("Transition", Some("Composable")),
    ("Timeline", Some("Node")),
    ("SerializableCollection", Some("Node")),
    ("Marker", Some("Node")),
    ("Effect", Some("Node")),
    ("TimeEffect", Some("Effect")),
    ("LinearTimeWarp", Some("TimeEffect")),
    ("FreezeFrame", Some("LinearTimeWarp")),
    ("MediaReference", Some("Node")),
    ("ExternalReference", Some("MediaReference")),
    ("GeneratorReference", Some("MediaReference")),
    ("ImageSequenceReference", Some("MediaReference")),
    ("MissingReference", Some("MediaReference")),
];

/// Entry points whose name puts them in the wrong place.
///
/// The C ABI groups a call by what it needs, and `otio_document_deep_clone`
/// needs a document. TypeScript groups it by what it is about, which is the
/// object being copied. These few say where each such call really belongs,
/// and they are the whole exception list: everything else follows the prefix.
pub const OVERRIDES: &[(&str, &str, &str)] = &[
    ("otio_document_deep_clone", "Node", "deepClone"),
    ("otio_document_remove", "Node", "remove"),
    ("otio_document_remove_recursive", "Node", "removeRecursive"),
    ("otio_document_contains", "Node", "isLive"),
    // `kind` is taken: a track's kind is what it carries, and a generator's is
    // what it generates. This one says which schema an object is, which is
    // what upstream's `schema_name()` answers, so it is named after that.
    ("otio_node_kind", "Node", "schemaKind"),
];

/// What a handle a call hands back, or takes, actually is.
///
/// The C ABI says `OtioNode` for everything, because C has one handle type.
/// Its prose says more — a timeline's tracks are a stack, `find_clips` finds
/// clips — and a caller in a language with classes should get that back. The
/// wrapper's real class is decided at run time from the object's kind, so
/// these are a promise the ABI already makes, not a cast being papered over.
pub const NODE_TYPES: &[(&str, &str)] = &[
    ("otio_algorithm_flatten_stack", "Track"),
    ("otio_algorithm_flatten_tracks", "Track"),
    ("otio_algorithm_track_trimmed_to_range", "Track"),
    ("otio_clip_media_reference", "MediaReference"),
    ("otio_clip_remove_media_reference", "MediaReference"),
    ("otio_clip_set_media_reference", "MediaReference"),
    ("otio_composition_append_child", "Composable"),
    ("otio_composition_child_at_time", "Composable"),
    ("otio_composition_children_in_range", "Composable"),
    ("otio_composition_clear_children", "Composable"),
    ("otio_composition_detach_child", "Composable"),
    ("otio_composition_find_children_of_kind", "Composable"),
    ("otio_composition_handles_of_child", "Composable"),
    ("otio_composition_has_child", "Composable"),
    ("otio_composition_index_of_child", "Composable"),
    ("otio_composition_insert_child", "Composable"),
    ("otio_composition_neighbors_of", "Composable"),
    ("otio_composition_range_of_child", "Composable"),
    ("otio_composition_ranges_of_children", "Composable"),
    ("otio_composition_remove_child", "Composable"),
    ("otio_composition_trimmed_range_of_child", "Composable"),
    ("otio_item_append_effect", "Effect"),
    ("otio_item_append_marker", "Marker"),
    ("otio_item_effect_at", "Effect"),
    ("otio_item_marker_at", "Marker"),
    ("otio_item_remove_effect", "Effect"),
    ("otio_item_remove_marker", "Marker"),
    ("otio_node_child_at", "Composable"),
    ("otio_node_children", "Composable"),
    ("otio_node_find_clips", "Clip"),
    ("otio_timeline_set_tracks", "Stack"),
    ("otio_timeline_tracks", "Stack"),
];

/// The class a call's handles belong to, which is `Node` unless
/// [`NODE_TYPES`] says otherwise.
#[must_use]
pub fn node_class(symbol: &str) -> &'static str {
    NODE_TYPES
        .iter()
        .find(|(name, _)| *name == symbol)
        .map_or("Node", |(_, class)| *class)
}

/// The free functions that are grouped under a name of their own.
///
/// The ten edit operations and the three algorithms are not about one object,
/// so they are not methods; grouping them keeps `edit.insert` and
/// `algorithms.flattenStack` from becoming `editInsert` and
/// `algorithmFlattenStack` at the top level of the module.
pub const NAMESPACES: &[(&str, &str)] =
    &[("otio_algorithm_", "algorithms"), ("otio_edit_", "edit")];

/// The value classes: plain data with methods, and no document behind them.
pub const VALUES: &[&str] = &["RationalTime", "TimeRange", "TimeTransform"];

/// Structs that cross the boundary as plain objects rather than classes.
pub const PLAIN: &[&str] = &[
    "OtioColor",
    "OtioV2d",
    "OtioBox2d",
    "OtioImageSequence",
    "OtioHandles",
    "OtioReadOptions",
    "OtioWriteOptions",
];

/// Which TypeScript type each name prefix belongs to, longest first.
fn owners() -> Vec<(&'static str, &'static str)> {
    let mut table = vec![
        ("otio_image_sequence_reference_", "ImageSequenceReference"),
        ("otio_serializable_collection_", "SerializableCollection"),
        ("otio_generator_reference_", "GeneratorReference"),
        ("otio_external_reference_", "ExternalReference"),
        ("otio_missing_reference_", "MissingReference"),
        ("otio_media_reference_", "MediaReference"),
        ("otio_linear_time_warp_", "LinearTimeWarp"),
        ("otio_time_transform_", "TimeTransform"),
        ("otio_rational_time_", "RationalTime"),
        ("otio_freeze_frame_", "FreezeFrame"),
        ("otio_time_effect_", "TimeEffect"),
        ("otio_composition_", "Composition"),
        ("otio_composable_", "Composable"),
        ("otio_transition_", "Transition"),
        ("otio_time_range_", "TimeRange"),
        ("otio_document_", "Document"),
        ("otio_timeline_", "Timeline"),
        ("otio_effect_", "Effect"),
        ("otio_marker_", "Marker"),
        ("otio_track_", "Track"),
        ("otio_stack_", "Stack"),
        ("otio_node_", "Node"),
        ("otio_item_", "Item"),
        ("otio_clip_", "Clip"),
        ("otio_gap_", "Gap"),
    ];
    table.sort_by_key(|(prefix, _)| std::cmp::Reverse(prefix.len()));
    table
}

/// Entry points the generated surface leaves alone, and why.
///
/// Each of these is either the runtime's own business, hand-written because
/// TypeScript wants a shape no rule here would produce, or meaningless in a
/// browser. Naming them is what lets the exhaustiveness check insist that
/// every other entry point is accounted for.
fn handled_elsewhere() -> BTreeMap<String, String> {
    let mut reserved: BTreeMap<String, String> = BTreeMap::new();
    let mut note = |symbol: &str, why: &str| {
        reserved.insert(symbol.to_string(), why.to_string());
    };

    note(
        "otio_buffer_free",
        "the runtime frees every buffer it reads",
    );
    note("otio_document_free", "`Document#free` and its disposer");
    note(
        "otio_document_new",
        "a document is made for each object built",
    );
    for symbol in [
        "otio_document_absorb",
        "otio_document_clone",
        "otio_document_from_json",
        "otio_document_node_count",
        "otio_document_root",
        "otio_document_set_root",
        "otio_document_to_json",
    ] {
        note(
            symbol,
            "the document layer, which keeps the arena out of the API",
        );
    }
    for symbol in ["otio_read_from_bytes", "otio_write_to_bytes"] {
        note(symbol, "the `adapters` namespace, which owns the documents");
    }
    note(
        "otio_error_message",
        "read by the runtime when a call fails",
    );
    note("otio_status_name", "`OtioError` names its own status");
    note("otio_node_none", "absence is `undefined` in TypeScript");
    note("otio_node_is_none", "absence is `undefined` in TypeScript");
    note("otio_node_equal", "`Node#equals`, written by hand");
    note(
        "otio_read_options_default",
        "an options object with every field left out",
    );
    note(
        "otio_write_options_default",
        "an options object with every field left out",
    );
    for symbol in [
        "otio_read_from_file",
        "otio_write_to_file",
        "otio_document_read_from_file",
        "otio_document_write_to_file",
    ] {
        note(symbol, "there is no filesystem behind a WebAssembly module");
    }
    reserved
}

/// Turns a `snake_case` name into `camelCase`.
#[must_use]
pub fn camel(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper = false;
    for character in name.chars() {
        if character == '_' {
            upper = true;
        } else if upper {
            out.extend(character.to_uppercase());
            upper = false;
        } else {
            out.push(character);
        }
    }
    out
}

/// Turns a `snake_case` name into `PascalCase`.
#[must_use]
pub fn pascal(name: &str) -> String {
    let camel = camel(name);
    let mut characters = camel.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => camel,
    }
}

/// The TypeScript name of one of the ABI's types.
///
/// `OtioNode` becomes `NodeHandle` rather than `Node`, because `Node` is the
/// base class of the object model and a handle is the two integers behind it.
#[must_use]
pub fn ts_name(otio: &str) -> String {
    if otio == "OtioNode" {
        return "NodeHandle".to_string();
    }
    otio.strip_prefix("Otio").unwrap_or(otio).to_string()
}

/// Plans the whole SDK.
///
/// # Errors
///
/// Fails on any entry point the rules cannot place, naming it and saying what
/// about it was not understood.
pub fn plan(abi: &Abi) -> Result<Sdk, String> {
    let reserved = handled_elsewhere();
    let owners = owners();
    let mut sdk = Sdk {
        handled_elsewhere: reserved.clone(),
        ..Sdk::default()
    };
    let mut collected: BTreeMap<String, Vec<Member>> = BTreeMap::new();

    for function in &abi.functions {
        // Consuming the source document makes this one unlike every other
        // call, and the two-pass protocol the emitter would use for its list
        // cannot work when the first pass destroys the thing being asked
        // about. The document layer marshals it by hand.
        if function.name == "otio_document_absorb" {
            continue;
        }
        // The one struct the runtime passes by value is a buffer, and it does
        // so where it reads one: `readBuffer` frees what it has just copied,
        // so `OtioBuffer` never has to appear in the generated types at all.
        if function.name == "otio_buffer_free" {
            continue;
        }
        if reserved.contains_key(&function.name) {
            sdk.internal.push(plan_one(function, &owners, abi)?);
            continue;
        }
        // Metadata is a tree of free-form values addressed by path. Forty
        // typed getters and setters is the right shape for C and the wrong one
        // for TypeScript, which wants one `get` that reads the kind and hands
        // back a JavaScript value. That wrapper is written by hand over the
        // generated low-level calls.
        if function.name.starts_with("otio_metadata_") {
            sdk.handled_elsewhere.insert(
                function.name.clone(),
                "`Metadata`, one `get` and one `set` over the typed calls".to_string(),
            );
            sdk.internal.push(plan_one(function, &owners, abi)?);
            continue;
        }

        let member = plan_one(function, &owners, abi)?;
        match member.owner.as_str() {
            "" => sdk.free.push(member),
            owner => collected.entry(owner.to_string()).or_default().push(member),
        }
    }

    for (owner, members) in collected {
        sdk.classes.insert(owner, group(members));
    }
    sdk.free.sort_by(|a, b| a.name.cmp(&b.name));
    sdk.internal.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    Ok(sdk)
}

/// Plans one entry point.
fn plan_one(function: &Function, owners: &[(&str, &str)], abi: &Abi) -> Result<Member, String> {
    let fail = |what: &str| format!("`{}`: {what}", function.name);

    let namespace = NAMESPACES
        .iter()
        .find(|(prefix, _)| function.name.starts_with(prefix))
        .map(|(_, namespace)| (*namespace).to_string());
    let override_ = OVERRIDES
        .iter()
        .find(|(symbol, _, _)| *symbol == function.name);
    let (mut owner, mut remainder) = owners
        .iter()
        .find(|(prefix, _)| function.name.starts_with(prefix))
        .map_or(
            ("", function.name.trim_start_matches("otio_")),
            |(prefix, owner)| (*owner, &function.name[prefix.len()..]),
        );

    if let Some((_, over, _)) = override_ {
        owner = over;
    }
    if let Some((prefix, _)) = NAMESPACES
        .iter()
        .find(|(prefix, _)| function.name.starts_with(prefix))
    {
        remainder = &function.name[prefix.len()..];
    }

    // The document may be anywhere in the list: it leads the calls that are
    // about an object, and follows the format on the ones that are about a
    // file. Wherever it is, it is not an argument in TypeScript.
    let document = function.parameters.iter().position(|parameter| {
        parameter.kind.pointee().and_then(Type::named) == Some("OtioDocument")
    });

    let node_owner = HIERARCHY.iter().any(|(name, _)| *name == owner);
    let mut receiver = Receiver::None;
    let mut consumed: Vec<usize> = document.into_iter().collect();

    // The object the call is about is the first handle it takes. That is the
    // parameter after the document on almost every call, and `from` rather
    // than the leading time on the two that transform a time between objects.
    let first_node = function.parameters.iter().position(|parameter| {
        parameter.kind == Type::Named("OtioNode".to_string())
            && !function.optional.contains(&parameter.name)
            && !parameter.name.starts_with("out_")
    });

    if document.is_some() {
        if node_owner {
            match first_node {
                Some(index) => {
                    consumed.push(index);
                    receiver = Receiver::Node;
                }
                // `otio_clip_new` belongs to `Clip` by name but takes no clip:
                // it makes one, which needs the arena rather than an object.
                None => receiver = Receiver::Document,
            }
        } else if owner.is_empty() {
            match first_node.or_else(|| {
                function.parameters.iter().position(|parameter| {
                    parameter.kind.pointee().and_then(Type::named) == Some("OtioNode")
                })
            }) {
                // An edit and an algorithm read as free functions, and the
                // document they work on is the one their subject lives in.
                Some(index) => {
                    receiver = Receiver::Borrowed(camel(&function.parameters[index].name));
                }
                None => {
                    owner = "Document";
                    receiver = Receiver::Document;
                }
            }
        } else {
            receiver = Receiver::Document;
        }
    } else if VALUES.contains(&owner)
        && function
            .parameters
            .first()
            .is_some_and(|parameter| parameter.kind.named().map(ts_name).as_deref() == Some(owner))
    {
        consumed.push(0);
        receiver = Receiver::Value(owner.to_string());
    }

    let mutates = document.is_some_and(|index| {
        matches!(
            function.parameters[index].kind,
            Type::Pointer { mutable: true, .. }
        )
    });
    let receiver_at = consumed.get(document.map_or(0, |_| 1)).copied();
    let rest: Vec<_> = function
        .parameters
        .iter()
        .enumerate()
        .filter(|(index, _)| !consumed.contains(index))
        .map(|(_, parameter)| parameter)
        .collect();
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    let mut list = false;
    let mut index = 0;

    while index < rest.len() {
        let parameter = rest[index];
        let name = camel(&parameter.name);
        let optional = function.optional.contains(&parameter.name);

        // `capacity` and `out_count` are the two-pass protocol, not arguments.
        if parameter.name == "capacity" && parameter.kind == Type::Usize {
            list = true;
            index += 1;
            continue;
        }
        if parameter.name == "out_count"
            && function
                .parameters
                .iter()
                .any(|other| other.name == "capacity")
        {
            index += 1;
            continue;
        }

        if let Some(bare) = parameter.name.strip_prefix("out_") {
            outputs.push(plan_output(bare, &parameter.kind, abi).map_err(|why| fail(&why))?);
            index += 1;
            continue;
        }

        // A pointer and the length beside it are one value.
        let next_is_length = rest.get(index + 1).is_some_and(|next| {
            matches!(next.name.as_str(), "len" | "count") && next.kind == Type::Usize
        });
        if next_is_length {
            match &parameter.kind {
                Type::Pointer { inner, .. } if **inner == Type::U8 => {
                    inputs.push(Input::Bytes {
                        name,
                        length: rest[index + 1].name.clone(),
                    });
                    index += 2;
                    continue;
                }
                Type::Pointer { inner, .. } if inner.named() == Some("OtioNode") => {
                    inputs.push(Input::NodeList {
                        name,
                        length: rest[index + 1].name.clone(),
                    });
                    index += 2;
                    continue;
                }
                _ => {}
            }
        }

        inputs.push(plan_input(&name, &parameter.kind, optional, abi).map_err(|why| fail(&why))?);
        index += 1;
    }

    // `otio_metadata_set_vector` takes a bare `len` with no pointer in front
    // of it, which the rule above would have swallowed. Nothing else does, and
    // metadata is handled by hand, so reaching here means a new shape.
    if list && outputs.is_empty() {
        return Err(fail("takes a capacity but produces no list"));
    }

    let constructs = remainder == "new" && receiver == Receiver::Document;
    let statik = constructs || (receiver == Receiver::None && !owner.is_empty());
    let name = match override_ {
        Some((_, _, name)) => (*name).to_string(),
        None => member_name(remainder, owner, &receiver),
    };
    let owner = owner.to_string();

    Ok(Member {
        symbol: function.name.clone(),
        doc: function.doc.clone(),
        owner,
        name,
        namespace,
        receiver,
        constructs,
        mutates,
        receiver_at,
        statik,
        inputs,
        outputs,
        list,
        no_value: function.no_value,
        fallible: function.fallible(),
        returns: function.returns.clone(),
    })
}

/// What a member is called, once its owner's prefix is gone.
fn member_name(remainder: &str, owner: &str, _receiver: &Receiver) -> String {
    let _ = owner;
    camel(remainder)
}

/// Works out what one argument becomes.
fn plan_input(name: &str, kind: &Type, optional: bool, abi: &Abi) -> Result<Input, String> {
    Ok(match kind {
        Type::Bool => Input::Boolean {
            name: name.to_string(),
        },
        Type::F64 | Type::I32 | Type::U32 | Type::I64 | Type::U64 | Type::Usize | Type::U8 => {
            Input::Number {
                name: name.to_string(),
                kind: kind.clone(),
            }
        }
        Type::Named(named) if named == "OtioNode" => Input::Node {
            name: name.to_string(),
            optional,
        },
        Type::Named(named) if abi.enumeration(named).is_some() => Input::Enumeration {
            name: name.to_string(),
            ts: ts_name(named),
        },
        Type::Named(named) => Input::Record {
            name: name.to_string(),
            ts: ts_name(named),
            optional: false,
        },
        Type::Pointer { inner, .. } if **inner == Type::Char => Input::Text {
            name: name.to_string(),
            optional,
        },
        // A struct behind a `*const` is an argument that may be left out: the
        // ABI's rule is that null means "do the usual thing".
        Type::Pointer { inner, .. } if inner.named().is_some_and(|n| abi.record(n).is_some()) => {
            Input::Record {
                name: name.to_string(),
                ts: ts_name(inner.named().unwrap_or_default()),
                optional: true,
            }
        }
        Type::Pointer { inner, .. } if inner.named() == Some("OtioDocument") => Input::Number {
            name: name.to_string(),
            kind: Type::Usize,
        },
        other => {
            return Err(format!(
                "argument `{name}` has type `{other}`, which no rule covers"
            ));
        }
    })
}

/// Works out what one result becomes.
fn plan_output(name: &str, kind: &Type, abi: &Abi) -> Result<Output, String> {
    let pointee = kind
        .pointee()
        .ok_or_else(|| format!("result `out_{name}` is not a pointer"))?;
    Ok(match pointee {
        Type::Bool => Output::Boolean {
            name: name.to_string(),
        },
        Type::F64 | Type::I32 | Type::U32 | Type::I64 | Type::U64 | Type::Usize => Output::Number {
            name: name.to_string(),
            kind: pointee.clone(),
        },
        Type::Named(named) if named == "OtioNode" => Output::Node {
            name: name.to_string(),
        },
        Type::Named(named) if named == "OtioBuffer" => {
            // The only buffer that is not text is the one a written file comes
            // back in.
            if name == "bytes" {
                Output::Bytes {
                    name: name.to_string(),
                }
            } else {
                Output::Text {
                    name: name.to_string(),
                }
            }
        }
        Type::Named(named) if abi.enumeration(named).is_some() => Output::Enumeration {
            name: name.to_string(),
            ts: ts_name(named),
        },
        Type::Named(named) if abi.record(named).is_some_and(|r| !r.fields.is_empty()) => {
            Output::Record {
                name: name.to_string(),
                ts: ts_name(named),
            }
        }
        Type::Pointer { inner, .. } if inner.named() == Some("OtioDocument") => Output::Document {
            name: name.to_string(),
        },
        other => {
            return Err(format!(
                "result `out_{name}` points at `{other}`, which no rule covers"
            ));
        }
    })
}

/// A property: a getter, its setter, and the call that clears it.
///
/// `otio_item_source_range`, `otio_item_set_source_range` and
/// `otio_item_clear_source_range` are one idea in C's spelling and three calls
/// only because C has no properties. Together they become `item.sourceRange`,
/// where assigning `undefined` is what calls the third one.
#[derive(Debug, Clone)]
pub struct Property {
    /// The property's name in TypeScript.
    pub name: String,
    /// The call that reads it.
    pub getter: Member,
    /// The call that writes it, if there is one.
    pub setter: Option<Member>,
    /// The call that unsets it, if there is one.
    pub clear: Option<Member>,
}

/// One TypeScript class.
#[derive(Debug, Clone, Default)]
pub struct Class {
    /// Its properties.
    pub properties: Vec<Property>,
    /// Its instance methods.
    pub methods: Vec<Member>,
    /// Its static members.
    pub statics: Vec<Member>,
}

/// Groups a class's members into properties, methods and statics.
fn group(members: Vec<Member>) -> Class {
    let by_name: BTreeMap<String, Member> = members
        .iter()
        .map(|member| (member.name.clone(), member.clone()))
        .collect();

    let mut class = Class::default();
    let mut consumed: BTreeSet<String> = BTreeSet::new();

    for member in &members {
        if consumed.contains(&member.name) || member.statik || member.receiver == Receiver::None {
            continue;
        }
        // A getter takes nothing and answers with exactly one thing.
        if !member.inputs.is_empty() || member.outputs.len() != 1 || member.list {
            continue;
        }
        let capitalised = capitalise(&member.name);
        let Some(setter) = by_name.get(&format!("set{capitalised}")) else {
            continue;
        };
        // A setter takes exactly the thing the getter answers with, and
        // answers with nothing itself.
        if setter.inputs.len() != 1
            || !setter.outputs.is_empty()
            || !same_shape(&member.outputs[0], &setter.inputs[0])
        {
            continue;
        }
        let clear = by_name
            .get(&format!("clear{capitalised}"))
            .filter(|clear| member.no_value && clear.inputs.is_empty() && clear.outputs.is_empty());

        consumed.insert(member.name.clone());
        consumed.insert(setter.name.clone());
        if let Some(clear) = clear {
            consumed.insert(clear.name.clone());
        }
        class.properties.push(Property {
            name: member.name.clone(),
            getter: member.clone(),
            setter: Some(setter.clone()),
            clear: clear.cloned(),
        });
    }

    for member in members {
        if consumed.contains(&member.name) {
            continue;
        }
        if member.statik {
            class.statics.push(member);
        } else {
            class.methods.push(member);
        }
    }

    class.properties.sort_by(|a, b| a.name.cmp(&b.name));
    class.methods.sort_by(|a, b| a.name.cmp(&b.name));
    class.statics.sort_by(|a, b| a.name.cmp(&b.name));
    class
}

/// Whether a result and an argument are the same TypeScript type.
fn same_shape(output: &Output, input: &Input) -> bool {
    match (output, input) {
        (Output::Boolean { .. }, Input::Boolean { .. }) => true,
        (Output::Number { kind: left, .. }, Input::Number { kind: right, .. }) => left == right,
        (Output::Node { .. }, Input::Node { .. }) => true,
        (Output::Text { .. }, Input::Text { .. }) => true,
        (Output::Record { ts: left, .. }, Input::Record { ts: right, .. })
        | (Output::Enumeration { ts: left, .. }, Input::Enumeration { ts: right, .. }) => {
            left == right
        }
        _ => false,
    }
}

/// Capitalises the first letter of a `camelCase` name.
fn capitalise(name: &str) -> String {
    let mut characters = name.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}
