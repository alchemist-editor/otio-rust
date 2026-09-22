//! The Go SDK.
//!
//! Go was picked as the first target over the C ABI because cgo is the
//! archetypal C consumer and because Go is about as far from C as a language
//! with a C FFI gets: it has methods, multiple returns, garbage collection,
//! slices and errors as values, and none of them look anything like an
//! out-parameter. If the description in `otio-sdk-model` is rich enough to
//! produce Go a Go programmer would have written, it is rich enough for Swift
//! and Zig, which are closer to C in every one of those respects.
//!
//! What that means in practice:
//!
//! - A handle and the document it belongs to travel together, so calls are
//!   methods: `clip.Duration()`, not `otio_item_duration(doc, clip, &out)`.
//! - The OTIO schema ladder becomes embedded structs, so `Clip` has every
//!   method of `Item`, `Composable` and `Node` without any of them being
//!   written twice.
//! - `OtioStatus` becomes `error`, and `OTIO_STATUS_NO_VALUE` becomes the
//!   sentinel `ErrNoValue`, which is how Go spells "there is nothing here"
//!   and not how it spells "something went wrong".
//! - A two-pass list call becomes a `[]Node`.
//! - An `OtioBuffer` becomes a `string` or a `[]byte`, copied and freed
//!   before the caller ever sees it.
//! - Reading, writing and JSON sit on `Document` and on the package, so the
//!   whole-document path is as short as it would be by hand.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use otio_sdk_model::model::{
    Api, CResult, Docs, Function, Group, Param, ParamRole, Placement, Receiver, Role, Type,
};
use otio_sdk_model::{ALREADY_PARENTED, names};

use crate::emit::File;

/// Where the Go module lives, relative to the workspace root.
const DIR: &str = "sdk/go";

/// The schema the Go binding spells `Node`, being the handle itself.
const ROOT: &str = "SerializableObject";

/// Generates every file of the Go module.
///
/// # Errors
///
/// Fails if two calls would end up with the same name on one Go type, which
/// means the description needs another entry in its naming table.
pub fn generate(api: &Api) -> Result<Vec<File>, String> {
    let backend = Backend::new(api);
    backend.check_names()?;
    Ok(vec![
        File {
            path: PathBuf::from(DIR).join("go.mod"),
            contents: MODULE.to_string(),
        },
        backend.assemble("otio.go", PACKAGE_DOC, true, backend.runtime()),
        backend.assemble("enums.go", "", false, backend.enums()),
        backend.assemble("values.go", "", false, backend.values()?),
        backend.assemble("schema.go", "", false, backend.schema()),
        backend.assemble("objects.go", "", false, backend.objects()?),
        backend.assemble("library.go", "", false, backend.library()?),
        backend.assemble("metadata.go", "", false, backend.metadata()?),
        crate::conformance::go::render(api)?,
    ])
}

/// The Go module file. The version is the oldest Go with generics, which the
/// one generic function here needs.
const MODULE: &str = "module github.com/alchemist-editor/otio-rust/sdk/go\n\ngo 1.21\n";

/// The state a backend carries while it writes.
struct Backend<'a> {
    api: &'a Api,
    /// How each interface symbol is spelled in Go, for rewriting the
    /// documentation.
    spellings: BTreeMap<String, String>,
}

impl<'a> Backend<'a> {
    fn new(api: &'a Api) -> Self {
        let mut spellings = BTreeMap::new();
        for group in &api.groups {
            for function in &group.functions {
                spellings.insert(function.symbol.clone(), self_name(api, group, function));
            }
        }
        for item in &api.enums {
            let go = enum_name(&item.name);
            for variant in &item.variants {
                spellings.insert(
                    variant.c_name.clone(),
                    format!("{go}{}", names::respell(&variant.name, names::INITIALISMS)),
                );
            }
        }
        Self { api, spellings }
    }

    /// Fails if two calls would land on one Go type with the same name.
    ///
    /// Go promotes an embedded type's methods, so a method on `Item` is
    /// reachable on a `Clip`, and two calls named the same on `Item` and on
    /// `Clip` would be one ambiguous selector. Two named the same on `Item`
    /// and on `Effect` would not, because neither derives from the other.
    fn check_names(&self) -> Result<(), String> {
        let mut placed: Vec<(String, String, String)> = Vec::new();
        for (owner, name) in RESERVED {
            placed.push(((*owner).to_string(), (*name).to_string(), String::new()));
        }
        let mut clashes = Vec::new();
        for group in &self.api.groups {
            for function in &group.functions {
                // A hand-written call holds its name through `RESERVED`, so
                // counting the symbol as well would have it collide with
                // itself.
                if matches!(function.role, Role::Plumbing | Role::Destructor)
                    || hidden(&function.symbol)
                {
                    continue;
                }
                let owner = self.owner_of(group, function);
                let name = self_name(self.api, group, function);
                for (other_owner, other_name, other_symbol) in &placed {
                    if *other_name != name || !self.meet(&owner, other_owner) {
                        continue;
                    }
                    let first = if other_symbol.is_empty() {
                        format!("the hand-written {other_owner}.{other_name}")
                    } else {
                        format!("`{other_symbol}`")
                    };
                    clashes.push(format!(
                        "{first} and `{}` are both {name} on {owner}",
                        function.symbol
                    ));
                }
                placed.push((owner, name, function.symbol.clone()));
            }
        }
        if clashes.is_empty() {
            return Ok(());
        }
        Err(format!(
            "the Go names collide:\n  {}\n\nGive one of each pair another name in \
             `otio-sdk-model/src/overrides.rs`.",
            clashes.join("\n  ")
        ))
    }

    /// Whether two owners are places a caller could reach the same name from.
    ///
    /// For an object that is whether one schema derives from the other, since
    /// that is what Go's embedding makes reachable.
    fn meet(&self, left: &str, right: &str) -> bool {
        if left == right {
            return true;
        }
        let (Some(one), Some(other)) =
            (left.strip_prefix("object:"), right.strip_prefix("object:"))
        else {
            return false;
        };
        self.api
            .ancestry(one)
            .iter()
            .any(|schema| schema.name == other)
            || self
                .api
                .ancestry(other)
                .iter()
                .any(|schema| schema.name == one)
    }

    /// The Go type a call hangs off, or the package for a plain function.
    ///
    /// Every type on the schema ladder is answered as the ladder's root,
    /// because Go's embedding puts a method on every type below where it is
    /// declared: two calls named `name` on `Item` and on `Clip` would be one
    /// ambiguous selector on a clip.
    fn owner_of(&self, group: &Group, function: &Function) -> String {
        if let Some((owner, _)) = rehomed(&function.symbol) {
            return owner.to_string();
        }
        match (&group.receiver, function.role) {
            (_, Role::Free) => "package".to_string(),
            (Receiver::None, _) => "package".to_string(),
            // Nothing hangs off the document, because there is no document to
            // hang it off: what is left is a plain function of the package.
            (Receiver::Document, _) => "package".to_string(),
            (Receiver::Node(_), Role::Constructor) => "package".to_string(),
            (Receiver::Node(schema), _) => {
                if group.view {
                    group.name.clone()
                } else {
                    format!("object:{schema}")
                }
            }
            (Receiver::Value(_), Role::Constructor) => "package".to_string(),
            (Receiver::Value(what), _) => struct_name(what),
        }
    }
}

/// Whether a call is given a document to work in.
fn takes_a_document(function: &Function) -> bool {
    function
        .params
        .iter()
        .any(|param| matches!(param.role, ParamRole::DocumentIn | ParamRole::DocumentMut))
}

/// What a call is called in Go.
fn self_name(api: &Api, group: &Group, function: &Function) -> String {
    if let Some((_, name)) = rehomed(&function.symbol) {
        return name.to_string();
    }
    let spelled = names::pascal(&function.name);
    match (&group.receiver, function.role) {
        (Receiver::Node(schema), Role::Constructor) => {
            let go = schema_name(schema);
            if function.name == "new" {
                format!("New{go}")
            } else {
                format!("{go}{spelled}")
            }
        }
        (Receiver::Value(what), Role::Constructor) => {
            let go = struct_name(what);
            if function.name == "new" {
                format!("New{go}")
            } else {
                format!("{go}{spelled}")
            }
        }
        (Receiver::Value(what), Role::Free) => {
            let _ = api;
            format!("{}{spelled}", struct_name(what))
        }
        (Receiver::Document, Role::Constructor) if function.name == "new" => "New".to_string(),
        _ => spelled,
    }
}

/// The Go name of an enum: `OtioNodeKind` becomes `NodeKind`.
fn enum_name(c_name: &str) -> String {
    names::respell(
        c_name.strip_prefix("Otio").unwrap_or(c_name),
        names::INITIALISMS,
    )
}

/// The Go name of a value struct: `OtioRationalTime` becomes `RationalTime`.
fn struct_name(c_name: &str) -> String {
    names::respell(
        c_name.strip_prefix("Otio").unwrap_or(c_name),
        names::INITIALISMS,
    )
}

/// Whether the package writes a call at all.
fn hidden(symbol: &str) -> bool {
    HIDDEN.iter().any(|(name, _)| *name == symbol)
}

/// Names this SDK writes by hand, which a generated one may not take.
const RESERVED: &[(&str, &str)] = &[
    ("package", "Open"),
    ("package", "Save"),
    ("package", "Filter"),
    ("object:SerializableObject", "Close"),
    ("object:SerializableObject", "IsA"),
];

/// The entry points this SDK does not write, and why.
///
/// Every one of them is the arena showing through. With the document hidden
/// there is nothing for a caller to ask them, and the package asks them
/// itself where the answer is still needed — reading a file ends in
/// `otio_document_root`, writing one begins with `otio_document_set_root`,
/// and appending an object to a timeline it did not come from is
/// `otio_document_absorb`.
const HIDDEN: &[(&str, &str)] = &[
    (
        "otio_document_absorb",
        "how an object built on its own joins a timeline, which appending it does",
    ),
    (
        "otio_document_clone",
        "copying an object is `DeepClone`, which is the question a caller has",
    ),
    (
        "otio_document_new",
        "a document is made for each object built",
    ),
    ("otio_document_node_count", "how big the arena is"),
    ("otio_document_root", "what reading a file answers with"),
    (
        "otio_document_set_root",
        "where writing starts, which is the object given",
    ),
    (
        "otio_document_to_json",
        "`Node.ToJSON`, which serialises from wherever it is pointed",
    ),
];

