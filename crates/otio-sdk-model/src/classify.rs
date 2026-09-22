//! Turning what the scanner read into what the generators need.
//!
//! The C ABI is written to a small set of conventions, and those conventions
//! carry real meaning: `otio_clip_new` builds a clip, the `parent` in
//! `otio_composition_append_child` is what the call is *about*, an
//! `out_nodes`/`capacity`/`out_count` run of three parameters is a list, an
//! `OtioBuffer` is a string the caller must free. This module reads those
//! conventions back out.
//!
//! Where it cannot, it stops. A function that fits no rule here is an error
//! naming the function, not a function quietly dropped from every SDK — which
//! is the whole reason this is a pipeline rather than six hand-written
//! bindings.

use crate::header;
use crate::layout;
use crate::model::{
    Api, ByWidth, CResult, Docs, Enum, Field, Function, Group, Layout, Output, Param, ParamRole,
    Receiver, Role, Struct, Type, Variant,
};
use crate::names;
use crate::overrides;
use crate::scan::{RawFunction, RawParam, ScanError, Scanned, Source};
use crate::schema;

/// Stands in for a size, an alignment or an offset until `layout::apply`
/// works the real one out.
const ZERO: ByWidth = ByWidth {
    pointer32: 0,
    pointer64: 0,
};

