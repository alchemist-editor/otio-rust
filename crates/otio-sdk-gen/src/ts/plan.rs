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

use otio_sdk_model::{Api, CResult, Docs, Function, Param, ParamRole, Placement, Type};

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
    },
    /// A handle to an object.
    Node {
        /// The TypeScript parameter name.
        name: String,
        /// Whether the none handle is accepted.
        optional: bool,
        /// What the call does with it.
        placement: Placement,
    },
    /// An array of handles, spelled in C as a pointer and a count.
    NodeList {
        /// The TypeScript parameter name.
        name: String,
        /// What the call does with them.
        placement: Placement,
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
    /// What the C entry point itself returns, which for most calls is a
    /// status and for the rest is the value.
    pub returns: CResult,
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
pub fn plan(api: &Api) -> Result<Sdk, String> {
    let reserved = handled_elsewhere();
    let owners = owners();
    let mut sdk = Sdk {
        handled_elsewhere: reserved.clone(),
        ..Sdk::default()
    };
    let mut collected: BTreeMap<String, Vec<Member>> = BTreeMap::new();

    for function in api.functions() {
        // Consuming the source document makes this one unlike every other
        // call, and the two-pass protocol the emitter would use for its list
        // cannot work when the first pass destroys the thing being asked
        // about. The document layer marshals it by hand.
        if function.symbol == "otio_document_absorb" {
            continue;
        }
        // The one struct the runtime passes by value is a buffer, and it does
        // so where it reads one: `readBuffer` frees what it has just copied,
        // so `OtioBuffer` never has to appear in the generated types at all.
        if function.symbol == "otio_buffer_free" {
            continue;
        }
        if reserved.contains_key(&function.symbol) {
            sdk.internal.push(plan_one(function, &owners, api)?);
            continue;
        }
        // Metadata is a tree of free-form values addressed by path. Forty
        // typed getters and setters is the right shape for C and the wrong one
        // for TypeScript, which wants one `get` that reads the kind and hands
        // back a JavaScript value. That wrapper is written by hand over the
        // generated low-level calls.
        if function.symbol.starts_with("otio_metadata_") {
            sdk.handled_elsewhere.insert(
                function.symbol.clone(),
                "`Metadata`, one `get` and one `set` over the typed calls".to_string(),
            );
            sdk.internal.push(plan_one(function, &owners, api)?);
            continue;
        }

        let member = plan_one(function, &owners, api)?;
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

/// A doc comment as the emitters want it: paragraphs, summary first.
///
/// The description keeps a comment as a summary and a body rather than as
/// the lines the Rust source happened to wrap them into, so this is where a
/// paragraph stops being one string and the emitter decides where to break
/// it.
pub fn paragraphs(docs: &Docs) -> Vec<String> {
    if docs.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(1 + docs.body.len());
    if !docs.summary.is_empty() {
        out.push(docs.summary.clone());
    }
    out.extend(docs.body.iter().cloned());
    out
}

/// Plans one entry point.
fn plan_one(function: &Function, owners: &[(&str, &str)], api: &Api) -> Result<Member, String> {
    let symbol = function.symbol.as_str();
    let fail = |what: &str| format!("`{symbol}`: {what}");

    let namespace = NAMESPACES
        .iter()
        .find(|(prefix, _)| symbol.starts_with(prefix))
        .map(|(_, namespace)| (*namespace).to_string());
    let override_ = OVERRIDES.iter().find(|(name, _, _)| *name == symbol);
    let (mut owner, mut remainder) = owners
        .iter()
        .find(|(prefix, _)| symbol.starts_with(prefix))
        .map_or(
            ("", symbol.trim_start_matches("otio_")),
            |(prefix, owner)| (*owner, &symbol[prefix.len()..]),
        );

    if let Some((_, over, _)) = override_ {
        owner = over;
    }
    if let Some((prefix, _)) = NAMESPACES
        .iter()
        .find(|(prefix, _)| symbol.starts_with(prefix))
    {
        remainder = &symbol[prefix.len()..];
    }

    // The document may be anywhere in the list: it leads the calls that are
    // about an object, and follows the format on the ones that are about a
    // file. Wherever it is, it is not an argument in TypeScript.
    let document = function.params.iter().position(|param| {
        matches!(
            param.role,
            ParamRole::DocumentIn | ParamRole::DocumentMut | ParamRole::DocumentTaken
        )
    });

    let node_owner = HIERARCHY.iter().any(|(name, _)| *name == owner);
    let mut receiver = Receiver::None;
    let mut consumed: Vec<usize> = document.into_iter().collect();

    // Which object's document the call is made in is the description's
    // answer, not this backend's: the same question decides the same way in
    // every binding that hides the document. It is the receiver where there
    // is one, and otherwise the object the call cannot move — which is not
    // the first object it takes. `otio_edit_insert` adopts its `item` and
    // requires its `composition`, so it happens where the composition is.
    let anchor = function.params.iter().position(|param| param.anchor);

    if document.is_some() {
        if node_owner {
            match anchor {
                Some(index) => {
                    consumed.push(index);
                    receiver = Receiver::Node;
                }
                // `otio_clip_new` belongs to `Clip` by name but takes no clip:
                // it makes one, which needs the arena rather than an object.
                None => receiver = Receiver::Document,
            }
        } else if owner.is_empty() {
            match anchor {
                // An edit and an algorithm read as free functions, and the
                // document they work on is the one their subject lives in.
                Some(index) => {
                    receiver = Receiver::Borrowed(camel(&function.params[index].name));
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
        && function.params.first().is_some_and(
            |param| matches!(&param.ty, Type::Struct(named) if ts_name(named) == owner),
        )
    {
        consumed.push(0);
        receiver = Receiver::Value(owner.to_string());
    }

    let receiver_at = consumed.get(document.map_or(0, |_| 1)).copied();
    let rest: Vec<_> = function
        .params
        .iter()
        .enumerate()
        .filter(|(index, _)| !consumed.contains(index))
        .map(|(_, param)| param)
        .collect();
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    let list = function
        .params
        .iter()
        .any(|param| param.role == ParamRole::ListCapacity);

    for param in rest {
        let name = camel(&param.name);
        match param.role {
            // The two-pass protocol and the length beside a borrowed run are
            // spelling, not arguments.
            ParamRole::Length | ParamRole::ListCapacity | ParamRole::OutputCount => continue,
            // Where a failing call writes why. It is not an argument or a
            // result in TypeScript: the emitter passes a slot of its own on
            // every call and the message arrives on the thrown `OtioError`.
            ParamRole::Error => continue,
            ParamRole::Output | ParamRole::OutputList => {
                outputs.push(plan_output(param, api).map_err(|why| fail(&why))?);
                continue;
            }
            _ => {}
        }

        inputs.push(plan_input(&name, param).map_err(|why| fail(&why))?);
    }

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
        symbol: symbol.to_string(),
        doc: paragraphs(&function.docs),
        owner,
        name,
        namespace,
        receiver,
        constructs,
        receiver_at,
        statik,
        inputs,
        outputs,
        list,
        no_value: function.optional,
        fallible: function.fallible(),
        returns: function.result.clone(),
    })
}

/// What a member is called, once its owner's prefix is gone.
fn member_name(remainder: &str, owner: &str, _receiver: &Receiver) -> String {
    let _ = owner;
    camel(remainder)
}

/// Works out what one argument becomes.
fn plan_input(name: &str, param: &Param) -> Result<Input, String> {
    let named = || name.to_string();
    // Whether an object argument moves into the receiver's document, or has
    // to be there already, is the description's answer and not this
    // backend's: the same question decides the same way in Go and in Swift.
    let placement = || {
        param.placement.ok_or_else(|| {
            format!(
                "argument `{name}` is an object and the description says nothing about what the \
                 call does with it"
            )
        })
    };
    Ok(match &param.ty {
        Type::Bool => Input::Boolean { name: named() },
        Type::Double | Type::Int32 | Type::Uint32 | Type::Int64 | Type::Uint64 | Type::Size => {
            Input::Number {
                name: named(),
                kind: param.ty.clone(),
            }
        }
        Type::Node => Input::Node {
            name: named(),
            optional: param.optional,
            placement: placement()?,
        },
        Type::List(inner) if **inner == Type::Node => Input::NodeList {
            name: named(),
            placement: placement()?,
        },
        Type::Bytes => Input::Bytes { name: named() },
        Type::Text => Input::Text {
            name: named(),
            optional: param.optional,
        },
        Type::Enum(name) => Input::Enumeration {
            name: named(),
            ts: ts_name(name),
        },
        // A struct behind a `*const` is an argument that may be left out: the
        // ABI's rule is that null means "do the usual thing".
        Type::Struct(name) => Input::Record {
            name: named(),
            ts: ts_name(name),
            optional: param.optional,
        },
        // The document reaches the module as the number its pointer is.
        Type::Document => Input::Number {
            name: named(),
            kind: Type::Size,
        },
        other => {
            return Err(format!(
                "argument `{name}` has type `{}`, which no rule covers",
                other.c_name()
            ));
        }
    })
}

/// Works out what one result becomes.
fn plan_output(param: &Param, api: &Api) -> Result<Output, String> {
    let name = param
        .name
        .strip_prefix("out_")
        .unwrap_or(&param.name)
        .to_string();
    let inner = match &param.ty {
        Type::List(inner) => inner.as_ref(),
        other => other,
    };
    Ok(match inner {
        Type::Bool => Output::Boolean { name },
        Type::Double | Type::Int32 | Type::Uint32 | Type::Int64 | Type::Uint64 | Type::Size => {
            Output::Number {
                name,
                kind: inner.clone(),
            }
        }
        Type::Node => Output::Node { name },
        // The only buffer that is not text is the one a written file comes
        // back in.
        Type::Bytes => Output::Bytes { name },
        Type::Text => {
            if name == "bytes" {
                Output::Bytes { name }
            } else {
                Output::Text { name }
            }
        }
        Type::Enum(named) => Output::Enumeration {
            name,
            ts: ts_name(named),
        },
        Type::Struct(named) if api.structure(named).is_some_and(|r| !r.fields.is_empty()) => {
            Output::Record {
                name,
                ts: ts_name(named),
            }
        }
        Type::Document => Output::Document { name },
        other => {
            return Err(format!(
                "result `{name}` points at `{}`, which no rule covers",
                other.c_name()
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