/// Where a call the C ABI hangs off the document belongs once the document is
/// out of sight, and what it is called there.
///
/// Each of these is really about the object it is handed rather than about
/// the arena holding it, and reads that way once it is a method on the
/// object. `contains` becomes `IsLive`, because "is this object in its
/// document" is how a caller with no document asks whether it is still there.
const REHOMED: &[(&str, &str, &str)] = &[
    (
        "otio_document_contains",
        "object:SerializableObject",
        "IsLive",
    ),
    (
        "otio_document_deep_clone",
        "object:SerializableObject",
        "DeepClone",
    ),
    (
        "otio_document_remove",
        "object:SerializableObject",
        "Remove",
    ),
    (
        "otio_document_remove_recursive",
        "object:SerializableObject",
        "RemoveRecursive",
    ),
];

/// What a rehomed call becomes, if it is one.
fn rehomed(symbol: &str) -> Option<(&'static str, &'static str)> {
    REHOMED
        .iter()
        .find(|(name, _, _)| *name == symbol)
        .map(|(_, owner, name)| (*owner, *name))
}

/// Which parameter a call is anchored on, which is the description's answer.
fn anchor_index(function: &Function) -> Result<usize, String> {
    function
        .params
        .iter()
        .position(|param| param.anchor)
        .ok_or_else(|| {
            format!(
                "`{}` is about one of the objects it is handed, and the description does not \
                 say which",
                function.symbol
            )
        })
}

/// The Go name of a schema. The root of the ladder is the handle itself.
fn schema_name(schema: &str) -> String {
    if schema == ROOT {
        "Node".to_string()
    } else {
        schema.to_string()
    }
}

/// The Go type a value has.
fn go_type(ty: &Type) -> String {
    match ty {
        Type::Bool => "bool".to_string(),
        Type::Double => "float64".to_string(),
        Type::Int64 => "int64".to_string(),
        Type::Uint64 => "uint64".to_string(),
        Type::Int32 => "int32".to_string(),
        Type::Uint32 => "uint32".to_string(),
        // Go counts and indexes with `int`, whatever C does.
        Type::Size => "int".to_string(),
        Type::Text => "string".to_string(),
        Type::Bytes => "[]byte".to_string(),
        Type::Node => "Node".to_string(),
        // A call that makes a document hands back what the document is
        // about, since the document itself is not part of the surface.
        Type::Document => "Node".to_string(),
        Type::Struct(name) | Type::Enum(name) => struct_name(name),
        Type::List(inner) => format!("[]{}", go_type(inner)),
    }
}

/// The cgo type a value has.
fn c_type(ty: &Type) -> String {
    match ty {
        Type::Bool => "C.bool".to_string(),
        Type::Double => "C.double".to_string(),
        Type::Int64 => "C.int64_t".to_string(),
        Type::Uint64 => "C.uint64_t".to_string(),
        Type::Int32 => "C.int32_t".to_string(),
        Type::Uint32 => "C.uint32_t".to_string(),
        Type::Size => "C.size_t".to_string(),
        // Anything of variable length arrives as a buffer this side frees.
        Type::Text | Type::Bytes => "C.OtioBuffer".to_string(),
        Type::Node => "C.OtioNode".to_string(),
        Type::Document => "*C.OtioDocument".to_string(),
        Type::Struct(name) | Type::Enum(name) => format!("C.{name}"),
        Type::List(inner) => c_type(inner),
    }
}

/// The zero value of a Go type, for the early return of a failing call.
fn zero_of(ty: &Type) -> String {
    if let Type::Enum(name) = ty {
        // A Go enum is a named integer, so its zero is a conversion and not
        // a composite literal.
        return format!("{}(0)", struct_name(name));
    }
    zero(&go_type(ty))
}

/// The zero value of a Go type, named.
fn zero(go: &str) -> String {
    match go {
        "bool" => "false".to_string(),
        "string" => "\"\"".to_string(),
        "float64" | "int" | "int32" | "int64" | "uint32" | "uint64" => "0".to_string(),
        other if other.starts_with("[]") || other.starts_with('*') => "nil".to_string(),
        other => format!("{other}{{}}"),
    }
}

/// Turns a Go value into the cgo one a call wants.
fn to_c(ty: &Type, value: &str) -> String {
    match ty {
        Type::Bool => format!("C.bool({value})"),
        Type::Double => format!("C.double({value})"),
        Type::Int64 => format!("C.int64_t({value})"),
        Type::Uint64 => format!("C.uint64_t({value})"),
        Type::Int32 => format!("C.int32_t({value})"),
        Type::Uint32 => format!("C.uint32_t({value})"),
        Type::Size => format!("C.size_t({value})"),
        Type::Enum(name) => format!("C.{name}({value})"),
        _ => value.to_string(),
    }
}

/// Turns the cgo value a call gave back into a Go one.
///
/// `owner` is the document a handle belongs to, since a node in Go carries
/// the document it can be resolved against rather than making its caller
/// remember.
fn from_c(ty: &Type, value: &str, owner: &str) -> String {
    match ty {
        Type::Bool => format!("bool({value})"),
        Type::Double => format!("float64({value})"),
        Type::Int64 => format!("int64({value})"),
        Type::Uint64 => format!("uint64({value})"),
        Type::Int32 => format!("int32({value})"),
        Type::Uint32 => format!("uint32({value})"),
        Type::Size => format!("int({value})"),
        Type::Node => format!("Node{{doc: {owner}, h: {value}}}"),
        // Handled in `render`, which has somewhere to put the failure of
        // asking a document what it is about.
        Type::Document => format!("takeDocument({value})"),
        Type::Text => format!("goText({value})"),
        Type::Bytes => format!("goBytes({value})"),
        Type::Enum(name) => format!("{}({value})", struct_name(name)),
        Type::Struct(name) => format!("{}FromC({value})", names::uncapitalize(&struct_name(name))),
        Type::List(_) => value.to_string(),
    }
}

/// Where a call gets the document it is made in, now that a caller no longer
/// hands one over.
///
/// The description says which object the call is anchored on — see
/// `Param::anchor` — and this is what that looks like in Go.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Anchor {
    /// The call is a method, and happens where its receiver is.
    Receiver,
    /// The call is a method on an object the C ABI passes as an ordinary
    /// argument. The argument is the receiver and not a parameter.
    Argument(usize),
    /// The call is a plain function, made in the document of one of the
    /// objects it is handed.
    Named(String),
    /// The same, for a function handed a list of objects.
    List(String),
    /// The call writes a document out and is handed no object to say which.
    /// It takes one, and writing starts there.
    Root,
    /// The call builds something, so it makes a document to build it in.
    Fresh,
    /// The call touches no document at all.
    None,
}

/// One call, being written into one place.
struct Site<'a> {
    api: &'a Api,
    function: &'a Function,
    /// The Go expression for the document pointer the call works in.
    document: String,
    /// The Go expression for the handle or value the call is about.
    receiver: String,
    /// The Go expression for the `*document` anything handed back belongs to.
    owner: String,
    /// Where that document comes from.
    anchor: Anchor,
    /// The Go expression for the object it comes from, where it comes from
    /// one.
    subject: String,
    /// The schema a constructor's handle should be handed back as, so that
    /// `NewClip` answers with a `Clip` rather than a bare `Node`.
    wrap: Option<String>,
}

/// The stand-in for "give up here", written before the shape of a failing
/// return is known and filled in once it is.
const FAIL: &str = "{fail}";

/// A call, written out.
struct Rendered {
    /// The Go parameters, as `name type`.
    params: Vec<String>,
    /// The Go results, the last of which is `error` where the call can fail.
    returns: Vec<String>,
    /// The lines of the body, already indented one tab.
    body: Vec<String>,
}