/// What a group of entry points is a method on.
#[derive(Debug, Clone, Copy)]
enum ReceiverSpec {
    /// Nothing: free functions, which keep their prefix in their name because
    /// there is no receiver to carry it.
    Free,
    /// The document.
    Document,
    /// An object of the named schema, or one deriving from it.
    Node(&'static str),
    /// One of the value types, struct or enum.
    Value(&'static str),
}

/// One row of the convention table: a symbol prefix and where it belongs.
struct GroupSpec {
    prefix: &'static str,
    group: &'static str,
    receiver: ReceiverSpec,
    /// Whether the group is a view of its receiver rather than part of it.
    view: bool,
    /// Whether the prefix stays in the function's name.
    ///
    /// A method drops it, because its receiver already says what it is about:
    /// `otio_clip_media_reference` becomes `media_reference` on a `Clip`. A
    /// call with nothing to hang off keeps it, because `from_file` on its own
    /// says neither what it reads nor that it reads at all.
    keep_prefix: bool,
    docs: &'static str,
}

/// Every `otio_` prefix the C ABI uses, and the type it belongs to.
///
/// The longest matching prefix wins, so `image_sequence_reference` is found
/// before `image`. A symbol matching no row is an error: the ABI grew
/// something nobody has placed yet.
const GROUPS: &[GroupSpec] = &[
    GroupSpec {
        prefix: "rational_time",
        group: "RationalTime",
        receiver: ReceiverSpec::Value("OtioRationalTime"),
        view: false,
        keep_prefix: false,
        docs: "A point in time, or a length of it, as a value over a rate.",
    },
    GroupSpec {
        prefix: "time_range",
        group: "TimeRange",
        receiver: ReceiverSpec::Value("OtioTimeRange"),
        view: false,
        keep_prefix: false,
        docs: "A span of time: where it starts and how long it lasts.",
    },
    GroupSpec {
        prefix: "time_transform",
        group: "TimeTransform",
        receiver: ReceiverSpec::Value("OtioTimeTransform"),
        view: false,
        keep_prefix: false,
        docs: "An offset and a scale, applied to a time or a range.",
    },
    GroupSpec {
        prefix: "image_sequence_reference",
        group: "ImageSequenceReference",
        receiver: ReceiverSpec::Node("ImageSequenceReference"),
        view: false,
        keep_prefix: false,
        docs: "Media held as numbered frames on disk.",
    },
    GroupSpec {
        prefix: "external_reference",
        group: "ExternalReference",
        receiver: ReceiverSpec::Node("ExternalReference"),
        view: false,
        keep_prefix: false,
        docs: "Media at a URL.",
    },
    GroupSpec {
        prefix: "generator_reference",
        group: "GeneratorReference",
        receiver: ReceiverSpec::Node("GeneratorReference"),
        view: false,
        keep_prefix: false,
        docs: "Media a generator produces, such as colour bars.",
    },
    GroupSpec {
        prefix: "missing_reference",
        group: "MissingReference",
        receiver: ReceiverSpec::Node("MissingReference"),
        view: false,
        keep_prefix: false,
        docs: "Media whose location is not known.",
    },
    GroupSpec {
        prefix: "media_reference",
        group: "MediaReference",
        receiver: ReceiverSpec::Node("MediaReference"),
        view: false,
        keep_prefix: false,
        docs: "What a clip draws its pictures from.",
    },
    GroupSpec {
        prefix: "serializable_collection",
        group: "SerializableCollection",
        receiver: ReceiverSpec::Node("SerializableCollection"),
        view: false,
        keep_prefix: false,
        docs: "A bag of objects, held together for writing to a file.",
    },
    GroupSpec {
        prefix: "linear_time_warp",
        group: "LinearTimeWarp",
        receiver: ReceiverSpec::Node("LinearTimeWarp"),
        view: false,
        keep_prefix: false,
        docs: "A constant-rate speed change.",
    },
    GroupSpec {
        prefix: "freeze_frame",
        group: "FreezeFrame",
        receiver: ReceiverSpec::Node("FreezeFrame"),
        view: false,
        keep_prefix: false,
        docs: "A hold on a single frame.",
    },
    GroupSpec {
        prefix: "time_effect",
        group: "TimeEffect",
        receiver: ReceiverSpec::Node("TimeEffect"),
        view: false,
        keep_prefix: false,
        docs: "An effect that alters timing.",
    },
    GroupSpec {
        prefix: "composition",
        group: "Composition",
        receiver: ReceiverSpec::Node("Composition"),
        view: false,
        keep_prefix: false,
        docs: "An item that holds other items and says where they sit.",
    },
    GroupSpec {
        prefix: "composable",
        group: "Composable",
        receiver: ReceiverSpec::Node("Composable"),
        view: false,
        keep_prefix: false,
        docs: "Anything that can sit inside a composition.",
    },
    GroupSpec {
        prefix: "transition",
        group: "Transition",
        receiver: ReceiverSpec::Node("Transition"),
        view: false,
        keep_prefix: false,
        docs: "A dissolve or a wipe between what comes before and after it.",
    },
    GroupSpec {
        prefix: "timeline",
        group: "Timeline",
        receiver: ReceiverSpec::Node("Timeline"),
        view: false,
        keep_prefix: false,
        docs: "A whole edit: a stack of tracks and where its clock starts.",
    },
    GroupSpec {
        prefix: "metadata",
        group: "Metadata",
        receiver: ReceiverSpec::Node("SerializableObjectWithMetadata"),
        view: true,
        keep_prefix: false,
        docs: "The free-form dictionary every named object carries.",
    },
    GroupSpec {
        prefix: "document",
        group: "Document",
        receiver: ReceiverSpec::Document,
        view: false,
        keep_prefix: false,
        docs: "The arena that owns every object in a timeline.",
    },
    GroupSpec {
        prefix: "algorithm",
        group: "Algorithm",
        receiver: ReceiverSpec::Document,
        view: false,
        keep_prefix: false,
        docs: "Operations that build a new object out of existing ones.",
    },
    GroupSpec {
        prefix: "marker",
        group: "Marker",
        receiver: ReceiverSpec::Node("Marker"),
        view: false,
        keep_prefix: false,
        docs: "A note pinned to a span of an item.",
    },
    GroupSpec {
        prefix: "effect",
        group: "Effect",
        receiver: ReceiverSpec::Node("Effect"),
        view: false,
        keep_prefix: false,
        docs: "Something applied to an item that changes how it plays.",
    },
    GroupSpec {
        prefix: "transition",
        group: "Transition",
        receiver: ReceiverSpec::Node("Transition"),
        view: false,
        keep_prefix: false,
        docs: "A dissolve or a wipe.",
    },
    GroupSpec {
        prefix: "track",
        group: "Track",
        receiver: ReceiverSpec::Node("Track"),
        view: false,
        keep_prefix: false,
        docs: "A composition that lays its children end to end.",
    },
    GroupSpec {
        prefix: "stack",
        group: "Stack",
        receiver: ReceiverSpec::Node("Stack"),
        view: false,
        keep_prefix: false,
        docs: "A composition that starts all its children together.",
    },
    GroupSpec {
        prefix: "item",
        group: "Item",
        receiver: ReceiverSpec::Node("Item"),
        view: false,
        keep_prefix: false,
        docs: "Anything that occupies time.",
    },
    GroupSpec {
        prefix: "clip",
        group: "Clip",
        receiver: ReceiverSpec::Node("Clip"),
        view: false,
        keep_prefix: false,
        docs: "A piece of media, trimmed to the part an edit uses.",
    },
    GroupSpec {
        prefix: "gap",
        group: "Gap",
        receiver: ReceiverSpec::Node("Gap"),
        view: false,
        keep_prefix: false,
        docs: "Time that holds nothing.",
    },
    GroupSpec {
        prefix: "node",
        group: "Node",
        receiver: ReceiverSpec::Node("SerializableObject"),
        view: false,
        keep_prefix: false,
        docs: "What every object in a document can do.",
    },
    GroupSpec {
        prefix: "edit",
        group: "Edit",
        receiver: ReceiverSpec::Document,
        view: false,
        keep_prefix: false,
        docs: "The ten edit operations, as an editor's tools spell them.",
    },
    GroupSpec {
        prefix: "format",
        group: "Format",
        receiver: ReceiverSpec::Value("OtioFormat"),
        view: false,
        keep_prefix: false,
        docs: "The file formats this library reads and writes.",
    },
    GroupSpec {
        prefix: "read",
        group: "Adapter",
        receiver: ReceiverSpec::Document,
        view: false,
        keep_prefix: true,
        docs: "Reading and writing the interchange formats.",
    },
    GroupSpec {
        prefix: "write",
        group: "Adapter",
        receiver: ReceiverSpec::Document,
        view: false,
        keep_prefix: true,
        docs: "Reading and writing the interchange formats.",
    },
    GroupSpec {
        prefix: "is",
        group: "Rate",
        receiver: ReceiverSpec::Free,
        view: false,
        keep_prefix: true,
        docs: "What the SMPTE timecode rates are, and which of them drop frames.",
    },
    GroupSpec {
        prefix: "nearest",
        group: "Rate",
        receiver: ReceiverSpec::Free,
        view: false,
        keep_prefix: true,
        docs: "What the SMPTE timecode rates are, and which of them drop frames.",
    },
    GroupSpec {
        prefix: "default",
        group: "Library",
        receiver: ReceiverSpec::Free,
        view: false,
        keep_prefix: true,
        docs: "The library itself: its version, and the defaults it uses.",
    },
    GroupSpec {
        prefix: "version",
        group: "Library",
        receiver: ReceiverSpec::Free,
        view: false,
        keep_prefix: true,
        docs: "The library itself: its version, and the defaults it uses.",
    },
    GroupSpec {
        prefix: "status",
        group: "Library",
        receiver: ReceiverSpec::Free,
        view: false,
        keep_prefix: true,
        docs: "The library itself: its version, and the defaults it uses.",
    },
    GroupSpec {
        prefix: "error",
        group: "Library",
        receiver: ReceiverSpec::Free,
        view: false,
        keep_prefix: true,
        docs: "The library itself: its version, and the defaults it uses.",
    },
    GroupSpec {
        prefix: "buffer",
        group: "Library",
        receiver: ReceiverSpec::Free,
        view: false,
        keep_prefix: true,
        docs: "The library itself: its version, and the defaults it uses.",
    },
];

/// The entry points a generated SDK calls but never shows.
///
/// Freeing a buffer and reading the last error message are how a binding is
/// written, not something the people using one should ever have to think
/// about. A backend reaches for these by name; they belong to no type.
const PLUMBING: &[&str] = &[
    "otio_buffer_free",
    "otio_error_message",
    "otio_status_name",
    "otio_document_free",
];

/// What sizes the answer of a list call that edits as it answers.
///
/// A two-pass list call asks how many there are, then asks again for them.
/// `otio_composition_clear_children` cannot be asked twice — the second time
/// there is nothing left — so its buffer has to be right the first time, and
/// this says what to ask instead. Every such call must be here; one that is
/// not stops the build, because the alternative is an SDK that silently loses
/// the handles it was told to hand back.
const SIZED_BY: &[(&str, &str)] = &[
    ("otio_composition_clear_children", "otio_node_child_count"),
    ("otio_document_absorb", "otio_document_node_count"),
];

/// The structs a generated SDK uses but never shows.
const PLUMBING_STRUCTS: &[&str] = &["OtioBuffer"];

/// Builds the description from what the scanner read.
///
/// # Errors
///
/// Fails if any entry point, enum or struct fits none of the conventions the
/// C ABI is written to.
pub fn api(source: &Source, header_text: &str, version: &str) -> Scanned<Api> {
    let enums = enums(source, header_text)?;
    let structs = structs(source)?;

    let node_kind = enums
        .iter()
        .find(|item| item.name == "OtioNodeKind")
        .ok_or_else(|| ScanError {
            location: "crates/otio-capi/src".to_string(),
            message: "the C ABI no longer declares `OtioNodeKind`".to_string(),
        })?;
    let schema = schema::ladder(node_kind)?;

    let known: Vec<&str> = enums
        .iter()
        .map(|item| item.name.as_str())
        .chain(structs.iter().map(|item| item.name.as_str()))
        .collect();

    let mut groups: Vec<Group> = Vec::new();
    for raw in &source.functions {
        let spec = spec_for(&raw.name)?;
        let function = function(raw, spec, &known)?;
        if let Some(group) = groups.iter_mut().find(|group| group.name == spec.group) {
            if !group.prefixes.iter().any(|prefix| prefix == spec.prefix) {
                group.prefixes.push(spec.prefix.to_string());
            }
            group.functions.push(function);
        } else {
            groups.push(Group {
                name: spec.group.to_string(),
                prefixes: vec![spec.prefix.to_string()],
                receiver: match spec.receiver {
                    ReceiverSpec::Free => Receiver::None,
                    ReceiverSpec::Document => Receiver::Document,
                    ReceiverSpec::Node(name) => Receiver::Node(name.to_string()),
                    ReceiverSpec::Value(name) => Receiver::Value(name.to_string()),
                },
                view: spec.view,
                docs: Docs {
                    summary: spec.docs.to_string(),
                    ..Docs::default()
                },
                functions: vec![function],
            });
        }
    }

    for group in &mut groups {
        group.functions.sort_by(|left, right| {
            left.symbol
                .cmp(&right.symbol)
                .then_with(|| left.name.cmp(&right.name))
        });
        group.prefixes.sort_unstable();
    }
    groups.sort_by(|left, right| left.name.cmp(&right.name));

    let enum_names: Vec<&str> = enums.iter().map(|item| item.name.as_str()).collect();
    for group in &mut groups {
        for function in &mut group.functions {
            for param in &mut function.params {
                name_enums(&mut param.ty, &enum_names);
            }
            for output in &mut function.outputs {
                name_enums(&mut output.ty, &enum_names);
            }
            if let CResult::Value(ty) = &mut function.result {
                name_enums(ty, &enum_names);
            }
        }
    }
    let mut structs = structs;
    for item in &mut structs {
        for field in &mut item.fields {
            name_enums(&mut field.ty, &enum_names);
        }
    }

    // Only now can a field holding an enum be told from one holding a
    // struct, and the two are different sizes.
    let trouble = |message: String| ScanError {
        location: "crates/otio-capi/src".to_string(),
        message,
    };
    layout::apply(&mut structs).map_err(trouble)?;
    layout::check(&structs, &source.sizes).map_err(trouble)?;
    let structs = structs;

    overrides::check(&groups)?;
    let stale: Vec<&str> = SIZED_BY
        .iter()
        .filter(|(symbol, sizer)| {
            let exists = |name: &str| {
                groups.iter().any(|group| {
                    group
                        .functions
                        .iter()
                        .any(|function| function.symbol == name)
                })
            };
            !exists(symbol) || !exists(sizer)
        })
        .map(|(symbol, _)| *symbol)
        .collect();
    if !stale.is_empty() {
        return Err(ScanError {
            location: "crates/otio-sdk-model/src/classify.rs".to_string(),
            message: format!("`SIZED_BY` names calls the C ABI no longer exports: {stale:?}"),
        });
    }
    collisions(&groups)?;

    Ok(Api {
        version: version.to_string(),
        enums,
        structs,
        schema,
        groups,
    })
}

/// Checks that no two calls that land on one type end up sharing a name.
///
/// Every group with a document receiver becomes one type in a generated SDK,
/// and a node group's calls reach every schema deriving from it, so two
/// functions can meet that never look adjacent in the C ABI: `otio_edit_fill`
/// and `otio_item_fill` would both be `fill` on a timeline's document.
///
/// Constructors are exempt, because every language spells them with the type
/// they build; so are views, which are reached through an object of their own.
fn collisions(groups: &[Group]) -> Scanned<()> {
    let mut clashes: Vec<String> = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        for other in groups.iter().skip(index + 1) {
            if !shares_a_surface(group, other) {
                continue;
            }
            for left in &group.functions {
                for right in &other.functions {
                    if left.name != right.name
                        || left.role == Role::Constructor
                        || right.role == Role::Constructor
                        || left.role == Role::Plumbing
                        || right.role == Role::Plumbing
                    {
                        continue;
                    }
                    clashes.push(format!(
                        "`{}` and `{}` would both be `{}` on {}",
                        left.symbol, right.symbol, left.name, group.name
                    ));
                }
            }
        }
    }
    if clashes.is_empty() {
        return Ok(());
    }
    Err(ScanError {
        location: "crates/otio-sdk-model/src/overrides.rs".to_string(),
        message: format!(
            "these calls collide once they reach one type:\n  {}\n\nGive one of each pair \
             another name in `overrides.rs`.",
            clashes.join("\n  ")
        ),
    })
}

/// Whether two groups' calls reach the same type in a generated SDK.
fn shares_a_surface(left: &Group, right: &Group) -> bool {
    if left.view || right.view {
        return false;
    }
    match (&left.receiver, &right.receiver) {
        (Receiver::Document, Receiver::Document) => true,
        (Receiver::Node(one), Receiver::Node(other)) => {
            derives_from(one, other) || derives_from(other, one)
        }
        (Receiver::Value(one), Receiver::Value(other)) => one == other,
        _ => false,
    }
}

/// Whether one schema is the other, or derives from it.
fn derives_from(schema: &str, ancestor: &str) -> bool {
    let mut current = Some(schema);
    while let Some(step) = current {
        if step == ancestor {
            return true;
        }
        current = schema::parent_of(step);
    }
    false
}

/// Marks the named types that are enums as enums.
///
/// Both spell their C type the same way, so the scanner cannot tell them
/// apart; the list of enums can.
fn name_enums(ty: &mut Type, enums: &[&str]) {
    match ty {
        Type::Struct(name) if enums.contains(&name.as_str()) => {
            *ty = Type::Enum(std::mem::take(name));
        }
        Type::List(inner) => name_enums(inner, enums),
        _ => {}
    }
}

/// Finds the convention row a symbol belongs to.
fn spec_for(symbol: &str) -> Scanned<&'static GroupSpec> {
    let body = symbol.strip_prefix("otio_").ok_or_else(|| ScanError {
        location: "crates/otio-capi/src".to_string(),
        message: format!("the exported symbol `{symbol}` does not start with `otio_`"),
    })?;

    GROUPS
        .iter()
        .filter(|spec| body == spec.prefix || body.starts_with(&format!("{}_", spec.prefix)))
        .max_by_key(|spec| spec.prefix.len())
        .ok_or_else(|| ScanError {
            location: "crates/otio-sdk-model/src/classify.rs".to_string(),
            message: format!(
                "`{symbol}` belongs to no group. Add a row to `GROUPS` saying which type it is a \
                 method on, and every SDK will carry it."
            ),
        })
}

/// Reads the enums, taking the C spelling of each constant from the header.
fn enums(source: &Source, header_text: &str) -> Scanned<Vec<Enum>> {
    let declared = header::enum_constants(header_text)?;
    let mut built = Vec::new();

    for raw in &source.enums {
        let constants = declared.get(&raw.name).ok_or_else(|| ScanError {
            location: "crates/otio-capi/include/otio.h".to_string(),
            message: format!("the header does not declare the enum `{}`", raw.name),
        })?;
        if constants.len() != raw.variants.len() {
            return Err(ScanError {
                location: "crates/otio-capi/include/otio.h".to_string(),
                message: format!(
                    "the header gives `{}` {} constants and the Rust source gives it {}",
                    raw.name,
                    constants.len(),
                    raw.variants.len()
                ),
            });
        }
        let mut variants = Vec::new();
        for (variant, (c_name, value)) in raw.variants.iter().zip(constants) {
            if *value != variant.value {
                return Err(ScanError {
                    location: "crates/otio-capi/include/otio.h".to_string(),
                    message: format!(
                        "`{c_name}` is {value} in the header and {} in the Rust source",
                        variant.value
                    ),
                });
            }
            variants.push(Variant {
                name: variant.name.clone(),
                c_name: c_name.clone(),
                value: *value,
                docs: docs(&variant.docs),
            });
        }
        built.push(Enum {
            name: raw.name.clone(),
            docs: docs(&raw.docs),
            variants,
        });
    }
    built.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(built)
}