impl Site<'_> {
    /// Writes the call out.
    fn render(&self) -> Result<Rendered, String> {
        let function = self.function;
        let mut params: Vec<String> = Vec::new();
        let mut args: Vec<String> = Vec::new();
        let mut pre: Vec<String> = Vec::new();
        let mut post: Vec<String> = Vec::new();
        let mut returns: Vec<String> = Vec::new();
        let mut zeros: Vec<String> = Vec::new();
        let mut results: Vec<String> = Vec::new();
        // The buffers a two-pass list call fills, and what goes in them.
        let mut lists: Vec<(String, Type)> = Vec::new();
        let mut length: Option<String> = None;
        // What is done with the document a `Type::Document` output hands
        // back, once the shape of a failing return is known.
        let mut opened: Option<String> = None;

        for (index, param) in function.params.iter().enumerate() {
            let local = format!("c{}", names::pascal(&param.name));
            // The object the call is anchored on is the call's receiver in
            // Go, wherever the C ABI happens to put it.
            if self.anchor == Anchor::Argument(index) {
                args.push(self.receiver.clone());
                continue;
            }
            match param.role {
                ParamRole::DocumentIn | ParamRole::DocumentMut => {
                    if self.anchor == Anchor::Root {
                        params.push("root Node".to_string());
                    }
                    args.push(self.document.clone());
                }
                ParamRole::DocumentTaken => {
                    // Consuming a document means closing the caller's handle
                    // on it too, which is bookkeeping this emitter has no way
                    // to do. The one call that takes one is written by hand.
                    return Err(format!(
                        "`{}` consumes a document, so it cannot be emitted mechanically;                          write it by hand and add it to `BY_HAND`",
                        function.symbol
                    ));
                }
                ParamRole::Receiver => args.push(self.receiver.clone()),
                ParamRole::Length => {
                    let taken = length.take().ok_or_else(|| {
                        format!(
                            "`{}` states a length with nothing before it",
                            function.symbol
                        )
                    })?;
                    args.push(taken);
                }
                ParamRole::ListCapacity => args.push("{capacity}".to_string()),
                ParamRole::OutputCount => args.push("&count".to_string()),
                ParamRole::Error => {
                    // The library writes the message beside the status it
                    // returns, so the error is built from what this call
                    // said and not from anything a later one could touch.
                    pre.push("var cError C.OtioBuffer".to_string());
                    args.push("&cError".to_string());
                }
                ParamRole::OutputList => {
                    let Type::List(element) = &param.ty else {
                        return Err(format!("`{}` has a list that is not one", function.symbol));
                    };
                    let buffer = format!("buffer{}", lists.len());
                    args.push(format!("{{list{}}}", lists.len()));
                    lists.push((buffer, (**element).clone()));
                }
                ParamRole::Output => {
                    let name = param.name.strip_prefix("out_").unwrap_or(&param.name);
                    let out = format!("out{}", names::pascal(name));
                    pre.push(format!("var {out} {}", c_type(&param.ty)));
                    args.push(format!("&{out}"));
                    let handed_back = from_c(&param.ty, &out, &self.owner);
                    match (&param.ty, self.wrap.as_deref()) {
                        (Type::Node, Some(schema)) => {
                            returns.push(schema.to_string());
                            zeros.push(format!("{schema}{{}}"));
                            results.push(format!("wrap{schema}({handed_back})"));
                        }
                        // A call that makes a document answers with what the
                        // document is about, which is a question of its own
                        // and can fail.
                        (Type::Document, _) => {
                            returns.push("Node".to_string());
                            zeros.push("Node{}".to_string());
                            results.push("root".to_string());
                            opened = Some(handed_back);
                        }
                        _ => {
                            returns.push(go_type(&param.ty));
                            zeros.push(zero_of(&param.ty));
                            results.push(handed_back);
                        }
                    }
                    if matches!(param.ty, Type::Text | Type::Bytes) {
                        post.push(format!("defer C.otio_buffer_free({out})"));
                    }
                }
                ParamRole::Bytes => {
                    let go = parameter_name(&param.name);
                    params.push(format!("{go} []byte"));
                    pre.push(format!("var {local} *C.uint8_t"));
                    pre.push(format!("if len({go}) > 0 {{"));
                    pre.push(format!(
                        "\t{local} = (*C.uint8_t)(unsafe.Pointer(&{go}[0]))"
                    ));
                    pre.push("}".to_string());
                    args.push(local);
                    length = Some(format!("C.size_t(len({go}))"));
                }
                ParamRole::Input => {
                    let go = parameter_name(&param.name);
                    self.input(
                        param,
                        &go,
                        &local,
                        &mut params,
                        &mut pre,
                        &mut args,
                        &mut length,
                    )?;
                }
            }
        }

        if let CResult::Value(ty) = &function.result {
            returns.push(go_type(ty));
            zeros.push(zero_of(ty));
            results.push(from_c(ty, "value", &self.owner));
        } else if function.result == CResult::StaticText {
            returns.push("string".to_string());
            zeros.push("\"\"".to_string());
            results.push("C.GoString(value)".to_string());
        }

        for (buffer, element) in &lists {
            returns.push(format!("[]{}", go_type(element)));
            zeros.push("nil".to_string());
            results.push(format!("{buffer}Out"));
        }

        let mut body = Vec::new();
        body.extend(self.reach());
        body.extend(pre);
        self.invoke(&mut body, &args, &lists, &zeros)?;
        body.extend(post);
        if let Some(taken) = &opened {
            body.push(format!("root, err := rootOf({taken})"));
            body.push("if err != nil {".to_string());
            body.push(format!("\t{FAIL}"));
            body.push("}".to_string());
        }
        for (buffer, element) in &lists {
            let go = go_type(element);
            body.push(format!("{buffer}Out := make([]{go}, int(count))"));
            body.push(format!("for index, item := range {buffer}[:int(count)] {{"));
            body.push(format!(
                "\t{buffer}Out[index] = {}",
                from_c(element, "item", &self.owner)
            ));
            body.push("}".to_string());
        }

        if function.fallible() {
            returns.push("error".to_string());
            results.push("nil".to_string());
        }
        body.push(format!("return {}", results.join(", ")));

        // A call that answers with a plain value has no error to hand back,
        // so an object from another timeline gets the answer it deserves: no
        // timeline holds one, and no composition has one for a child.
        let mut failing: Vec<String> = zeros.clone();
        if function.fallible() {
            failing.push("err".to_string());
        }
        let give_up = format!("return {}", failing.join(", "));
        let body = body
            .into_iter()
            .map(|line| line.replace(FAIL, &give_up))
            .collect();

        Ok(Rendered {
            params,
            returns,
            body,
        })
    }

    /// The lines that find the document this call is made in.
    fn reach(&self) -> Vec<String> {
        let stop = |line: String| {
            vec![
                line,
                "if err != nil {".to_string(),
                format!("\t{FAIL}"),
                "}".to_string(),
            ]
        };
        match &self.anchor {
            Anchor::None => Vec::new(),
            Anchor::Receiver | Anchor::Argument(_) | Anchor::Named(_) => {
                vec![format!("at := {}.at()", self.subject)]
            }
            Anchor::List(name) => stop(format!("at, err := siteOfAll({name})")),
            Anchor::Root => stop("at, err := rootedAt(root)".to_string()),
            Anchor::Fresh => stop("doc, err := newDocument()".to_string()),
        }
    }

    /// Writes an argument the caller supplies.
    #[allow(clippy::too_many_arguments)]
    fn input(
        &self,
        param: &Param,
        go: &str,
        local: &str,
        params: &mut Vec<String>,
        pre: &mut Vec<String>,
        args: &mut Vec<String>,
        length: &mut Option<String>,
    ) -> Result<(), String> {
        // What the call does with an object it is handed is the description's
        // answer and not this backend's: the same question decides the same
        // way in every binding that hides the document. Getting it backwards
        // is silent — moving an object the call was only going to name
        // swallows the timeline it came from.
        let bring = || match param.placement {
            Some(Placement::Adopt) => Ok("adopt"),
            Some(Placement::AdoptOrphan) => Ok("adoptOrphan"),
            Some(Placement::Require) => Ok("handleOf"),
            None => Err(format!(
                "`{}` takes `{}` as an object and the description does not say what it does \
                 with it",
                self.function.symbol, param.name
            )),
        };
        match (&param.ty, param.optional) {
            (Type::Node, false) => {
                params.push(format!("{go} Node"));
                pre.push(format!("{local}, err := at.doc.{}({go})", bring()?));
                pre.push("if err != nil {".to_string());
                pre.push(format!("\t{FAIL}"));
                pre.push("}".to_string());
                args.push(local.to_string());
                return Ok(());
            }
            (Type::Node, true) => {
                params.push(format!("{go} *Node"));
                pre.push(format!("{local} := C.otio_node_none()"));
                pre.push(format!("if {go} != nil {{"));
                pre.push(format!("\thandle, err := at.doc.{}(*{go})", bring()?));
                pre.push("\tif err != nil {".to_string());
                pre.push(format!("\t\t{FAIL}"));
                pre.push("\t}".to_string());
                pre.push(format!("\t{local} = handle"));
                pre.push("}".to_string());
                args.push(local.to_string());
                return Ok(());
            }
            (Type::List(inner), _) if **inner == Type::Node => {
                params.push(format!("{go} []Node"));
                pre.push(format!("{local} := make([]C.OtioNode, len({go}))"));
                pre.push(format!("for index, item := range {go} {{"));
                pre.push(format!("\thandle, err := at.doc.{}(item)", bring()?));
                pre.push("\tif err != nil {".to_string());
                pre.push(format!("\t\t{FAIL}"));
                pre.push("\t}".to_string());
                pre.push(format!("\t{local}[index] = handle"));
                pre.push("}".to_string());
                pre.push(format!("var {local}First *C.OtioNode"));
                pre.push(format!("if len({local}) > 0 {{"));
                pre.push(format!("\t{local}First = &{local}[0]"));
                pre.push("}".to_string());
                args.push(format!("{local}First"));
                *length = Some(format!("C.size_t(len({go}))"));
                return Ok(());
            }
            _ => {}
        }
        match (&param.ty, param.optional) {
            (Type::Text, true) => {
                params.push(format!("{go} string"));
                pre.push(format!("var {local} *C.char"));
                pre.push(format!("if {go} != \"\" {{"));
                pre.push(format!("\t{local} = C.CString({go})"));
                pre.push(format!("\tdefer C.free(unsafe.Pointer({local}))"));
                pre.push("}".to_string());
                args.push(local.to_string());
            }
            (Type::Text, false) => {
                params.push(format!("{go} string"));
                pre.push(format!("{local} := C.CString({go})"));
                pre.push(format!("defer C.free(unsafe.Pointer({local}))"));
                args.push(local.to_string());
            }
            (Type::Struct(name), true) => {
                params.push(format!("{go} *{}", struct_name(name)));
                pre.push(format!("var {local} *C.{name}"));
                pre.push(format!("if {go} != nil {{"));
                pre.push(format!("\tvalue, release := {go}.c()"));
                pre.push("\tdefer release()".to_string());
                pre.push(format!("\t{local} = &value"));
                pre.push("}".to_string());
                args.push(local.to_string());
            }
            (Type::Struct(_), false) => {
                params.push(format!("{go} {}", go_type(&param.ty)));
                pre.push(format!("{local}, release{} := {go}.c()", names::pascal(go)));
                pre.push(format!("defer release{}()", names::pascal(go)));
                args.push(local.to_string());
            }
            (Type::List(element), _) => {
                params.push(format!("{go} []{}", go_type(element)));
                pre.push(format!("{local} := make([]{}, len({go}))", c_type(element)));
                pre.push(format!("for index, item := range {go} {{"));
                pre.push(format!("\t{local}[index] = {}", to_c(element, "item")));
                pre.push("}".to_string());
                pre.push(format!("var {local}First *{}", c_type(element)));
                pre.push(format!("if len({local}) > 0 {{"));
                pre.push(format!("\t{local}First = &{local}[0]"));
                pre.push("}".to_string());
                args.push(format!("{local}First"));
                *length = Some(format!("C.size_t(len({go}))"));
            }
            (Type::Bytes | Type::Document, _) => {
                return Err(format!(
                    "`{}` takes a `{}` as an argument, which this does not write",
                    self.function.symbol,
                    param.ty.c_name()
                ));
            }
            (ty, _) => {
                params.push(format!("{go} {}", go_type(ty)));
                args.push(to_c(ty, go));
            }
        }
        Ok(())
    }
}