/// Reads the value structs.
fn structs(source: &Source) -> Scanned<Vec<Struct>> {
    let known: Vec<&str> = source
        .enums
        .iter()
        .map(|item| item.name.as_str())
        .chain(source.structs.iter().map(|item| item.name.as_str()))
        .collect();

    let mut built = Vec::new();
    for raw in &source.structs {
        let mut fields = Vec::new();
        for field in &raw.fields {
            fields.push(Field {
                name: field.name.clone(),
                ty: value_type(&field.rust_type, &known).ok_or_else(|| ScanError {
                    location: "crates/otio-capi/src".to_string(),
                    message: format!(
                        "the field `{}` of `{}` has the type `{}`, which crosses the boundary in \
                         no way this knows about",
                        field.name, raw.name, field.rust_type
                    ),
                })?,
                offset: ZERO,
                docs: docs(&field.docs),
            });
        }
        built.push(Struct {
            name: raw.name.clone(),
            docs: docs(&raw.docs),
            fields,
            // Filled in by `layout::apply` once every struct is gathered,
            // since one may be measured in terms of another.
            layout: Layout {
                size: ZERO,
                align: ZERO,
            },
            plumbing: PLUMBING_STRUCTS.contains(&raw.name.as_str()),
        });
    }
    built.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(built)
}

/// Reads a type that crosses the boundary by value.
///
/// `known` holds every named type, and `enums` the subset of them that are
/// enums, so that a backend can tell a value it may compute with from one it
/// must switch on.
fn value_type(rust: &str, known: &[&str]) -> Option<Type> {
    Some(match rust {
        "bool" => Type::Bool,
        "f64" => Type::Double,
        "i64" => Type::Int64,
        "u64" => Type::Uint64,
        "i32" => Type::Int32,
        "u32" => Type::Uint32,
        "usize" => Type::Size,
        "*const c_char" | "*mut c_char" => Type::Text,
        "OtioNode" => Type::Node,
        other => {
            if known.contains(&other) {
                Type::Struct(other.to_string())
            } else {
                return None;
            }
        }
    })
}

/// Reads one entry point.
fn function(raw: &RawFunction, spec: &GroupSpec, known: &[&str]) -> Scanned<Function> {
    let mut params = params(raw, spec, known, overrides::detaches(&raw.name))?;
    let result = result(raw, known)?;
    let outputs = outputs(&params, &result);

    let body = raw.name.strip_prefix("otio_").unwrap_or(&raw.name);
    let name = if spec.keep_prefix {
        body.to_string()
    } else {
        body.strip_prefix(&format!("{}_", spec.prefix))
            .unwrap_or(spec.prefix)
            .to_string()
    };

    let role = role(raw, spec, &params, &outputs);
    let (name, role) = overrides::apply(&raw.name, name, role);

    let edits = params
        .iter()
        .any(|param| param.role == ParamRole::DocumentMut);
    let answers_a_list = params
        .iter()
        .any(|param| param.role == ParamRole::OutputList);
    let sized_by = if edits && answers_a_list {
        let sizer = SIZED_BY
            .iter()
            .find(|(symbol, _)| *symbol == raw.name)
            .map(|(_, sizer)| (*sizer).to_string());
        Some(sizer.ok_or_else(|| ScanError {
            location: "crates/otio-sdk-model/src/classify.rs".to_string(),
            message: format!(
                "`{}` answers with a list and edits the document as it does, so it cannot be \
                 called twice. Add it to `SIZED_BY` with the call that says how long its answer \
                 will be.",
                raw.name
            ),
        })?)
    } else {
        None
    };

    let optional = raw.body.contains("no_value(")
        || raw
            .docs
            .iter()
            .any(|line| line.contains("OTIO_STATUS_NO_VALUE"));

    crate::placement::annotate(&raw.name, &mut params)?;

    Ok(Function {
        symbol: raw.name.clone(),
        name,
        role,
        params,
        result,
        outputs,
        sized_by,
        optional,
        docs: docs(&raw.docs),
    })
}