impl Site<'_> {
    /// Writes the call itself, and the two-pass dance where it answers with a
    /// list.
    fn invoke(
        &self,
        body: &mut Vec<String>,
        args: &[String],
        lists: &[(String, Type)],
        zeros: &[String],
    ) -> Result<(), String> {
        let symbol = &self.function.symbol;
        let mut failing: Vec<String> = zeros.to_vec();
        failing.push("statusError(status, cError)".to_string());
        let fail = format!("return {}", failing.join(", "));

        if lists.is_empty() {
            let call = format!("C.{symbol}({})", args.join(", "));
            match &self.function.result {
                CResult::Status => {
                    body.push(format!(
                        "if status := {call}; status != C.OTIO_STATUS_OK {{"
                    ));
                    body.push(format!("\t{fail}"));
                    body.push("}".to_string());
                }
                CResult::Void => body.push(call),
                CResult::Value(_) | CResult::StaticText => body.push(format!("value := {call}")),
            }
            return Ok(());
        }

        body.push("var count C.size_t".to_string());

        if let Some(sizer) = self.function.sized_by.as_deref() {
            // This call empties what it reports, so it cannot be asked twice.
            // Another call says how long the answer will be, and this one is
            // made once into a buffer that size.
            body.push(format!(
                "// {symbol} answers and empties in one go, so the buffer is sized first."
            ));
            body.push("var room C.size_t".to_string());
            let sizing = self.sizing_call(sizer)?;
            body.push(format!(
                "if status := {sizing}; status != C.OTIO_STATUS_OK {{"
            ));
            body.push(format!("\t{fail}"));
            body.push("}".to_string());
            for (buffer, element) in lists {
                body.extend(allocate(buffer, element, "int(room)"));
            }
            let filled = fill(args, lists);
            body.push(format!(
                "if status := C.{symbol}({filled}); status != C.OTIO_STATUS_OK {{"
            ));
            body.push(format!("\t{fail}"));
            body.push("}".to_string());
        } else {
            let sized: Vec<String> = args
                .iter()
                .map(|argument| {
                    if argument.starts_with("{list") {
                        "nil".to_string()
                    } else if argument == "{capacity}" {
                        "0".to_string()
                    } else {
                        argument.clone()
                    }
                })
                .collect();
            body.push(format!(
                "if status := C.{symbol}({}); status != C.OTIO_STATUS_OK {{",
                sized.join(", ")
            ));
            body.push(format!("\t{fail}"));
            body.push("}".to_string());
            for (buffer, element) in lists {
                body.extend(allocate(buffer, element, "int(count)"));
            }
            let filled = fill(args, lists);
            body.push(format!(
                "if status := C.{symbol}({filled}); status != C.OTIO_STATUS_OK {{"
            ));
            body.push(format!("\t{fail}"));
            body.push("}".to_string());
        }

        // A document does not change between the two calls, so this cannot
        // trip; it is here so that a mistaken count is a short slice rather
        // than a panic in someone else's program.
        body.push(format!("if int(count) > len({}) {{", lists[0].0.clone()));
        body.push(format!("\tcount = C.size_t(len({}))", lists[0].0));
        body.push("}".to_string());
        Ok(())
    }

    /// Writes the call that says how long a consuming list call's answer will
    /// be.
    fn sizing_call(&self, sizer: &str) -> Result<String, String> {
        let function = self
            .api
            .functions()
            .find(|candidate| candidate.symbol == sizer)
            .ok_or_else(|| format!("`{sizer}` is not a call this library has"))?;
        let mut args = Vec::new();
        for param in &function.params {
            match param.role {
                ParamRole::DocumentIn | ParamRole::DocumentMut => args.push(self.document.clone()),
                ParamRole::Receiver => args.push(self.receiver.clone()),
                ParamRole::Output => args.push("&room".to_string()),
                ParamRole::Error => args.push("&cError".to_string()),
                _ => {
                    return Err(format!(
                        "`{sizer}` takes a `{}`, so it cannot size another call's answer",
                        param.name
                    ));
                }
            }
        }
        Ok(format!("C.{sizer}({})", args.join(", ")))
    }
}

/// The lines that make a buffer for one of a call's lists.
fn allocate(buffer: &str, element: &Type, size: &str) -> Vec<String> {
    let c = c_type(element);
    vec![
        format!("{buffer} := make([]{c}, {size})"),
        format!("var {buffer}First *{c}"),
        format!("if len({buffer}) > 0 {{"),
        format!("\t{buffer}First = &{buffer}[0]"),
        "}".to_string(),
    ]
}

/// Fills in a call's list pointers and capacity, now that the buffers exist.
fn fill(args: &[String], lists: &[(String, Type)]) -> String {
    args.iter()
        .map(|argument| {
            if let Some(index) = argument
                .strip_prefix("{list")
                .and_then(|rest| rest.strip_suffix('}'))
                .and_then(|digits| digits.parse::<usize>().ok())
            {
                return format!("{}First", lists[index].0);
            }
            if argument == "{capacity}" {
                return format!("C.size_t(len({}))", lists[0].0);
            }
            argument.clone()
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// One tab, which is what `gofmt` indents with.
const TAB: &str = "\t";

impl Backend<'_> {
    /// Puts a header, the cgo preamble and whatever imports the body needs
    /// around generated Go, and names the file it goes in.
    fn assemble(&self, name: &str, package_doc: &str, directives: bool, body: String) -> File {
        let mut out = String::new();
        out.push_str("// Code generated by otio-sdk-gen from crates/otio-capi. DO NOT EDIT.\n\n");
        out.push_str(package_doc);
        out.push_str("package otio\n\n");
        out.push_str("/*\n");
        if directives {
            out.push_str(
                "#cgo CFLAGS: -I${SRCDIR}/../../crates/otio-capi/include\n\
                 #cgo LDFLAGS: -L${SRCDIR}/lib -lotio\n\
                 #cgo linux LDFLAGS: -lm -ldl -lpthread\n\
                 #cgo darwin LDFLAGS: -lm -framework CoreFoundation -framework Security -liconv\n",
            );
        }
        out.push_str("#include <otio.h>\n#include <stdlib.h>\n*/\nimport \"C\"\n");

        // Judged from the code, not from the doc comments, which mention
        // packages a reader might want without this file using them.
        let code: String = body
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut imports: Vec<&str> = Vec::new();
        for (package, used) in [
            ("errors", "errors."),
            ("path/filepath", "filepath."),
            ("runtime", "runtime."),
            ("strconv", "strconv."),
            ("strings", "strings."),
            ("unsafe", "unsafe."),
        ] {
            if code.contains(used) {
                imports.push(package);
            }
        }
        if !imports.is_empty() {
            out.push_str("\nimport (\n");
            for package in imports {
                let _ = writeln!(out, "{TAB}\"{package}\"");
            }
            out.push_str(")\n");
        }
        out.push('\n');
        out.push_str(body.trim_end());
        out.push('\n');

        File {
            path: PathBuf::from(DIR).join(name),
            contents: out,
        }
    }

    /// Rewrites a doc comment into Go's shape: the name first, the interface's
    /// own symbols spelled the way this SDK spells them.
    /// As `doc_saying`, for something whose summary is a noun phrase rather than a
    /// sentence about what it does: `A Status is what a call did`.
    fn doc_of(&self, name: &str, joined: &str, docs: &Docs, symbol: Option<&str>) -> Vec<String> {
        self.doc_saying(name, joined, docs, &[], symbol, "nil")
    }

    /// As `doc`, saying a given word where the interface's prose says `null`.
    fn doc_saying(
        &self,
        name: &str,
        joined: &str,
        docs: &Docs,
        notes: &[String],
        symbol: Option<&str>,
        absent: &str,
    ) -> Vec<String> {
        let mut paragraphs: Vec<String> = Vec::new();
        let summary = self.rewrite(&docs.summary);
        let joined = if joined.is_empty() {
            String::new()
        } else {
            format!("{joined} ")
        };
        let opening = if summary.is_empty() {
            format!("{name} wraps the interface's call of the same name.")
        } else {
            let mut characters = summary.chars();
            let first: String = characters
                .next()
                .map(|character| character.to_lowercase().collect())
                .unwrap_or_default();
            format!("{name} {joined}{first}{}", characters.as_str())
        };
        paragraphs.push(opening);
        for paragraph in &docs.body {
            paragraphs.push(self.rewrite(paragraph));
        }
        paragraphs.extend(notes.iter().cloned());
        for paragraph in &mut paragraphs {
            *paragraph = nil_for_null(paragraph, absent);
        }
        if let Some(symbol) = symbol {
            paragraphs.push(format!("C: {symbol}"));
        }

        let mut lines = Vec::new();
        for (index, paragraph) in paragraphs.iter().enumerate() {
            if index > 0 {
                lines.push("//".to_string());
            }
            for line in wrap(paragraph, 74) {
                lines.push(format!("// {line}"));
            }
        }
        lines
    }

    /// Replaces the interface's own names with this SDK's.
    fn rewrite(&self, text: &str) -> String {
        let mut out = text.to_string();
        // Longest first, so `otio_node_kind` is not half-replaced by a
        // shorter symbol that happens to be a prefix of it.
        let mut symbols: Vec<&String> = self.spellings.keys().collect();
        symbols.sort_by_key(|symbol| std::cmp::Reverse(symbol.len()));
        for symbol in symbols {
            if !out.contains(symbol.as_str()) {
                continue;
            }
            let go = &self.spellings[symbol];
            out = out.replace(&format!("`{symbol}`"), go);
            out = out.replace(symbol.as_str(), go);
        }
        // Go's doc comments are plain text, so the interface's backticks
        // would show up as backticks rather than as code.
        out.replace('`', "")
    }
}

/// Lines up the values of a run of `key: value` entries, which is what
/// `gofmt` does to a map or a struct literal and therefore what this has to
/// write if the generated files are to be already formatted.
fn aligned(entries: &[(String, String)]) -> Vec<String> {
    let widest = entries
        .iter()
        .map(|(key, _)| key.len())
        .max()
        .unwrap_or_default();
    entries
        .iter()
        .map(|(key, value)| {
            let padding = " ".repeat(widest - key.len());
            format!("{key}:{padding} {value},")
        })
        .collect()
}

/// Says what a Go programmer says where the C interface's prose says `null`.
///
/// For a pointer that is `nil`. For a string it is the empty string, since
/// that is how this binding spells "no string at all" — a Go `string` cannot
/// be absent, and making every optional name a `*string` would be worse for
/// every caller who has one.
fn nil_for_null(text: &str, instead: &str) -> String {
    let vowel = instead.starts_with(['a', 'e', 'i', 'o', 'u']);
    let mut words: Vec<String> = Vec::new();
    for word in text.split(' ') {
        let trimmed = word.trim_end_matches(|character: char| !character.is_alphanumeric());
        if !trimmed.eq_ignore_ascii_case("null") {
            words.push(word.to_string());
            continue;
        }
        // "a null key" becomes "an empty key", not "a empty key".
        if let Some(article) = words.last_mut() {
            if article == "a" && vowel {
                *article = "an".to_string();
            } else if article == "A" && vowel {
                *article = "An".to_string();
            } else if (article == "an" || article == "An") && !vowel {
                article.truncate(1);
            }
        }
        words.push(format!("{instead}{}", &word[trimmed.len()..]));
    }
    words.join(" ")
}

/// Wraps a paragraph at a width, on spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Writes a function or method out in full.
fn write_function(out: &mut String, doc: &[String], signature: &str, body: &[String]) {
    for line in doc {
        let _ = writeln!(out, "{line}");
    }
    let _ = writeln!(out, "{signature} {{");
    for line in body {
        if line.is_empty() {
            out.push('\n');
        } else {
            let _ = writeln!(out, "{TAB}{line}");
        }
    }
    let _ = writeln!(out, "}}\n");
}

/// Spells a Go signature from its parts.
fn signature(receiver: Option<&str>, name: &str, params: &[String], returns: &[String]) -> String {
    let receiver = receiver.map_or_else(String::new, |text| format!("({text}) "));
    let results = match returns.len() {
        0 => String::new(),
        1 => format!(" {}", returns[0]),
        _ => format!(" ({})", returns.join(", ")),
    };
    format!("func {receiver}{name}({}){results}", params.join(", "))
}

/// Words Go will not let a parameter be called, and what to call it instead.
///
/// Some are keywords and some are names the generated bodies use themselves —
/// a parameter called `len` would shadow the builtin two lines later. The
/// replacements are words, not decorations, because the name is what someone
/// reading the documentation sees.
const RENAMED: &[(&str, &str)] = &[
    ("range", "span"),
    ("type", "kind"),
    ("len", "length"),
    ("cap", "capacity"),
    ("copy", "source"),
    ("new", "created"),
    ("make", "built"),
    ("string", "text"),
    ("int", "number"),
    ("bool", "flag"),
    ("error", "failure"),
    ("nil", "nothing"),
    ("append", "added"),
    ("func", "function"),
    ("map", "mapping"),
    ("var", "variable"),
    ("const", "constant"),
    ("return", "result"),
    ("default", "fallback"),
    ("select", "selection"),
    ("go", "routine"),
    ("chan", "channel"),
    ("interface", "shape"),
    ("struct", "record"),
    ("package", "grouping"),
    ("import", "imported"),
    ("switch", "choice"),
    ("case", "branch"),
];

/// What a parameter is called in Go.
fn parameter_name(name: &str) -> String {
    let spelled = names::camel(name);
    for (taken, instead) in RENAMED {
        if spelled == *taken {
            return (*instead).to_string();
        }
    }
    spelled
}

/// A short Go receiver name for a type, as Go's own style prefers.
fn receiver_name(go_type_name: &str) -> String {
    go_type_name
        .chars()
        .next()
        .map(|first| first.to_lowercase().to_string())
        .unwrap_or_else(|| "v".to_string())
}

impl Backend<'_> {
    /// Writes every call of one group.
    fn emit_group(&self, out: &mut String, group: &Group) -> Result<(), String> {
        self.emit_some(out, group, |_| true)
    }

    /// Writes the calls of one group that a filter accepts.
    fn emit_some(
        &self,
        out: &mut String,
        group: &Group,
        wanted: impl Fn(&Function) -> bool,
    ) -> Result<(), String> {
        for function in &group.functions {
            if matches!(function.role, Role::Plumbing | Role::Destructor)
                || hidden(&function.symbol)
                || !wanted(function)
            {
                continue;
            }
            self.emit_function(out, group, function)?;
        }
        Ok(())
    }

    /// Writes one call.
    fn emit_function(
        &self,
        out: &mut String,
        group: &Group,
        function: &Function,
    ) -> Result<(), String> {
        let name = self_name(self.api, group, function);
        let mut prologue: Vec<String> = Vec::new();
        let mut receiver_clause: Option<String> = None;
        let mut wrap_as: Option<String> = None;

        // Where the object the call is about, and so the document it is made
        // in, comes from. `at` is that object resolved: the document holding
        // it now, that document's pointer, and its handle there.
        let anchored = "at.ptr".to_string();
        let here = "at.h".to_string();
        let mine = "at.doc".to_string();
        let mut subject = String::new();
        let mut anchor = Anchor::None;

        let (document, receiver, owner) = match (&group.receiver, function.role) {
            (Receiver::Node(schema), Role::Constructor) if takes_a_document(function) => {
                // An object is built in a document of its own, and moves into
                // a timeline's when it is put in one. That is what lets a
                // clip exist before the track it is going to sit on.
                wrap_as = Some(schema_name(schema));
                anchor = Anchor::Fresh;
                ("doc.ptr".to_string(), String::new(), "doc".to_string())
            }
            (Receiver::Node(_), Role::Constructor) => {
                (String::new(), String::new(), "nil".to_string())
            }
            (Receiver::Node(_), _) if group.view => {
                receiver_clause = Some(format!("m {}", group.name));
                subject = "m.node".to_string();
                anchor = Anchor::Receiver;
                (anchored, here, mine)
            }
            (Receiver::Node(schema), _) => {
                let go = schema_name(schema);
                let short = receiver_name(&go);
                receiver_clause = Some(format!("{short} {go}"));
                subject = short;
                anchor = Anchor::Receiver;
                (anchored, here, mine)
            }
            // A constructor makes the document, and a call like
            // `otio_read_options_default` never touches one.
            (Receiver::Document, Role::Constructor) => {
                (String::new(), String::new(), "nil".to_string())
            }
            (Receiver::Document, _) if !takes_a_document(function) => {
                (String::new(), String::new(), "nil".to_string())
            }
            (Receiver::Document, _) => {
                match rehomed(&function.symbol) {
                    // A call the C ABI hangs off the document is about one of
                    // the objects it is handed, so in Go it hangs off that.
                    Some((owner, _)) if owner.starts_with("object:") => {
                        let index = anchor_index(function)?;
                        let go = schema_name(owner.trim_start_matches("object:"));
                        let short = receiver_name(&go);
                        receiver_clause = Some(format!("{short} {go}"));
                        subject = short;
                        anchor = Anchor::Argument(index);
                    }
                    _ => match function.params.iter().position(|param| param.anchor) {
                        Some(index) => {
                            let name = parameter_name(&function.params[index].name);
                            anchor = if matches!(function.params[index].ty, Type::List(_)) {
                                Anchor::List(name.clone())
                            } else {
                                Anchor::Named(name.clone())
                            };
                            subject = name;
                        }
                        // Writing is the one thing left that wants a whole
                        // document and is handed no object to find it by, so
                        // it takes one and starts there.
                        None => anchor = Anchor::Root,
                    },
                }
                (anchored, here, mine)
            }
            (Receiver::Value(_), Role::Constructor | Role::Free) | (Receiver::None, _) => {
                (String::new(), String::new(), "nil".to_string())
            }
            (Receiver::Value(what), _) => {
                let go = struct_name(what);
                let short = receiver_name(&go);
                receiver_clause = Some(format!("{short} {go}"));
                let expression = if self.api.enumeration(what).is_some() {
                    format!("C.{what}({short})")
                } else {
                    prologue.push(format!("self, releaseSelf := {short}.c()"));
                    prologue.push("defer releaseSelf()".to_string());
                    "self".to_string()
                };
                (String::new(), expression, "nil".to_string())
            }
        };

        let site = Site {
            api: self.api,
            function,
            document,
            receiver,
            owner: owner.clone(),
            anchor,
            subject,
            wrap: wrap_as,
        };
        let mut rendered = site.render()?;

        // The document must outlive the call that borrows its pointer.
        if takes_a_document(function) && owner != "nil" {
            let last = rendered.body.len() - 1;
            rendered
                .body
                .insert(last, format!("runtime.KeepAlive({owner})"));
        }

        let mut body = prologue;
        body.extend(rendered.body);

        // Where the interface's prose already says an argument may be absent,
        // it is left to say it rather than said twice in different words.
        let said = function.docs.summary.to_lowercase().contains("null")
            || function
                .docs
                .body
                .iter()
                .any(|paragraph| paragraph.to_lowercase().contains("null"));
        let mut absent = "nil";
        let mut notes = Vec::new();
        for param in function.inputs() {
            if !param.optional {
                continue;
            }
            let go = parameter_name(&param.name);
            if param.ty == Type::Text {
                absent = "empty";
                if !said {
                    notes.push(format!("An empty {go} means none."));
                }
            } else if !said {
                notes.push(format!("A nil {go} means none."));
            }
        }
        if function
            .inputs()
            .any(|param| param.optional && param.ty != Type::Text)
        {
            absent = "nil";
        }
        let says_no_value = std::iter::once(&function.docs.summary)
            .chain(function.docs.body.iter())
            .any(|paragraph| paragraph.contains("NO_VALUE"));
        if function.optional && !says_no_value {
            notes.push(
                "Where there is nothing to report this returns ErrNoValue, which is an \
                 answer rather than a failure."
                    .to_string(),
            );
        }
        let doc = self.doc_saying(
            &name,
            "",
            &function.docs,
            &notes,
            Some(&function.symbol),
            absent,
        );
        let signature = signature(
            receiver_clause.as_deref(),
            &name,
            &rendered.params,
            &rendered.returns,
        );
        write_function(out, &doc, &signature, &body);
        Ok(())
    }
}

/// The package's own documentation, which is the first thing anyone reads.
const PACKAGE_DOC: &str = r#"// Package otio reads, writes and edits OpenTimelineIO timelines.
//
// It is generated from the C interface of the otio-rust core, so it carries
// the whole data model: the schemas, the composition algorithms, the ten edit
// operations and the file-format adapters.
//
// # Objects
//
// An object is a [Node]. The OTIO schemas are Go types that embed one another
// the way the schemas derive from one another, so a [Clip] has every method
// of [Item], [Composable] and [Node]. Ask a node what it is with its As
// method:
//
//	if clip, ok := node.AsClip(); ok {
//		reference, err := clip.MediaReference("")
//	}
//
// Objects are built on their own and put together afterwards, the way
// upstream's own bindings do it:
//
//	track, err := otio.NewTrack("V1")
//	clip, err := otio.NewClip("shot_01")
//	err = track.AppendChild(clip.Node)
//
// Behind that, the core keeps its objects in arenas and an object is an index
// into one. This package does that bookkeeping: a new object gets an arena of
// its own, and appending it to a timeline moves it into the timeline's. What
// that leaves visible is [ErrOtherTimeline], for the calls that only name an
// object rather than placing one — detaching a child that belongs to another
// timeline is a mistake rather than an instruction to merge the two.
//
// Asking an object for something it does not have fails rather than answering
// with a zero value: a clip asked for a track's kind returns an error saying
// so.
//
// # Whole files at a time
//
// Reading answers with what the file was about, and writing starts wherever
// it is pointed:
//
//	root, err := otio.ReadFromFile(otio.FormatCMX3600, "cut.edl", nil)
//	if err != nil {
//		return err
//	}
//	defer root.Close()
//
//	clips, err := root.FindClips()
//
// [Open] and [Save] work the format out from the filename, and [FromJSON]
// and [Node.ToJSON] are the same for OpenTimelineIO's own JSON.
//
// A timeline is released when it is collected, so [Node.Close] is not
// required; it is worth calling anyway, because it frees a whole timeline at
// once and at a moment you chose. A timeline is not safe to use from two
// goroutines while one of them is changing it.
//
// # Errors
//
// A call that can fail returns an error. Where "there is nothing here" is one
// of the answers — an item with no source range, a clip with no active media
// reference — the error is [ErrNoValue], and it means the question was
// answered rather than that something went wrong:
//
//	span, err := clip.SourceRange()
//	if errors.Is(err, otio.ErrNoValue) {
//		// the clip is untrimmed
//	}
//
// # Optional arguments
//
// Where the C interface accepts no string at all, this package takes the
// empty string to mean the same: otio.NewClip("") makes a clip with no name,
// and clip.MediaReference("") asks for the active one. Optional objects and
// optional structs are pointers, and nil means none.
"#;

impl Backend<'_> {
    /// The handle types, the error type, and the plumbing the rest calls.
    fn runtime(&self) -> String {
        let mut out = String::new();
        out.push_str(
            &r#"// A document is the arena the core keeps its objects in.
//
// It is not part of this package's surface. An object carries the document it
// lives in, every object starts life in one of its own, and putting an object
// into a timeline moves it into the timeline's — so what is left for a caller
// to think about is objects. See [Node.Close] for releasing one.
type document struct {
	ptr *C.OtioDocument
	// Where this document's objects went, once another document absorbed
	// them. A handle issued here is looked up in translation and then means
	// something in movedInto.
	movedInto   *document
	translation map[C.OtioNode]C.OtioNode
}

// takeDocument takes ownership of a document the library has just made.
func takeDocument(ptr *C.OtioDocument) *document {
	if ptr == nil {
		return nil
	}
	held := &document{ptr: ptr}
	runtime.SetFinalizer(held, func(doomed *document) { doomed.close() })
	return held
}

// newDocument makes an empty one, for an object about to be built.
func newDocument() (*document, error) {
	ptr := C.otio_document_new()
	if ptr == nil {
		return nil, errors.New("otio: the library could not make a timeline")
	}
	return takeDocument(ptr), nil
}

// live follows the chain to the document holding the objects now.
func (d *document) live() *document {
	// Iteratively: a timeline assembled an object at a time has a chain as
	// long as it has objects, and a stack overflow would be a ridiculous way
	// to fail.
	for d != nil && d.movedInto != nil {
		d = d.movedInto
	}
	return d
}

// close releases the document and every object in it.
func (d *document) close() {
	live := d.live()
	if live == nil || live.ptr == nil {
		return
	}
	C.otio_document_free(live.ptr)
	live.ptr = nil
	runtime.SetFinalizer(live, nil)
}

// absorb moves every object of another document into this one.
//
// The call consumes what it is given: it frees the source and answers with a
// table saying where each of its objects went. The source is left marked as
// moved rather than forgotten, so a [Node] still naming it is translated
// through the table instead of going stale.
func (d *document) absorb(source *document) error {
	if d.ptr == nil || source == nil || source.ptr == nil {
		return refusal(C.OTIO_STATUS_NULL_POINTER)
	}
	// The call cannot be asked twice to size the answer, because the first
	// ask would already have consumed the source. The source's own count is
	// exactly how many objects will move.
	moving := int(C.otio_document_node_count(source.ptr))
	from := make([]C.OtioNode, moving)
	to := make([]C.OtioNode, moving)
	var fromFirst, toFirst *C.OtioNode
	if moving > 0 {
		fromFirst = &from[0]
		toFirst = &to[0]
	}
	var count C.size_t
	var cError C.OtioBuffer
	status := C.otio_document_absorb(d.ptr, &source.ptr, fromFirst, toFirst, C.size_t(moving), &count, &cError)
	runtime.KeepAlive(d)
	runtime.KeepAlive(source)
	if status != C.OTIO_STATUS_OK {
		return statusError(status, cError)
	}
	if int(count) > moving {
		count = C.size_t(moving)
	}
	translation := make(map[C.OtioNode]C.OtioNode, int(count))
	for i := 0; i < int(count); i++ {
		translation[from[i]] = to[i]
	}
	// The library released the source and nulled the slot, so nothing here
	// may free it a second time.
	runtime.SetFinalizer(source, nil)
	source.ptr = nil
	source.movedInto = d
	source.translation = translation
	return nil
}

// A Node is an object: which object, and which timeline it belongs to.
//
// It is a small value, so copying one, storing it and comparing two all work
// as they look. The schema types embed it, so every one of them is a Node and
// has its methods.
type Node struct {
	doc *document
	h   C.OtioNode
}

// A site is an object resolved: the document holding it now, that document's
// pointer, and its handle there.
type site struct {
	doc *document
	ptr *C.OtioDocument
	h   C.OtioNode
}

// at resolves an object through however many documents have absorbed it.
//
// A handle means nothing outside the document that issued it, and absorbing
// reissues every one of them, so a Node held from before is translated a step
// at a time along the chain. ptr is nil for an object whose timeline has been
// released, and the library refuses the call rather than reading freed
// memory.
func (n Node) at() site {
	doc := n.doc
	handle := n.h
	for doc != nil && doc.movedInto != nil {
		if to, ok := doc.translation[handle]; ok {
			handle = to
		}
		doc = doc.movedInto
	}
	var ptr *C.OtioDocument
	if doc != nil {
		ptr = doc.ptr
	}
	return site{doc: doc, ptr: ptr, h: handle}
}

// Close releases the timeline this object belongs to, and everything in it.
//
// It is not required: a timeline nothing refers to any more is released when
// it is collected, which is correct but late. Call it where the moment
// matters — a viewer opening one file after another, say. Every object that
// lived in the timeline fails afterwards.
func (n Node) Close() {
	if n.doc != nil {
		n.doc.close()
	}
}

// siteOfAll finds the timeline a list of objects is about.
//
// The objects are checked one at a time as they are handed over, so this only
// has to say where the call is made; an empty list says nothing, which is the
// one thing it cannot answer.
func siteOfAll(nodes []Node) (site, error) {
	if len(nodes) == 0 {
		return site{}, errors.New("otio: no objects were given, so there is no timeline to work in")
	}
	return nodes[0].at(), nil
}

// rootedAt makes an object the root of its document, which is where writing
// starts.
//
// The C interface writes a document from its root. An object read out of a
// file is already that root; one built here is not, so it is made so — which
// is what writing a track rather than a whole timeline means.
func rootedAt(node Node) (site, error) {
	at := node.at()
	if at.ptr == nil {
		return site{}, refusal(C.OTIO_STATUS_NULL_POINTER)
	}
	var cError C.OtioBuffer
	if status := C.otio_document_set_root(at.ptr, at.h, &cError); status != C.OTIO_STATUS_OK {
		return site{}, statusError(status, cError)
	}
	runtime.KeepAlive(at.doc)
	return at, nil
}

// rootOf answers what a document just read is about.
func rootOf(doc *document) (Node, error) {
	if doc == nil || doc.ptr == nil {
		return Node{}, refusal(C.OTIO_STATUS_NULL_POINTER)
	}
	var out C.OtioNode
	var cError C.OtioBuffer
	status := C.otio_document_root(doc.ptr, &out, &cError)
	runtime.KeepAlive(doc)
	if status != C.OTIO_STATUS_OK {
		return Node{}, statusError(status, cError)
	}
	return Node{doc: doc, h: out}, nil
}

// ErrOtherTimeline is the answer "that object belongs to another timeline".
//
// It is this package's own refusal rather than the library's, which is why it
// carries no [Status]: the call was never made. A call that only names an
// object — detaching a child, asking a composition for its neighbours —
// reports it rather than dragging the other timeline in behind the object.
// Compare with errors.Is:
//
//	if errors.Is(err, otio.ErrOtherTimeline) {
//		// the clip came from somewhere else
//	}
var ErrOtherTimeline = errors.New("otio: the object belongs to another timeline")

// handleOf answers the handle of an object that already lives here.
//
// Used by the calls that only name one. An object from another timeline is
// not in this one and the honest answer is to say so, rather than to move it
// because somebody asked whether it was here.
func (d *document) handleOf(node Node) (C.OtioNode, error) {
	at := node.at()
	// The zero Node is "no object", which every call may be handed.
	if at.doc == nil || bool(C.otio_node_is_none(at.h)) {
		return C.otio_node_none(), nil
	}
	if at.doc != d.live() {
		return C.otio_node_none(), ErrOtherTimeline
	}
	return at.h, nil
}

// adopt answers the handle of an object, bringing it here if it is elsewhere.
//
// Used by the calls that place one. This is where [NewClip] followed by
// track.AppendChild(clip) turns into one timeline rather than two.
func (d *document) adopt(node Node) (C.OtioNode, error) {
	return d.bringHere(node, false)
}

// adoptOrphan is adopt for the calls that make an object a child, which the
// library refuses for one that already has a parent.
func (d *document) adoptOrphan(node Node) (C.OtioNode, error) {
	return d.bringHere(node, true)
}

// alreadyParented is what the library says when it refuses to give an object
// a second parent.
const alreadyParented = @ALREADY_PARENTED@

// bringHere is adopt and adoptOrphan: the handle of an object, bringing it
// here if it is elsewhere, and refusing first what the library would refuse.
//
// Bringing an object here brings its whole timeline, and that cannot be taken
// back: were the library to refuse afterwards, the call would fail with the
// two timelines already merged, and releasing either would release both. So
// an object from another timeline is first asked, there, for its parent. A
// handle that has gone stale fails that question with the library's own
// status and message, and so does anything else the library would not
// accept, and the refusal moves nothing. Where the call makes the object a
// child, an answer that it has a parent is refused too, as the library
// refuses it.
func (d *document) bringHere(node Node, orphan bool) (C.OtioNode, error) {
	at := node.at()
	if at.doc == nil || bool(C.otio_node_is_none(at.h)) {
		return C.otio_node_none(), nil
	}
	here := d.live()
	if here == nil {
		return C.otio_node_none(), refusal(C.OTIO_STATUS_NULL_POINTER)
	}
	if at.doc == here {
		return at.h, nil
	}
	var parent C.OtioNode
	var cError C.OtioBuffer
	status := C.otio_node_parent(at.ptr, at.h, &parent, &cError)
	runtime.KeepAlive(at.doc)
	switch status {
	case C.OTIO_STATUS_OK:
		C.otio_buffer_free(cError)
		if orphan {
			return C.otio_node_none(), &Error{Status: StatusCoreError, Message: alreadyParented}
		}
	case C.OTIO_STATUS_NO_VALUE:
		C.otio_buffer_free(cError)
	default:
		return C.otio_node_none(), statusError(status, cError)
	}
	if err := here.absorb(at.doc); err != nil {
		return C.otio_node_none(), err
	}
	return node.at().h, nil
}

// An Error is a failure the library reported.
//
// Compare one with errors.Is: every error of the same status matches, so
// errors.Is(err, otio.ErrNoValue) asks whether the answer was "nothing".
type Error struct {
	// Status is what kind of failure it was.
	Status Status
	// Message is the sentence the library left about this one.
	Message string
}

// Error gives the sentence the library left, or the status if it left none.
func (e *Error) Error() string {
	if e.Message == "" {
		return e.Status.String()
	}
	return e.Message
}

// Is reports whether another error is the same kind of failure as this one.
func (e *Error) Is(target error) bool {
	other, ok := target.(*Error)
	return ok && other.Status == e.Status
}

// ErrNoValue is the answer "there is nothing here".
//
// An item with no source range and a clip with no active media reference both
// report it. It is not a failure, which is why it is worth telling apart from
// one.
var ErrNoValue = &Error{Status: StatusNoValue, Message: "there is no value"}

// statusError turns a status into an error, with the message the same call
// wrote beside it. It releases the message, so each one is handed here once.
func statusError(status C.OtioStatus, message C.OtioBuffer) error {
	text := goText(message)
	C.otio_buffer_free(message)
	if status == C.OTIO_STATUS_OK {
		return nil
	}
	return &Error{Status: Status(status), Message: text}
}

// refusal is an error this package reports before the library is asked, so
// there is no message from it to carry.
func refusal(status C.OtioStatus) error {
	return &Error{Status: Status(status)}
}

// goText copies a buffer of text out of the library.
func goText(buffer C.OtioBuffer) string {
	if buffer.data == nil {
		return ""
	}
	return C.GoStringN(buffer.data, C.int(buffer.len))
}

// goBytes copies a buffer of bytes out of the library.
func goBytes(buffer C.OtioBuffer) []byte {
	if buffer.data == nil {
		return nil
	}
	return C.GoBytes(unsafe.Pointer(buffer.data), C.int(buffer.len))
}

// Filter keeps the objects one of the As methods accepts, as that type.
//
//	clips := otio.Filter(children, otio.Node.AsClip)
//
// Objects of another schema are left out rather than reported, since asking
// which of a list are clips is a question with an answer.
func Filter[T any](nodes []Node, as func(Node) (T, bool)) []T {
	kept := make([]T, 0, len(nodes))
	for _, node := range nodes {
		if value, ok := as(node); ok {
			kept = append(kept, value)
		}
	}
	return kept
}

// Open reads a timeline from a file, working out its format from the name.
//
// It is the short way to say ReadFromFile when the suffix already says what
// the file holds, which is how upstream's read_from_file behaves when no
// adapter is named. Where the suffix belongs to no format it returns
// ErrNoValue.
func Open(path string) (Node, error) {
	format, err := FormatFromSuffix(strings.TrimPrefix(filepath.Ext(path), "."))
	if err != nil {
		return Node{}, err
	}
	return ReadFromFile(format, path, nil)
}

// Save writes an object out to a file, working out its format from the name.
//
// It is the short way to say WriteToFile, as Open is for ReadFromFile. What
// is written is the object given and everything under it, so passing a
// timeline writes the timeline and passing a track writes the track.
func Save(root Node, path string) error {
	format, err := FormatFromSuffix(strings.TrimPrefix(filepath.Ext(path), "."))
	if err != nil {
		return err
	}
	return WriteToFile(format, root, path, nil)
}

"#
            .replace("@ALREADY_PARENTED@", &format!("{ALREADY_PARENTED:?}")),
        );
        for group in &self.api.groups {
            if group.receiver == Receiver::None {
                let _ = self.emit_group(&mut out, group);
            }
        }
        out
    }

    /// The enums, with the names the library spells them by.
    fn enums(&self) -> String {
        let mut out = String::new();
        for item in &self.api.enums {
            let go = enum_name(&item.name);
            for line in self.doc_of(&format!("A {go}"), "is", &item.docs, Some(&item.name)) {
                let _ = writeln!(out, "{line}");
            }
            let _ = writeln!(out, "type {go} int32\n");
            let _ = writeln!(out, "const (");
            for variant in &item.variants {
                let constant = format!("{go}{}", names::respell(&variant.name, names::INITIALISMS));
                for line in self.doc_of(&constant, "means", &variant.docs, None) {
                    let _ = writeln!(out, "{TAB}{line}");
                }
                let _ = writeln!(out, "{TAB}{constant} {go} = {}", variant.value);
            }
            let _ = writeln!(out, ")\n");

            let _ = writeln!(
                out,
                "// String gives the name the C interface spells this by."
            );
            let _ = writeln!(out, "func (v {go}) String() string {{");
            let _ = writeln!(out, "{TAB}switch v {{");
            for variant in &item.variants {
                let constant = format!("{go}{}", names::respell(&variant.name, names::INITIALISMS));
                let _ = writeln!(out, "{TAB}case {constant}:");
                let _ = writeln!(out, "{TAB}{TAB}return \"{}\"", variant.c_name);
            }
            let _ = writeln!(out, "{TAB}}}");
            let _ = writeln!(out, "{TAB}return \"{go}(\" + strconv.Itoa(int(v)) + \")\"");
            let _ = writeln!(out, "}}\n");
        }
        out
    }
}