/// Works out what part each C parameter plays.
fn params(
    raw: &RawFunction,
    spec: &GroupSpec,
    known: &[&str],
    detached: bool,
) -> Scanned<Vec<Param>> {
    let mut built: Vec<Param> = Vec::new();
    // A call that belongs to no receiver — plumbing, or one the naming table
    // detached because it is about two things equally — supplies everything
    // as an argument.
    let mut receiver_taken = detached || PLUMBING.contains(&raw.name.as_str());
    let mut index = 0;

    while index < raw.params.len() {
        let param = &raw.params[index];
        let next = raw.params.get(index + 1);
        let rest = &raw.params[index + 1..];
        let is_out = param.name.starts_with("out_");

        let (role, ty, consumed) = classify_param(
            param,
            next,
            rest,
            is_out,
            spec,
            known,
            &mut receiver_taken,
            raw,
        )
        .ok_or_else(|| ScanError {
            location: "crates/otio-capi/src".to_string(),
            message: format!(
                "the parameter `{}: {}` of `{}` fits none of the C ABI's conventions",
                param.name, param.rust_type, raw.name
            ),
        })?;

        built.push(Param {
            name: param.name.clone(),
            role,
            ty,
            optional: optional_param(raw, param),
            docs: Docs::default(),
            placement: None,
        });
        if consumed > 1 {
            for extra in 1..consumed {
                let follower = &raw.params[index + extra];
                built.push(Param {
                    name: follower.name.clone(),
                    role: match (role, extra) {
                        (ParamRole::OutputList, 1) => ParamRole::ListCapacity,
                        (ParamRole::OutputList, _) => ParamRole::OutputCount,
                        // A run of bytes and a list both lend a pointer and
                        // then say how long it is.
                        _ => ParamRole::Length,
                    },
                    ty: Type::Size,
                    optional: false,
                    docs: Docs::default(),
                    placement: None,
                });
            }
        }
        index += consumed;
    }
    Ok(built)
}

/// The rules, one parameter at a time. Returns the part it plays, the type it
/// carries, and how many parameters the rule consumed.
#[allow(clippy::too_many_arguments)]
fn classify_param(
    param: &RawParam,
    next: Option<&RawParam>,
    rest: &[RawParam],
    is_out: bool,
    spec: &GroupSpec,
    known: &[&str],
    receiver_taken: &mut bool,
    raw: &RawFunction,
) -> Option<(ParamRole, Type, usize)> {
    let rust = param.rust_type.as_str();

    // The document, which a call either reads or edits.
    if rust == "*const OtioDocument" {
        return Some((ParamRole::DocumentIn, Type::Document, 1));
    }
    if rust == "*mut OtioDocument" && !is_out {
        return Some((ParamRole::DocumentMut, Type::Document, 1));
    }
    if rust == "*mut *mut OtioDocument" {
        // Out means a document the call makes; anything else is one it
        // consumes, which it signals the same way — by nulling the caller's
        // pointer once there is nothing left to free.
        let role = if is_out {
            ParamRole::Output
        } else {
            ParamRole::DocumentTaken
        };
        return Some((role, Type::Document, 1));
    }

    // A run of bytes the caller lends: a pointer and a length.
    if rust == "*const u8" {
        let length = next?;
        if length.rust_type == "usize" {
            return Some((ParamRole::Bytes, Type::Bytes, 2));
        }
        return None;
    }

    // A list the caller lends: a pointer and a count.
    if !is_out {
        if let Some(inner) = rust.strip_prefix("*const ") {
            let counted = next.is_some_and(|length| {
                length.rust_type == "usize" && (length.name == "count" || length.name == "len")
            });
            if known.contains(&inner) && counted {
                let element = value_type(inner, known)?;
                return Some((ParamRole::Input, Type::List(Box::new(element)), 2));
            }
        }
    }

    // A list the library fills: somewhere to put it, how much room there is,
    // and how many there turned out to be. Where a call answers with two
    // things per entry — every child and where each sits — the pointers come
    // in a run and share the one capacity and count.
    if is_out {
        if let Some(inner) = rust.strip_prefix("*mut ") {
            if known.contains(&inner) && list_tail(rest).is_some() {
                let element = value_type(inner, known)?;
                let consumed = if list_tail(rest) == Some(0) { 3 } else { 1 };
                return Some((
                    ParamRole::OutputList,
                    Type::List(Box::new(element)),
                    consumed,
                ));
            }
        }
    }

    // Anywhere else a single result is written.
    if is_out {
        if let Some(inner) = rust.strip_prefix("*mut ") {
            // A buffer holds text unless the ABI named it `out_bytes`, which
            // is how it spells "this is a file, not a string".
            let ty = if inner == "OtioBuffer" {
                if param.name == "out_bytes" {
                    Type::Bytes
                } else {
                    Type::Text
                }
            } else {
                value_type(inner, known)?
            };
            return Some((ParamRole::Output, ty, 1));
        }
    }

    // A struct passed behind a `const` pointer rather than by value is how
    // this ABI spells "optional": null means the caller did not supply one.
    if let Some(inner) = rust.strip_prefix("*const ") {
        if known.contains(&inner) {
            return Some((ParamRole::Input, value_type(inner, known)?, 1));
        }
    }

    let ty = value_type(rust, known)?;

    // The object the call is about. For a node group that is the first node
    // the caller supplies; for a value group, the first value of that type.
    if !*receiver_taken {
        let claims = match spec.receiver {
            ReceiverSpec::Node(_) => ty == Type::Node,
            ReceiverSpec::Value(name) => ty == Type::Struct(name.to_string()),
            ReceiverSpec::Free | ReceiverSpec::Document => false,
        };
        // A constructor's node is an out-parameter, so it never reaches here;
        // one that takes a template node as an argument would, which is why
        // a `new` is never given a receiver.
        if claims && !is_constructor_name(raw) {
            *receiver_taken = true;
            return Some((ParamRole::Receiver, ty, 1));
        }
    }

    Some((ParamRole::Input, ty, 1))
}

/// How many more out-pointers stand between here and a list's
/// `capacity`/`out_count` pair, or `None` if this is not a list at all.
///
/// `rest` starts at the parameter after the one being classified.
fn list_tail(rest: &[RawParam]) -> Option<usize> {
    for (offset, param) in rest.iter().enumerate() {
        if param.rust_type == "usize" && param.name == "capacity" {
            let count = rest.get(offset + 1)?;
            if count.rust_type == "*mut usize" && count.name.starts_with("out_") {
                return Some(offset);
            }
            return None;
        }
        if !(param.name.starts_with("out_") && param.rust_type.starts_with("*mut ")) {
            return None;
        }
    }
    None
}

/// Whether a symbol names a constructor, by the ABI's `_new` convention.
fn is_constructor_name(raw: &RawFunction) -> bool {
    raw.name.ends_with("_new")
}