impl Backend<'_> {
    /// The value structs, their conversions, and the calls that are methods
    /// on them.
    fn values(&self) -> Result<String, String> {
        let mut out = String::new();
        for item in &self.api.structs {
            // `OtioNode` is the handle, which the runtime spells itself, and
            // `OtioBuffer` is how text crosses the boundary, which nobody
            // using this should ever see.
            if item.plumbing || item.name == "OtioNode" {
                continue;
            }
            let go = struct_name(&item.name);
            for line in self.doc_of(&format!("A {go}"), "is", &item.docs, Some(&item.name)) {
                let _ = writeln!(out, "{line}");
            }
            let _ = writeln!(out, "type {go} struct {{");
            for field in &item.fields {
                for line in self.doc_of(&names::pascal(&field.name), "is", &field.docs, None) {
                    let _ = writeln!(out, "{TAB}{line}");
                }
                let _ = writeln!(
                    out,
                    "{TAB}{} {}",
                    names::pascal(&field.name),
                    go_type(&field.ty)
                );
            }
            let _ = writeln!(out, "}}\n");

            // Into C. Anything of variable length is allocated here and
            // released by the function handed back, so a caller that defers
            // it cannot leak.
            let short = receiver_name(&go);
            let _ = writeln!(
                out,
                "// c spells the value the way the C interface wants it, and hands back the\n\
                 // call that releases whatever had to be allocated for it."
            );
            let _ = writeln!(out, "func ({short} {go}) c() (C.{}, func()) {{", item.name);
            let _ = writeln!(out, "{TAB}var release []func()");
            let _ = writeln!(out, "{TAB}var out C.{}", item.name);
            for field in &item.fields {
                let target = format!("out.{}", field.name);
                let source = format!("{short}.{}", names::pascal(&field.name));
                match &field.ty {
                    Type::Text => {
                        let _ = writeln!(out, "{TAB}if {source} != \"\" {{");
                        let _ = writeln!(out, "{TAB}{TAB}text := C.CString({source})");
                        let _ = writeln!(
                            out,
                            "{TAB}{TAB}release = append(release, func() {{ C.free(unsafe.Pointer(text)) }})"
                        );
                        let _ = writeln!(out, "{TAB}{TAB}{target} = text");
                        let _ = writeln!(out, "{TAB}}}");
                    }
                    Type::Struct(_) => {
                        let field_name = names::camel(&field.name);
                        let _ = writeln!(
                            out,
                            "{TAB}{field_name}Value, {field_name}Release := {source}.c()"
                        );
                        let _ =
                            writeln!(out, "{TAB}release = append(release, {field_name}Release)");
                        let _ = writeln!(out, "{TAB}{target} = {field_name}Value");
                    }
                    ty => {
                        let _ = writeln!(out, "{TAB}{target} = {}", to_c(ty, &source));
                    }
                }
            }
            let _ = writeln!(
                out,
                "{TAB}return out, func() {{\n\
                 {TAB}{TAB}for _, done := range release {{\n\
                 {TAB}{TAB}{TAB}done()\n\
                 {TAB}{TAB}}}\n\
                 {TAB}}}"
            );
            let _ = writeln!(out, "}}\n");

            // Out of C.
            let from = format!("{}FromC", names::uncapitalize(&go));
            let _ = writeln!(
                out,
                "// {from} reads the value back out of the C interface."
            );
            let _ = writeln!(out, "func {from}(value C.{}) {go} {{", item.name);
            let _ = writeln!(out, "{TAB}return {go}{{");
            let entries: Vec<(String, String)> = item
                .fields
                .iter()
                .map(|field| {
                    let source = format!("value.{}", field.name);
                    let converted = match &field.ty {
                        Type::Text => format!("C.GoString({source})"),
                        ty => from_c(ty, &source, "nil"),
                    };
                    (names::pascal(&field.name), converted)
                })
                .collect();
            for line in aligned(&entries) {
                let _ = writeln!(out, "{TAB}{TAB}{line}");
            }
            let _ = writeln!(out, "{TAB}}}");
            let _ = writeln!(out, "}}\n");
        }

        for group in &self.api.groups {
            if matches!(group.receiver, Receiver::Value(_)) {
                self.emit_group(&mut out, group)?;
            }
        }
        Ok(out)
    }

    /// The schema ladder, as Go types that embed one another.
    fn schema(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "// SerializableObject is the root of the OTIO schema ladder, which this package\n\
             // spells Node because that is what a handle to one is."
        );
        let _ = writeln!(out, "type SerializableObject = Node\n");

        for schema in &self.api.schema {
            let Some(parent) = schema.parent.as_deref() else {
                continue;
            };
            let go = schema_name(&schema.name);
            let notes = vec![format!(
                "It is a {}, so it has every method of one.",
                schema_name(parent)
            )];
            for line in self.doc_saying(&format!("A {go}"), "is", &schema.docs, &notes, None, "nil")
            {
                let _ = writeln!(out, "{line}");
            }
            let _ = writeln!(
                out,
                "type {go} struct {{\n{TAB}{}\n}}\n",
                schema_name(parent)
            );
        }

        // Building one up from a handle, one rung at a time.
        for schema in &self.api.schema {
            let Some(parent) = schema.parent.as_deref() else {
                continue;
            };
            let go = schema_name(&schema.name);
            let up = schema_name(parent);
            let inner = if parent == ROOT {
                "node".to_string()
            } else {
                format!("wrap{up}(node)")
            };
            let _ = writeln!(out, "// wrap{go} takes a handle to be a {go}, unchecked.");
            let _ = writeln!(out, "func wrap{go}(node Node) {go} {{");
            let _ = writeln!(out, "{TAB}return {go}{{{up}: {inner}}}");
            let _ = writeln!(out, "}}\n");
        }

        // Which schema derives from which, so that asking a node what it is
        // can answer for a whole branch rather than one leaf.
        let _ = writeln!(
            out,
            "// schemaParents says which schema each one derives from, so that asking whether\n\
             // an object is an Item can say yes for a clip."
        );
        let _ = writeln!(out, "var schemaParents = map[NodeKind]NodeKind{{");
        let entries: Vec<(String, String)> = self
            .api
            .schema
            .iter()
            .filter_map(|schema| {
                let parent = schema.parent.as_deref()?;
                Some((
                    format!(
                        "NodeKind{}",
                        names::respell(&schema.kind, names::INITIALISMS)
                    ),
                    format!("NodeKind{}", names::respell(parent, names::INITIALISMS)),
                ))
            })
            .collect();
        for line in aligned(&entries) {
            let _ = writeln!(out, "{TAB}{line}");
        }
        let _ = writeln!(out, "}}\n");

        let _ = writeln!(
            out,
            "// IsA reports whether the object is of a schema, or of one deriving from it.\n\
             //\n\
             // An object whose document has gone, or whose handle no longer resolves, is of\n\
             // no schema at all, so this answers false rather than guessing.\n\
             func (n Node) IsA(schema NodeKind) bool {{\n\
             {TAB}kind, err := n.SchemaKind()\n\
             {TAB}if err != nil {{\n\
             {TAB}{TAB}return false\n\
             {TAB}}}\n\
             {TAB}for {{\n\
             {TAB}{TAB}if kind == schema {{\n\
             {TAB}{TAB}{TAB}return true\n\
             {TAB}{TAB}}}\n\
             {TAB}{TAB}parent, ok := schemaParents[kind]\n\
             {TAB}{TAB}if !ok {{\n\
             {TAB}{TAB}{TAB}return false\n\
             {TAB}{TAB}}}\n\
             {TAB}{TAB}kind = parent\n\
             {TAB}}}\n\
             }}\n"
        );

        for schema in &self.api.schema {
            if schema.parent.is_none() || !schema.concrete {
                continue;
            }
            let go = schema_name(&schema.name);
            let kind = format!(
                "NodeKind{}",
                names::respell(&schema.kind, names::INITIALISMS)
            );
            let _ = writeln!(
                out,
                "// As{go} reports whether the object is a {go} and, if it is, gives it back\n\
                 // as one. An object of another schema is declined rather than wrapped, so a\n\
                 // {go} method is never called on something that is not one."
            );
            let _ = writeln!(out, "func (n Node) As{go}() ({go}, bool) {{");
            let _ = writeln!(out, "{TAB}if !n.IsA({kind}) {{");
            let _ = writeln!(out, "{TAB}{TAB}return {go}{{}}, false");
            let _ = writeln!(out, "{TAB}}}");
            let _ = writeln!(out, "{TAB}return wrap{go}(n), true");
            let _ = writeln!(out, "}}\n");
        }
        out
    }

    /// The calls that are methods on an object.
    fn objects(&self) -> Result<String, String> {
        let mut out = String::new();
        for group in &self.api.groups {
            if matches!(group.receiver, Receiver::Node(_)) && !group.view {
                // A constructor makes an object rather than acting on one, so
                // it is a function of the package and written with those.
                self.emit_some(&mut out, group, |function| {
                    function.role != Role::Constructor
                })?;
            }
        }
        // The handful the C ABI hangs off the document that are really about
        // an object: taking one out of its timeline, copying it, asking
        // whether it is still there.
        for group in &self.api.groups {
            if group.receiver == Receiver::Document {
                self.emit_some(&mut out, group, |function| {
                    rehomed(&function.symbol).is_some()
                })?;
            }
        }
        Ok(out)
    }

    /// The calls that belong to no object: reading, writing, the algorithms,
    /// the edit operations, and the constructors.
    ///
    /// The C ABI hangs all of these off the document. With the document out
    /// of sight they are plain functions of the package, which is where a Go
    /// programmer would look for `otio.ReadFromFile` and `otio.NewClip`
    /// anyway. The few that are really about an object they are handed are
    /// methods on it instead, and written with the objects — see `REHOMED`.
    fn library(&self) -> Result<String, String> {
        let mut out = String::new();
        for group in &self.api.groups {
            if group.receiver == Receiver::Document {
                self.emit_some(&mut out, group, |function| {
                    rehomed(&function.symbol).is_none()
                })?;
            }
        }
        for group in &self.api.groups {
            if matches!(group.receiver, Receiver::Node(_)) && !group.view {
                self.emit_some(&mut out, group, |function| {
                    function.role == Role::Constructor
                })?;
            }
        }
        Ok(out)
    }

    /// The metadata dictionary every named object carries.
    fn metadata(&self) -> Result<String, String> {
        let mut out = String::new();
        for group in &self.api.groups {
            if !group.view {
                continue;
            }
            let go = &group.name;
            for line in self.doc_of(&format!("A {go}"), "is", &group.docs, None) {
                let _ = writeln!(out, "{line}");
            }
            let _ = writeln!(out, "type {go} struct {{\n{TAB}node Node\n}}\n");
            let _ = writeln!(
                out,
                "// {go} gives the object's metadata, which is a dictionary of its own.\n\
                 //\n\
                 // A path names a value inside it, a step at a time, separated by dots:\n\
                 // \"cmx_3600.reel\" reaches the reel of the dictionary the EDL adapter left\n\
                 // behind, and \"takes[0]\" the first entry of a list.\n\
                 //\n\
                 // A path is followed, not created. Writing one step deep always works, but\n\
                 // a deeper one needs its dictionary to exist first:\n\
                 //\n\
                 //\tmeta := clip.Metadata()\n\
                 //\tmeta.SetDictionary(\"cmx_3600\")\n\
                 //\tmeta.SetString(\"cmx_3600.reel\", \"ZZ100\")\n\
                 func (n Node) {go}() {go} {{\n\
                 {TAB}return {go}{{node: n}}\n\
                 }}\n"
            );
            self.emit_group(&mut out, group)?;
        }
        Ok(out)
    }
}