/// Whether a call accepts "nothing" for a parameter.
///
/// Read from the body rather than the prose: the helpers the C ABI uses to
/// accept an absent argument are named, so what the code does is plain.
fn optional_param(raw: &RawFunction, param: &RawParam) -> bool {
    let name = &param.name;
    // Matched with what follows the name, so that `optional_node(fill_template)`
    // does not also claim a neighbouring `fill`.
    raw.body.contains(&format!("optional_text({name},"))
        || raw.body.contains(&format!("optional_node({name})"))
        || raw.body.contains(&format!("{name}.is_null()"))
        // An options struct behind a `const` pointer: null means "the usual
        // thing", and the helper that reads it does the null check.
        || (param.rust_type.starts_with("*const Otio")
            && !param.rust_type.contains("c_char")
            && !param.rust_type.contains("u8")
            && !raw.body.contains(&format!("{name}, \"{name}\"")))
}

/// Reads what the C function itself returns.
fn result(raw: &RawFunction, known: &[&str]) -> Scanned<CResult> {
    let Some(returns) = raw.returns.as_deref() else {
        return Ok(CResult::Void);
    };
    Ok(match returns {
        "OtioStatus" => CResult::Status,
        "*const c_char" => CResult::StaticText,
        "*mut OtioDocument" => CResult::Value(Type::Document),
        other => CResult::Value(value_type(other, known).ok_or_else(|| ScanError {
            location: "crates/otio-capi/src".to_string(),
            message: format!("`{}` returns `{other}`, which this does not know", raw.name),
        })?),
    })
}

/// What a caller of the generated SDK gets back.
fn outputs(params: &[Param], result: &CResult) -> Vec<Output> {
    let mut built: Vec<Output> = params
        .iter()
        .filter(|param| matches!(param.role, ParamRole::Output | ParamRole::OutputList))
        .map(|param| Output {
            name: param
                .name
                .strip_prefix("out_")
                .unwrap_or(&param.name)
                .to_string(),
            ty: param.ty.clone(),
            // Text and bytes only ever reach a caller as an `OtioBuffer`,
            // which the generated code copies and then frees.
            owned_buffer: matches!(param.ty, Type::Text | Type::Bytes),
        })
        .collect();

    match result {
        CResult::Value(ty) => built.push(Output {
            name: "value".to_string(),
            ty: ty.clone(),
            owned_buffer: false,
        }),
        CResult::StaticText => built.push(Output {
            name: "value".to_string(),
            ty: Type::Text,
            owned_buffer: false,
        }),
        CResult::Status | CResult::Void => {}
    }
    built
}

/// Works out what part an entry point plays in its group.
fn role(raw: &RawFunction, spec: &GroupSpec, params: &[Param], outputs: &[Output]) -> Role {
    if PLUMBING.contains(&raw.name.as_str()) {
        return Role::Plumbing;
    }
    if raw.name.ends_with("_free") {
        return Role::Destructor;
    }

    // What this group's calls are methods on, spelled as a type. A group with
    // no receiver at all has only free functions in it.
    let receiver_type = match spec.receiver {
        ReceiverSpec::Free => None,
        ReceiverSpec::Document => Some(Type::Document),
        ReceiverSpec::Node(_) => Some(Type::Node),
        ReceiverSpec::Value(name) => Some(Type::Struct(name.to_string())),
    };
    let Some(receiver_type) = receiver_type else {
        return Role::Free;
    };

    // The document is a group's receiver by being a parameter at all; a node
    // or a value is one only where the classifier said so.
    let has_receiver = params.iter().any(|param| match spec.receiver {
        ReceiverSpec::Document => {
            matches!(param.role, ParamRole::DocumentIn | ParamRole::DocumentMut)
        }
        _ => param.role == ParamRole::Receiver,
    });

    if !has_receiver {
        // A call with nothing to be a method on either hands back one of the
        // things this group is about, and so builds one, or it does not, and
        // is a plain function that happens to live here.
        return if outputs.iter().any(|output| output.ty == receiver_type) {
            Role::Constructor
        } else {
            Role::Free
        };
    }

    let name = raw.name.as_str();
    if name.contains("_set_") || name.ends_with("_set") {
        return Role::Setter;
    }
    if name.contains("_clear_") {
        return Role::Clearer;
    }

    let supplied = params
        .iter()
        .filter(|param| matches!(param.role, ParamRole::Input | ParamRole::Bytes))
        .count();
    if supplied == 0 && outputs.len() == 1 {
        return Role::Getter;
    }
    Role::Method
}

/// Splits a doc comment into a summary, the rest, and the symbols it names.
fn docs(lines: &[String]) -> Docs {
    let text: Vec<&str> = lines.iter().map(String::as_str).collect();
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in text {
        if line.trim().is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" ").trim().to_string());
                current.clear();
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current.join(" ").trim().to_string());
    }

    let summary = if paragraphs.is_empty() {
        String::new()
    } else {
        paragraphs.remove(0)
    };
    let references = names::references(&summary, &paragraphs);
    Docs {
        summary,
        body: paragraphs,
        references,
    }
}
