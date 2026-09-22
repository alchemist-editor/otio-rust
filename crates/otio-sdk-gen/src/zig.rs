//! The Zig SDK.
//!
//! Zig is the one target with no upstream OpenTimelineIO binding to copy, so
//! what things are *called* still follows upstream's Python and Swift — the
//! schema names, the member names, the bare-noun getter and `set_` setter —
//! and what the binding *is* had to be decided here.
//!
//! Three decisions shape everything below.
//!
//! **The document stays visible.** Every other SDK is moving to hide it, so
//! that a clip is built on its own and adopts a document when it is
//! appended. That rests on each binding keeping a translation chain for
//! handles whose document has been absorbed, which in Go and TypeScript is a
//! finalizer and a weak cache. Zig has neither, and would have to pay for
//! them with an allocator inside every object, a `deinit` on every clip, and
//! a handle that mutates when someone else appends it. A Zig programmer
//! already holds an arena and passes it to the things that allocate from it,
//! which is exactly what a document is. So `Clip.init(doc, "A")` is both the
//! honest shape and the idiomatic one, and ADR 0003 records why this target
//! does not follow the others.
//!
//! **Nothing is marshalled.** The value structs *are* the C structs:
//! `extern struct`, laid out by C's rules, with the offsets the description
//! computed asserted at compile time. A `RationalTime` crosses the boundary
//! as itself, and a wrong layout fails the build rather than corrupting a
//! timeline.
//!
//! **Memory is the caller's, and so is the allocator.** Anything the library
//! hands back that has to be freed — a name, a JSON document, a list of
//! children — is copied into an allocator the caller passes, and freed with
//! `allocator.free`. That is the one rule Zig's standard library holds to
//! everywhere, and it means no wrapper type stands between a caller and a
//! `[]u8`.
//!
//! The rest falls out of those: `OtioStatus` becomes a Zig error set,
//! `OTIO_STATUS_NO_VALUE` becomes `null` in an optional rather than an
//! error, the schema ladder becomes structs with explicitly forwarded
//! methods because Zig has no inheritance and no `usingnamespace` any more,
//! and a two-pass list call becomes a slice.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use otio_sdk_model::model::{
    Api, CResult, Docs, Function, Group, Param, ParamRole, Receiver, Role, Type,
};
use otio_sdk_model::names;

use crate::emit::File;

/// Where the Zig package lives, relative to the workspace root.
const DIR: &str = "sdk/zig";

/// The schema this binding spells `Node`, being the handle itself.
const ROOT: &str = "SerializableObject";

/// The Zig version the package is written for and tested against.
///
/// Zig's build system and its language are both still changing between
/// releases, so a generated package that does not say which one it was
/// written for is a bug report waiting to happen.
const ZIG_VERSION: &str = "0.16.0";

/// The calls this backend writes itself rather than emitting mechanically.
///
/// `otio_document_absorb` consumes the document it is given and answers with
/// a translation table as two parallel lists of handles. Emitted
/// mechanically that is a pair of slices, half of which name a document the
/// same call has just freed. Written by hand it is a slice of pairs and a
/// `source` the call nulls out, so a `defer` that frees it does the right
/// thing.
///
/// A symbol here is still in the description and still checked for a name
/// collision, so the hand-written version cannot quietly diverge from the
/// call it stands for.
const BY_HAND: &[&str] = &["otio_document_absorb"];

/// Generates every file of the Zig package.
///
/// # Errors
///
/// Fails if two calls would end up with the same name on one Zig type, which
/// means the description needs another entry in its naming table, or if a
/// call's shape is one this backend has no way to write.
pub fn generate(api: &Api) -> Result<Vec<File>, String> {
    let backend = Backend::new(api);
    backend.check_respellings()?;
    backend.check_names()?;
    Ok(vec![
        file("build.zig", build_zig()),
        file("build.zig.zon", build_zon(&api.version)),
        file("src/c.zig", backend.c_declarations()),
        file("src/support.zig", backend.support()),
        file("src/enums.zig", backend.enums()?),
        file("src/values.zig", backend.values()?),
        file("src/schema.zig", backend.schema()?),
        file("src/document.zig", backend.document()?),
        file("src/metadata.zig", backend.metadata()?),
        file("src/root.zig", backend.root()?),
        crate::conformance::zig::render(api)?,
    ])
}

/// One generated file, under the package directory.
fn file(name: &str, contents: String) -> File {
    File {
        path: PathBuf::from(DIR).join(name),
        contents,
    }
}

// ---------------------------------------------------------------------------
// Spelling
// ---------------------------------------------------------------------------

/// The Zig name of a type: `OtioRationalTime` becomes `RationalTime`.
///
/// Zig leaves initialisms in title case — its own standard library writes
/// `Utf8View` and `Uri` — so unlike the Go backend nothing is re-capitalised.
fn type_name(c_name: &str) -> String {
    c_name.strip_prefix("Otio").unwrap_or(c_name).to_string()
}

/// The Zig name of a schema. The root of the ladder is the handle itself.
fn schema_name(schema: &str) -> String {
    if schema == ROOT {
        "Node".to_string()
    } else {
        schema.to_string()
    }
}

/// The Zig name of a function: `to_json` becomes `toJson`.
fn fn_name(name: &str) -> String {
    names::camel_with(name, &[])
}

/// The calls Zig will not let this SDK name the way the description does,
/// and what it calls them instead.
///
/// This is the same kind of table as `RENAMED` below, for declarations
/// rather than parameters: a correction the language forces, not an opinion
/// about the interface. A name change that is *not* forced belongs in
/// `otio-sdk-model/src/overrides.rs`, where every backend sees it.
///
/// Each entry names a symbol, so a correction cannot outlive the call it
/// was written for; `check_respellings` proves they all still exist.
const RESPELLED: &[(&str, &str, &str)] = &[
    (
        "otio_transition_type",
        "transitionType",
        "`type` is one of Zig's primitives and cannot be a declaration's \
         name. Upstream's Python calls this member `transition_type`, so \
         this is the name it would have had anyway.",
    ),
    (
        "otio_transition_set_type",
        "setTransitionType",
        "Zig does not force this one; the getter above it does, and a \
         getter and setter that no longer read as a pair are worse than \
         either name alone.",
    ),
];

/// Words Zig will not accept as a declaration's name at all.
///
/// Its keywords are one half; the primitives are the other, and they are
/// not keywords, so nothing stops one being written until the compiler
/// refuses it. Anything landing here has to go in `RESPELLED`.
const PRIMITIVES: &[&str] = &[
    "anyerror",
    "anyframe",
    "anyopaque",
    "anytype",
    "bool",
    "comptime_float",
    "comptime_int",
    "isize",
    "noreturn",
    "type",
    "usize",
    "void",
];

/// The words a `PascalCase` enum variant is made of, for spelling it in the
/// `snake_case` Zig gives its enum tags.
///
/// This splits where Zig would want a word break and nowhere else. A digit
/// belongs to the word in front of it, so `Cmx3600` is one word rather than
/// two; and a lone capital belongs to the word after it, so `UInt` is `uint`
/// rather than `u_int`.
fn tag_words(variant: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    for character in variant.chars() {
        let lone_capital = current.len() == 1 && current.chars().all(char::is_uppercase);
        if character.is_uppercase() && !current.is_empty() && !lone_capital {
            words.push(std::mem::take(&mut current));
        }
        current.push(character);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// The Zig name of an enum variant, escaped if it collides with a keyword.
fn tag_name(variant: &str) -> String {
    let spelled = tag_words(variant)
        .into_iter()
        .map(|word| word.to_lowercase())
        .collect::<Vec<_>>()
        .join("_");
    escape(&spelled)
}

/// Zig's keywords, which an identifier taken from the C ABI may land on.
const KEYWORDS: &[&str] = &[
    "addrspace",
    "align",
    "allowzero",
    "and",
    "anyframe",
    "anytype",
    "asm",
    "async",
    "await",
    "break",
    "callconv",
    "catch",
    "comptime",
    "const",
    "continue",
    "defer",
    "else",
    "enum",
    "errdefer",
    "error",
    "export",
    "extern",
    "fn",
    "for",
    "if",
    "inline",
    "linksection",
    "noalias",
    "noinline",
    "nosuspend",
    "opaque",
    "or",
    "orelse",
    "packed",
    "pub",
    "resume",
    "return",
    "struct",
    "suspend",
    "switch",
    "test",
    "threadlocal",
    "try",
    "union",
    "unreachable",
    "usingnamespace",
    "var",
    "volatile",
    "while",
];

/// Spells an identifier so Zig will accept it, quoting a keyword.
fn escape(name: &str) -> String {
    if KEYWORDS.contains(&name) {
        format!("@\"{name}\"")
    } else {
        name.to_string()
    }
}

/// Words a parameter cannot be called here, and what to call it instead.
///
/// Some are Zig keywords and some are names the generated bodies use for
/// themselves, which a parameter of the same name would shadow. The
/// replacements are words rather than decorations, because the name is what
/// someone reading the documentation sees.
const RENAMED: &[(&str, &str)] = &[
    ("align", "alignment"),
    ("buffer", "bytes"),
    ("const", "constant"),
    ("count", "how_many"),
    // `fill` is also the name of one of the ten edit operations, which is
    // declared on the same type. The C interface's own prose says that with
    // it set "a gap takes its place".
    ("fill", "fill_gap"),
    ("doc", "document"),
    ("enum", "enumeration"),
    ("error", "failure"),
    ("fn", "function"),
    ("len", "length"),
    ("opaque", "hidden"),
    ("return", "answer"),
    ("room", "capacity"),
    ("self", "subject"),
    ("status", "code"),
    ("struct", "record"),
    ("switch", "choice"),
    ("test", "check"),
    ("type", "kind"),
    ("union", "variant"),
    ("var", "variable"),
];

/// What a parameter is called in Zig, before its type is consulted.
fn parameter_name(name: &str) -> String {
    for (taken, instead) in RENAMED {
        if name == *taken {
            return (*instead).to_string();
        }
    }
    escape(name)
}

/// The Zig type a value has where a caller of the SDK sees it.
fn zig_type(ty: &Type) -> String {
    match ty {
        Type::Bool => "bool".to_string(),
        Type::Double => "f64".to_string(),
        Type::Int64 => "i64".to_string(),
        Type::Uint64 => "u64".to_string(),
        Type::Int32 => "i32".to_string(),
        Type::Uint32 => "u32".to_string(),
        Type::Size => "usize".to_string(),
        // Text and bytes come back copied into the caller's allocator.
        Type::Text | Type::Bytes => "[]u8".to_string(),
        Type::Node => "Node".to_string(),
        Type::Document => "*Document".to_string(),
        Type::Struct(name) | Type::Enum(name) => type_name(name),
        Type::List(inner) => format!("[]{}", zig_type(inner)),
    }
}

/// The type a value has in the `extern fn` declarations.
fn c_type(ty: &Type) -> String {
    match ty {
        Type::Bool => "bool".to_string(),
        Type::Double => "f64".to_string(),
        Type::Int64 => "i64".to_string(),
        Type::Uint64 => "u64".to_string(),
        Type::Int32 => "i32".to_string(),
        Type::Uint32 => "u32".to_string(),
        Type::Size => "usize".to_string(),
        Type::Text => "?[*:0]const u8".to_string(),
        Type::Bytes => "?[*]const u8".to_string(),
        Type::Node => "NodeHandle".to_string(),
        Type::Document => "*Document".to_string(),
        Type::Struct(name) | Type::Enum(name) => type_name(name),
        Type::List(inner) => format!("?[*]{}", c_type(inner)),
    }
}

/// The C type an out-parameter of this type is a pointer to, as `c.zig`
/// spells it.
fn out_c_type(ty: &Type) -> String {
    match ty {
        Type::Text | Type::Bytes => "Buffer".to_string(),
        Type::Document => "?*Document".to_string(),
        other => c_type(other),
    }
}

/// The same, spelled from a file that reaches the plumbing through `c`.
fn local_c_type(ty: &Type) -> String {
    match ty {
        Type::Text | Type::Bytes => "c.Buffer".to_string(),
        Type::Node => "c.NodeHandle".to_string(),
        other => out_c_type(other),
    }
}

// ---------------------------------------------------------------------------
// The backend
// ---------------------------------------------------------------------------

/// The state the backend carries while it writes.
struct Backend<'a> {
    api: &'a Api,
    /// How each interface symbol is spelled here, for rewriting the
    /// documentation out of C and into Zig.
    spellings: BTreeMap<String, String>,
}

/// What a generated call hangs off.
#[derive(Clone)]
enum SelfKind {
    /// Nothing: a plain function of the package.
    None,
    /// The document, as `self: *Document`.
    Document,
    /// An object of a schema, as `self: Clip`.
    Node(String),
    /// The metadata view, as `self: Metadata`.
    View(String),
    /// A value struct or enum, by value.
    Value(String),
}

impl<'a> Backend<'a> {
    fn new(api: &'a Api) -> Self {
        let mut spellings = BTreeMap::new();
        for group in &api.groups {
            for function in &group.functions {
                spellings.insert(function.symbol.clone(), self_name(function));
            }
        }
        for item in &api.enums {
            let zig = type_name(&item.name);
            for variant in &item.variants {
                spellings.insert(
                    variant.c_name.clone(),
                    format!("{zig}.{}", tag_name(&variant.name)),
                );
            }
        }
        Self { api, spellings }
    }

    /// The group a schema's own calls live in, if it has any.
    fn group_of(&self, schema: &str) -> Option<&'a Group> {
        self.api
            .groups
            .iter()
            .find(|group| !group.view && group.receiver == Receiver::Node(schema.to_string()))
    }

    /// The view group every object carries, which is the metadata dictionary.
    fn view_group(&self) -> Option<&'a Group> {
        self.api.groups.iter().find(|group| group.view)
    }

    /// Whether a call is one this backend emits at all.
    fn emitted(function: &Function) -> bool {
        !matches!(function.role, Role::Plumbing | Role::Destructor)
            && !BY_HAND.contains(&function.symbol.as_str())
    }

    /// Every call that ends up declared on one schema type, nearest schema
    /// first, paired with the schema that declares it.
    ///
    /// Zig has no inheritance and, since 0.15, no `usingnamespace` either, so
    /// a method of `Item` is on a `Clip` only because the generator writes it
    /// there. Writing them out is what makes `clip.duration()` work, and it
    /// costs nothing but generated lines.
    fn methods_on(&self, schema: &str) -> Vec<(&'a Group, &'a Function, bool)> {
        let mut found = Vec::new();
        for (depth, rung) in self.api.ancestry(schema).iter().enumerate() {
            let Some(group) = self.group_of(&rung.name) else {
                continue;
            };
            for function in &group.functions {
                if !Self::emitted(function) || function.role == Role::Constructor {
                    continue;
                }
                found.push((group, function, depth > 0));
            }
        }
        found
    }

    /// Every name declared on a schema type.
    fn declared_names(&self, schema: &str) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        names.insert("belongsTo".to_string());
        names.insert("isA".to_string());
        if self.view_group().is_some() {
            names.insert("metadata".to_string());
        }
        for rung in self.api.ancestry(schema) {
            names.insert(format!("as{}", schema_name(&rung.name)));
        }
        for below in self.descendants(schema) {
            if below.concrete {
                names.insert(format!("as{}", schema_name(&below.name)));
            }
        }
        if let Some(group) = self.group_of(schema) {
            for function in &group.functions {
                if Self::emitted(function) && function.role == Role::Constructor {
                    names.insert(self_name(function));
                }
            }
        }
        for (_, function, _) in self.methods_on(schema) {
            names.insert(self_name(function));
        }
        names
    }

    /// Every name a call will find already declared beside it.
    ///
    /// A method of `Item` is written out again on every type below it, so
    /// what counts is not what `Item` declares but what any of them do.
    fn taken_for(&self, kind: &SelfKind) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        match kind {
            SelfKind::Node(schema) => {
                names.extend(self.declared_names(schema));
                for below in self.descendants(schema) {
                    names.extend(self.declared_names(&below.name));
                }
            }
            SelfKind::View(_) => {
                if let Some(group) = self.view_group() {
                    for function in &group.functions {
                        names.insert(self_name(function));
                    }
                }
            }
            SelfKind::Document => {
                names.insert("deinit".to_string());
                names.insert("absorb".to_string());
                names.insert("save".to_string());
                for group in &self.api.groups {
                    if group.receiver != Receiver::Document {
                        continue;
                    }
                    for function in &group.functions {
                        if Self::emitted(function) && function.role != Role::Free {
                            names.insert(self_name(function));
                        }
                    }
                }
            }
            SelfKind::Value(what) => {
                names.insert("cName".to_string());
                for group in &self.api.groups {
                    if group.receiver != Receiver::Value(what.clone()) {
                        continue;
                    }
                    for function in &group.functions {
                        names.insert(self_name(function));
                    }
                }
            }
            SelfKind::None => {
                names.insert("open".to_string());
                for group in &self.api.groups {
                    for function in &group.functions {
                        if Self::emitted(function) && owner_of(group, function) == "package" {
                            names.insert(self_name(function));
                        }
                    }
                }
            }
        }
        names
    }

    /// Fails if a correction in `RESPELLED` names a call that is not there,
    /// or if a call would be declared with a name Zig will not accept.
    fn check_respellings(&self) -> Result<(), String> {
        for (symbol, _, _) in RESPELLED {
            if !self
                .api
                .functions()
                .any(|function| function.symbol == *symbol)
            {
                return Err(format!(
                    "`{symbol}` is respelled for Zig, but the C ABI no longer has it. Take the \
                     entry out of `RESPELLED`."
                ));
            }
        }
        for group in &self.api.groups {
            for function in &group.functions {
                if !Self::emitted(function) {
                    continue;
                }
                let name = self_name(function);
                if !declarable(&name) {
                    return Err(format!(
                        "`{}` would be declared as `{name}`, which Zig will not accept as a \
                         name. Add it to `RESPELLED` in the Zig backend with the name it \
                         should have.",
                        function.symbol
                    ));
                }
            }
        }
        Ok(())
    }

    /// Fails if two calls would land on one Zig type with the same name.
    ///
    /// A forwarded method is a real declaration, so a call named the same on
    /// `Item` and on `Clip` is a duplicate declaration rather than something
    /// the compiler resolves. The test is therefore the same one the Go
    /// backend makes for its embedding: two names collide when one of their
    /// schemas derives from the other.
    fn check_names(&self) -> Result<(), String> {
        let mut placed: Vec<(String, String, String)> = Vec::new();
        for (owner, name) in self.reserved() {
            placed.push((owner, name, String::new()));
        }
        let mut clashes = Vec::new();
        for group in &self.api.groups {
            for function in &group.functions {
                if !Self::emitted(function) {
                    continue;
                }
                let owner = owner_of(group, function);
                let name = self_name(function);
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
            "the Zig names collide:\n  {}\n\nGive one of each pair another name in \
             `otio-sdk-model/src/overrides.rs`.",
            clashes.join("\n  ")
        ))
    }

    /// The names this SDK writes itself, which a generated one may not take.
    fn reserved(&self) -> Vec<(String, String)> {
        let root = format!("object:{ROOT}");
        let mut names = vec![
            ("Document".to_string(), "deinit".to_string()),
            ("Document".to_string(), "absorb".to_string()),
            ("Document".to_string(), "save".to_string()),
            ("package".to_string(), "open".to_string()),
            (root.clone(), "isA".to_string()),
            (root.clone(), "belongsTo".to_string()),
            (root.clone(), "metadata".to_string()),
        ];
        for schema in &self.api.schema {
            names.push((root.clone(), format!("as{}", schema_name(&schema.name))));
        }
        for item in &self.api.enums {
            names.push((type_name(&item.name), "cName".to_string()));
        }
        names
    }

    /// Whether two owners are places one name could be declared twice.
    ///
    /// A method is written onto every type below the one that declares it,
    /// so two methods meet when either schema derives from the other. A
    /// constructor is written onto exactly one type, so two constructors
    /// meet only on the same schema — but a constructor still meets any
    /// method it will sit beside, which is every method declared at or above
    /// its own schema.
    fn meet(&self, left: &str, right: &str) -> bool {
        if left == right {
            return true;
        }
        let derives = |below: &str, above: &str| {
            self.api
                .ancestry(below)
                .iter()
                .any(|schema| schema.name == above)
        };
        match (owner_schema(left), owner_schema(right)) {
            (Some((one, false)), Some((other, false))) => {
                derives(one, other) || derives(other, one)
            }
            (Some((built, true)), Some((declared, false)))
            | (Some((declared, false)), Some((built, true))) => derives(built, declared),
            _ => false,
        }
    }
}

/// Reads an owner back: which schema, and whether it is the constructor side.
fn owner_schema(owner: &str) -> Option<(&str, bool)> {
    if let Some(schema) = owner.strip_prefix("object:") {
        return Some((schema, false));
    }
    owner.strip_prefix("new:").map(|schema| (schema, true))
}

/// Whether a call is given a document to work in.
fn takes_a_document(function: &Function) -> bool {
    function
        .params
        .iter()
        .any(|param| matches!(param.role, ParamRole::DocumentIn | ParamRole::DocumentMut))
}

/// The Zig type a call is declared on, or the package for a plain function.
///
/// Every rung of the schema ladder answers as the ladder's root, because a
/// method declared on `Item` is written out again on `Clip`.
fn owner_of(group: &Group, function: &Function) -> String {
    match (&group.receiver, function.role) {
        (Receiver::None, _) => "package".to_string(),
        (Receiver::Value(what), _) => type_name(what),
        (_, Role::Free) => "package".to_string(),
        (Receiver::Document, _) => "Document".to_string(),
        (Receiver::Node(schema), Role::Constructor) => {
            // A constructor is a declaration of the type it builds, not of
            // the document it builds in.
            format!("new:{schema}")
        }
        (Receiver::Node(schema), _) => {
            if group.view {
                group.name.clone()
            } else {
                format!("object:{schema}")
            }
        }
    }
}

/// Whether Zig will accept a name for a declaration.
fn declarable(name: &str) -> bool {
    if KEYWORDS.contains(&name) || PRIMITIVES.contains(&name) {
        return false;
    }
    // `i32`, `u8`, `f64`, and every other width Zig spells the same way.
    let sized_number = name.len() > 1
        && matches!(name.as_bytes()[0], b'i' | b'u' | b'f')
        && name[1..].bytes().all(|byte| byte.is_ascii_digit());
    !sized_number
}

/// What a call is called in Zig.
///
/// Unlike the Go backend, nothing here needs the group's name: a
/// constructor is declared inside the type it builds, so `otio_clip_new` is
/// `Clip.init` rather than `NewClip`.
fn self_name(function: &Function) -> String {
    for (symbol, instead, _) in RESPELLED {
        if function.symbol == *symbol {
            return (*instead).to_string();
        }
    }
    if function.role == Role::Constructor && function.name == "new" {
        return "init".to_string();
    }
    fn_name(&function.name)
}

// ---------------------------------------------------------------------------
// Writing one call
// ---------------------------------------------------------------------------

/// One call, being written into one place.
struct Site<'a> {
    api: &'a Api,
    function: &'a Function,
    /// The Zig expression for the document the call works in.
    document: String,
    /// The Zig expression for the handle or value the call is about.
    receiver: String,
    /// The Zig expression for the document anything handed back belongs to,
    /// or `null` where the call has no document in sight.
    owner: String,
    /// The schema a constructor's handle is handed back as, so that
    /// `Clip.init` answers with a `Clip` rather than a bare `Node`.
    wrap: Option<String>,
    /// The names already declared on every type this call will appear on.
    ///
    /// Zig counts a parameter that shares a name with a declaration in
    /// scope as shadowing it, and refuses. Almost every case is a setter
    /// whose argument is named after the property it writes, where `new_`
    /// in front of it is what someone would have written anyway.
    taken: BTreeSet<String>,
}

impl Site<'_> {
    /// What a parameter of this call is called.
    fn param_name(&self, name: &str) -> String {
        let spelled = parameter_name(name);
        if self.taken.contains(&spelled) {
            format!("new_{spelled}")
        } else {
            spelled
        }
    }
}

/// A call, written out.
struct Rendered {
    /// The Zig parameters, as `name: Type`, without `self`.
    params: Vec<String>,
    /// Whether the call has to be given an allocator.
    needs_allocator: bool,
    /// What the function answers with, error union and all.
    return_type: String,
    /// The lines of the body, unindented.
    body: Vec<String>,
    /// The type a call with more than one answer needs declared beside it.
    result_struct: Option<(String, String)>,
}

/// One of the things a call answers with.
struct Answer {
    /// What to call it, where several have to share a struct.
    name: String,
    /// Its Zig type.
    ty: String,
    /// The expression that produces it.
    expr: String,
    /// Whether it holds memory the caller has to give back.
    owned: bool,
}

impl Site<'_> {
    /// Writes the call out.
    #[allow(clippy::too_many_lines)]
    fn render(&self) -> Result<Rendered, String> {
        let function = self.function;
        let mut params: Vec<String> = Vec::new();
        let mut args: Vec<String> = Vec::new();
        let mut pre: Vec<String> = Vec::new();
        let mut post: Vec<String> = Vec::new();
        let mut answers: Vec<Answer> = Vec::new();
        let mut lists: Vec<(String, Type, String)> = Vec::new();
        let mut guards: Vec<String> = Vec::new();
        let mut length: Option<String> = None;
        let mut needs_allocator = false;
        let mut fallible = function.fallible();

        for param in &function.params {
            match param.role {
                ParamRole::DocumentIn | ParamRole::DocumentMut => args.push(self.document.clone()),
                ParamRole::DocumentTaken => {
                    return Err(format!(
                        "`{}` consumes a document, so it cannot be emitted mechanically; write \
                         it by hand and add it to `BY_HAND`",
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
                    // returns, on every return, so the error is built from
                    // what this call said and not from anything a later one
                    // could touch. It starts empty and is freed by a `defer`
                    // declared with it: an allocation that fails before the
                    // call, a "no value" answer that carries a message too,
                    // and a failure all leave the function the same way, and
                    // each releases whatever is there exactly once.
                    pre.push("var out_error: c.Buffer = .{ .data = null, .len = 0 };".to_string());
                    pre.push("defer c.otio_buffer_free(out_error);".to_string());
                    args.push("&out_error".to_string());
                }
                ParamRole::OutputList => {
                    let Type::List(element) = &param.ty else {
                        return Err(format!("`{}` has a list that is not one", function.symbol));
                    };
                    let index = lists.len();
                    let name = param.name.strip_prefix("out_").unwrap_or(&param.name);
                    args.push(format!("{{list{index}}}"));
                    lists.push((format!("raw{index}"), (**element).clone(), name.to_string()));
                    answers.push(Answer {
                        name: name.to_string(),
                        ty: format!("[]{}", zig_type(element)),
                        expr: format!("out{index}"),
                        owned: true,
                    });
                    needs_allocator = true;
                }
                ParamRole::Output => {
                    let name = param.name.strip_prefix("out_").unwrap_or(&param.name);
                    let local = format!("out_{name}");
                    pre.push(format!(
                        "var {local}: {} = undefined;",
                        local_c_type(&param.ty)
                    ));
                    args.push(format!("&{local}"));
                    match &param.ty {
                        Type::Text | Type::Bytes => {
                            needs_allocator = true;
                            post.push(format!("defer c.otio_buffer_free({local});"));
                            answers.push(Answer {
                                name: name.to_string(),
                                ty: "[]u8".to_string(),
                                expr: format!("try support.copyBuffer(allocator, {local})"),
                                owned: true,
                            });
                        }
                        Type::Document => {
                            fallible = true;
                            answers.push(Answer {
                                name: name.to_string(),
                                ty: "*Document".to_string(),
                                expr: format!("{local} orelse return Error.NullPointer"),
                                owned: false,
                            });
                        }
                        Type::Node => {
                            let node =
                                format!("Node{{ .doc = {}, .handle = {local} }}", self.owner);
                            let (ty, expr) = match self.wrap.as_deref() {
                                Some(schema) => (schema.to_string(), wrapped(schema, &node)),
                                None => ("Node".to_string(), node),
                            };
                            answers.push(Answer {
                                name: name.to_string(),
                                ty,
                                expr,
                                owned: false,
                            });
                        }
                        other => answers.push(Answer {
                            name: name.to_string(),
                            ty: zig_type(other),
                            expr: local,
                            owned: false,
                        }),
                    }
                }
                ParamRole::Bytes => {
                    let name = self.param_name(&param.name);
                    params.push(format!("{name}: []const u8"));
                    pre.push(format!(
                        "const arg_{name}: ?[*]const u8 = if ({name}.len > 0) {name}.ptr else null;"
                    ));
                    args.push(format!("arg_{name}"));
                    length = Some(format!("{name}.len"));
                }
                ParamRole::Input => {
                    self.input(
                        param,
                        &mut params,
                        &mut pre,
                        &mut args,
                        &mut length,
                        &mut guards,
                        &mut needs_allocator,
                    )?;
                }
            }
        }

        match &function.result {
            CResult::Value(Type::Document) => {
                fallible = true;
                answers.push(Answer {
                    name: "value".to_string(),
                    ty: "*Document".to_string(),
                    expr: "answer orelse return Error.NullPointer".to_string(),
                    owned: false,
                });
            }
            CResult::Value(Type::Node) => answers.push(Answer {
                name: "value".to_string(),
                ty: "Node".to_string(),
                expr: format!("Node{{ .doc = {}, .handle = answer }}", self.owner),
                owned: false,
            }),
            CResult::Value(ty) => answers.push(Answer {
                name: "value".to_string(),
                ty: zig_type(ty),
                expr: "answer".to_string(),
                owned: false,
            }),
            CResult::StaticText => answers.push(Answer {
                name: "value".to_string(),
                ty: "[]const u8".to_string(),
                expr: "std.mem.span(answer)".to_string(),
                owned: false,
            }),
            CResult::Void | CResult::Status => {}
        }

        if function.optional && !function.fallible() {
            return Err(format!(
                "`{}` answers \"no value\" without a status to say so",
                function.symbol
            ));
        }

        // What the function hands back: nothing, one thing, or a record of
        // the several things a C call writes into several out-parameters.
        let mut result_struct = None;
        // A call that reports "there is nothing here" and has nothing else
        // to say would otherwise answer with `?void`, which is a shape Zig
        // has but nobody wants to read. It answers whether there was
        // something instead.
        let nothing_but_presence = function.optional && answers.is_empty();
        let plain = match answers.len() {
            0 if nothing_but_presence => "bool".to_string(),
            0 => "void".to_string(),
            1 => answers[0].ty.clone(),
            _ => {
                let name = format!("{}Result", names::pascal_with(&function.name, &[]));
                result_struct = Some((name.clone(), record(&name, &answers, function)));
                name
            }
        };
        // A guard rejects an object from another document. A call that
        // answers with a plain value has no error to hand back, so a foreign
        // object gets the answer it deserves: no object equals one, and no
        // document contains one.
        let fail = if !fallible && plain == "bool" {
            "false".to_string()
        } else {
            if !guards.is_empty() {
                fallible = true;
            }
            "Error.ForeignObject".to_string()
        };
        for guard in &mut guards {
            *guard = guard.replace("{fail}", &fail);
        }

        let mut return_type = plain.clone();
        if function.optional && !nothing_but_presence {
            return_type = format!("?{return_type}");
        }
        if fallible || needs_allocator {
            return_type = format!("Error!{return_type}");
        }

        let mut body: Vec<String> = Vec::new();
        body.extend(guards);
        body.extend(pre);
        self.invoke(&mut body, &args, &lists)?;
        body.extend(post);
        for (index, (buffer, element, _)) in lists.iter().enumerate() {
            let zig = zig_type(element);
            body.push(format!(
                "const out{index} = try allocator.alloc({zig}, count);"
            ));
            body.push(format!("errdefer allocator.free(out{index});"));
            body.push(format!("for ({buffer}[0..count], 0..) |item, index| {{"));
            body.push(format!(
                "    out{index}[index] = {};",
                from_c(element, "item", &self.owner)
            ));
            body.push("}".to_string());
        }
        match answers.len() {
            0 if nothing_but_presence => body.push("return true;".to_string()),
            0 => {}
            1 => body.push(format!("return {};", answers[0].expr)),
            _ => {
                body.push("return .{".to_string());
                for answer in &answers {
                    body.push(format!("    .{} = {},", escape(&answer.name), answer.expr));
                }
                body.push("};".to_string());
            }
        }

        collapse(&mut body);
        Ok(Rendered {
            params,
            needs_allocator,
            return_type,
            body,
            result_struct,
        })
    }

    /// Writes an argument the caller supplies.
    #[allow(clippy::too_many_arguments)]
    fn input(
        &self,
        param: &Param,
        params: &mut Vec<String>,
        pre: &mut Vec<String>,
        args: &mut Vec<String>,
        length: &mut Option<String>,
        guards: &mut Vec<String>,
        needs_allocator: &mut bool,
    ) -> Result<(), String> {
        let name = self.param_name(&param.name);
        // A handle is an index into one document's arena, and two documents
        // issue the same indices, so an object from elsewhere would resolve
        // to an unrelated one here rather than failing. Only the Zig value
        // knows where it came from, so this is where it is checked.
        if self.owner != "null" {
            match (&param.ty, param.optional) {
                (Type::Node, true) => guards.push(format!(
                    "if ({name}) |object| if (!object.belongsTo({})) return {{fail}};",
                    self.owner
                )),
                (Type::Node, false) => guards.push(format!(
                    "if (!{name}.belongsTo({})) return {{fail}};",
                    self.owner
                )),
                (Type::List(inner), _) if **inner == Type::Node => guards.push(format!(
                    "for ({name}) |object| if (!object.belongsTo({})) return {{fail}};",
                    self.owner
                )),
                _ => {}
            }
        }
        match (&param.ty, param.optional) {
            (Type::Text, false) => {
                params.push(format!("{name}: [:0]const u8"));
                args.push(format!("{name}.ptr"));
            }
            (Type::Text, true) => {
                params.push(format!("{name}: ?[:0]const u8"));
                pre.push(format!(
                    "const arg_{name}: ?[*:0]const u8 = if ({name}) |text| text.ptr else null;"
                ));
                args.push(format!("arg_{name}"));
            }
            (Type::Node, false) => {
                params.push(format!("{name}: Node"));
                args.push(format!("{name}.handle"));
            }
            (Type::Node, true) => {
                params.push(format!("{name}: ?Node"));
                pre.push(format!(
                    "const arg_{name}: c.NodeHandle = if ({name}) |object| object.handle else \
                     c.otio_node_none();"
                ));
                args.push(format!("arg_{name}"));
            }
            (Type::Struct(what), true) => {
                let zig = type_name(what);
                params.push(format!("{name}: ?{zig}"));
                // The library wants a pointer, and a pointer into the
                // capture of an `if` would not outlive the call, so the
                // value is kept in a local of this function's own.
                pre.push(format!(
                    "const held_{name}: {zig} = if ({name}) |value| value else undefined;"
                ));
                pre.push(format!(
                    "const arg_{name}: ?*const {zig} = if ({name} == null) null else &held_{name};"
                ));
                args.push(format!("arg_{name}"));
            }
            (Type::List(element), _) if **element == Type::Node => {
                *needs_allocator = true;
                params.push(format!("{name}: []const Node"));
                pre.push(format!(
                    "const arg_{name} = try allocator.alloc(c.NodeHandle, {name}.len);"
                ));
                // Next to the allocation rather than after the call: a
                // failing status returns before anything the call wrote is
                // read, and a `defer` registered later than that would not
                // have run.
                pre.push(format!("defer allocator.free(arg_{name});"));
                pre.push(format!(
                    "for ({name}, 0..) |object, index| arg_{name}[index] = object.handle;"
                ));
                args.push(format!("if (arg_{name}.len > 0) arg_{name}.ptr else null"));
                *length = Some(format!("{name}.len"));
            }
            (Type::List(element), _) => {
                let zig = zig_type(element);
                params.push(format!("{name}: []const {zig}"));
                args.push(format!("if ({name}.len > 0) {name}.ptr else null"));
                *length = Some(format!("{name}.len"));
            }
            (Type::Bytes | Type::Document, _) => {
                return Err(format!(
                    "`{}` takes a `{}` as an argument, which this does not write",
                    self.function.symbol,
                    param.ty.c_name()
                ));
            }
            (ty, _) => {
                params.push(format!("{name}: {}", zig_type(ty)));
                args.push(name);
            }
        }
        Ok(())
    }
}

impl Site<'_> {
    /// Writes the call itself, and the two-pass dance where it answers with
    /// a list.
    fn invoke(
        &self,
        body: &mut Vec<String>,
        args: &[String],
        lists: &[(String, Type, String)],
    ) -> Result<(), String> {
        let symbol = &self.function.symbol;
        let optional = self.function.optional;

        if lists.is_empty() {
            let call = format!("c.{symbol}({})", args.join(", "));
            match &self.function.result {
                CResult::Status => {
                    body.push(format!("const status = {call};"));
                    if optional {
                        body.push(format!(
                            "if (status == .no_value) return {};",
                            self.absent()
                        ));
                    }
                    body.push(
                        "if (status != .ok) return support.statusError(status, out_error);"
                            .to_string(),
                    );
                }
                CResult::Void => body.push(format!("{call};")),
                CResult::Value(_) | CResult::StaticText => {
                    body.push(format!("const answer = {call};"));
                }
            }
            return Ok(());
        }

        body.push("var count: usize = 0;".to_string());
        if let Some(sizer) = self.function.sized_by.as_deref() {
            // This call empties what it reports, so it cannot be asked
            // twice. Another call says how long the answer will be, and this
            // one is made once into a buffer that size.
            body.push(format!(
                "// {symbol} answers and empties in one go, so the buffer is sized first."
            ));
            body.push(format!("const sizing = {};", self.sizing_call(sizer)?));
            body.push(
                "if (sizing != .ok) return support.statusError(sizing, out_error);".to_string(),
            );
        } else {
            let sized: Vec<String> = args
                .iter()
                .map(|argument| {
                    if argument.starts_with("{list") {
                        "null".to_string()
                    } else if argument == "{capacity}" {
                        "0".to_string()
                    } else {
                        argument.clone()
                    }
                })
                .collect();
            body.push(format!("const sizing = c.{symbol}({});", sized.join(", ")));
            if optional {
                body.push(format!(
                    "if (sizing == .no_value) return {};",
                    self.absent()
                ));
            }
            body.push(
                "if (sizing != .ok) return support.statusError(sizing, out_error);".to_string(),
            );
        }

        for (buffer, element, _) in lists {
            body.push(format!(
                "const {buffer} = try allocator.alloc({}, count);",
                local_c_type(element)
            ));
            body.push(format!("defer allocator.free({buffer});"));
        }
        body.push(format!("const status = c.{symbol}({});", fill(args, lists)));
        if optional {
            body.push(format!(
                "if (status == .no_value) return {};",
                self.absent()
            ));
        }
        body.push("if (status != .ok) return support.statusError(status, out_error);".to_string());
        // A document does not change between the two calls, so this cannot
        // trip; it is here so that a mistaken count is a short slice rather
        // than a read past the end of a buffer.
        body.push(format!("if (count > {0}.len) count = {0}.len;", lists[0].0));
        Ok(())
    }

    /// What a call answers where the library reports there is nothing.
    fn absent(&self) -> &'static str {
        let nothing_to_say = self
            .function
            .params
            .iter()
            .all(|param| !matches!(param.role, ParamRole::Output | ParamRole::OutputList))
            && matches!(self.function.result, CResult::Status | CResult::Void);
        if nothing_to_say { "false" } else { "null" }
    }

    /// Writes the call that says how long a consuming list call's answer
    /// will be.
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
                ParamRole::Output => args.push("&count".to_string()),
                // The sizing call and the one it sizes share the message:
                // the first writes it empty or fails and ends the function,
                // so the second never writes over one that is still owed a
                // free.
                ParamRole::Error => args.push("&out_error".to_string()),
                _ => {
                    return Err(format!(
                        "`{sizer}` takes a `{}`, so it cannot size another call's answer",
                        param.name
                    ));
                }
            }
        }
        Ok(format!("c.{sizer}({})", args.join(", ")))
    }
}

/// Fills in a call's list pointers and capacity, now that the buffers exist.
fn fill(args: &[String], lists: &[(String, Type, String)]) -> String {
    args.iter()
        .map(|argument| {
            if let Some(index) = argument
                .strip_prefix("{list")
                .and_then(|rest| rest.strip_suffix('}'))
                .and_then(|digits| digits.parse::<usize>().ok())
            {
                let buffer = &lists[index].0;
                return format!("if ({buffer}.len > 0) {buffer}.ptr else null");
            }
            if argument == "{capacity}" {
                return format!("{}.len", lists[0].0);
            }
            argument.clone()
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Folds `const answer = call; return answer;` into one line, which is what
/// someone writing this by hand would have written.
fn collapse(body: &mut Vec<String>) {
    if body.len() < 2 || body[body.len() - 1] != "return answer;" {
        return;
    }
    let Some(call) = body[body.len() - 2].strip_prefix("const answer = ") else {
        return;
    };
    let line = format!("return {call}");
    body.truncate(body.len() - 2);
    body.push(line);
}

/// Turns a value the library handed back into the one a caller sees.
fn from_c(ty: &Type, value: &str, owner: &str) -> String {
    match ty {
        Type::Node => format!("Node{{ .doc = {owner}, .handle = {value} }}"),
        _ => value.to_string(),
    }
}

/// Spells a node as the schema type a constructor answers with.
fn wrapped(schema: &str, node: &str) -> String {
    if schema == "Node" {
        node.to_string()
    } else {
        format!("{schema}{{ .node = {node} }}")
    }
}

/// Puts a block one level in, leaving blank lines blank: Zig's formatter
/// takes an indented empty line as trailing whitespace, and it is right.
fn indented(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        if line.is_empty() {
            out.push('\n');
        } else {
            let _ = writeln!(out, "    {line}");
        }
    }
    out
}

/// The record a call with more than one answer hands back.
fn record(name: &str, answers: &[Answer], function: &Function) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "/// What `{}` answers with.", fn_name(&function.name));
    let _ = writeln!(out, "pub const {name} = struct {{");
    for answer in answers {
        let _ = writeln!(out, "    {}: {},", escape(&answer.name), answer.ty);
    }
    if answers.iter().any(|answer| answer.owned) {
        let _ = writeln!(out, "\n    /// Gives back the memory this holds.");
        let _ = writeln!(
            out,
            "    pub fn deinit(self: {name}, allocator: Allocator) void {{"
        );
        for answer in answers.iter().filter(|answer| answer.owned) {
            let _ = writeln!(
                out,
                "        allocator.free(self.{});",
                escape(&answer.name)
            );
        }
        let _ = writeln!(out, "    }}");
    }
    let _ = writeln!(out, "}};");
    out
}

// ---------------------------------------------------------------------------
// Documentation
// ---------------------------------------------------------------------------

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

impl Backend<'_> {
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
            out = out.replace(symbol.as_str(), &self.spellings[symbol]);
        }
        out
    }

    /// A doc comment, in Zig's shape: the name first, the interface's own
    /// symbols spelled the way this SDK spells them.
    fn doc(
        &self,
        indent: &str,
        name: &str,
        joined: &str,
        docs: &Docs,
        notes: &[String],
        symbol: Option<&str>,
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
        if let Some(symbol) = symbol {
            paragraphs.push(format!("C: `{symbol}`"));
        }

        let mut lines = Vec::new();
        for (index, paragraph) in paragraphs.iter().enumerate() {
            if index > 0 {
                lines.push(format!("{indent}///"));
            }
            for line in wrap(paragraph, 72 - indent.len()) {
                if line.is_empty() {
                    lines.push(format!("{indent}///"));
                } else {
                    lines.push(format!("{indent}/// {line}"));
                }
            }
        }
        lines
    }
}

// ---------------------------------------------------------------------------
// Declaring a call where it belongs
// ---------------------------------------------------------------------------

/// Everything a call needs written around it.
struct Declared {
    /// The declaration, doc comment and all.
    text: String,
    /// The record type it answers with, where it has more than one answer.
    record: Option<String>,
    /// Its name, for a subtype that has to forward it.
    name: String,
    /// Its parameters, without `self`.
    params: Vec<String>,
    /// Whether it takes an allocator.
    needs_allocator: bool,
    /// What it answers with.
    return_type: String,
}

impl Backend<'_> {
    /// Works out where a call stands and writes it.
    fn declare(&self, kind: &SelfKind, function: &Function) -> Result<Declared, String> {
        let mut self_param: Option<String> = None;
        let mut leading: Vec<String> = Vec::new();
        let mut prologue: Vec<String> = Vec::new();
        let mut forced = false;

        let (document, receiver, owner, wrap) = match (kind, function.role) {
            (SelfKind::Node(schema), Role::Constructor) => {
                let wrap = Some(schema_name(schema));
                if takes_a_document(function) {
                    leading.push("document: *Document".to_string());
                    (
                        "document".to_string(),
                        String::new(),
                        "document".to_string(),
                        wrap,
                    )
                } else {
                    (String::new(), String::new(), "null".to_string(), wrap)
                }
            }
            (SelfKind::Node(schema), _) => {
                let zig = schema_name(schema);
                self_param = Some(format!("self: {zig}"));
                let held = if zig == "Node" { "self" } else { "self.node" };
                let owner = format!("{held}.doc");
                if takes_a_document(function) {
                    prologue.push(format!(
                        "const doc = {held}.doc orelse return Error.NullPointer;"
                    ));
                    forced = true;
                    ("doc".to_string(), format!("{held}.handle"), owner, None)
                } else {
                    (String::new(), format!("{held}.handle"), owner, None)
                }
            }
            (SelfKind::View(name), _) => {
                self_param = Some(format!("self: {name}"));
                prologue
                    .push("const doc = self.node.doc orelse return Error.NullPointer;".to_string());
                forced = true;
                (
                    "doc".to_string(),
                    "self.node.handle".to_string(),
                    "self.node.doc".to_string(),
                    None,
                )
            }
            (SelfKind::Document, Role::Constructor) => {
                (String::new(), String::new(), "null".to_string(), None)
            }
            (SelfKind::Document, _) => {
                self_param = Some("self: *Document".to_string());
                ("self".to_string(), String::new(), "self".to_string(), None)
            }
            (SelfKind::Value(what), Role::Constructor | Role::Free) => {
                let _ = what;
                (String::new(), String::new(), "null".to_string(), None)
            }
            (SelfKind::Value(what), _) => {
                self_param = Some(format!("self: {}", type_name(what)));
                (String::new(), "self".to_string(), "null".to_string(), None)
            }
            (SelfKind::None, _) => (String::new(), String::new(), "null".to_string(), None),
        };

        let site = Site {
            api: self.api,
            function,
            document,
            receiver,
            owner,
            wrap,
            taken: self.taken_for(kind),
        };
        let rendered = site.render()?;

        let mut return_type = rendered.return_type;
        if forced && !return_type.starts_with("Error!") {
            return_type = format!("Error!{return_type}");
        }

        let name = self_name(function);
        let mut params: Vec<String> = Vec::new();
        params.extend(leading);
        if rendered.needs_allocator {
            params.push("allocator: Allocator".to_string());
        }
        params.extend(rendered.params.clone());

        let mut signature: Vec<String> = Vec::new();
        if let Some(receiver) = &self_param {
            signature.push(receiver.clone());
        }
        signature.extend(params.clone());

        let mut notes: Vec<String> = Vec::new();
        let says_null = std::iter::once(&function.docs.summary)
            .chain(function.docs.body.iter())
            .any(|paragraph| paragraph.to_lowercase().contains("null"));
        if !says_null {
            for param in function.inputs() {
                if param.optional {
                    notes.push(format!(
                        "A null {} means none.",
                        site.param_name(&param.name)
                    ));
                }
            }
        }
        let says_no_value = std::iter::once(&function.docs.summary)
            .chain(function.docs.body.iter())
            .any(|paragraph| paragraph.contains("NO_VALUE"));
        if function.optional && !says_no_value {
            notes.push(
                if return_type.ends_with("bool") {
                    "Where there was nothing to do this answers false, which is an answer \
                     rather than a failure."
                } else {
                    "Where there is nothing to report this answers null, which is an answer \
                     rather than a failure."
                }
                .to_string(),
            );
        }
        if rendered.needs_allocator {
            notes.push(
                "What this hands back was allocated with allocator, and is the caller's to \
                 free."
                    .to_string(),
            );
        }

        let mut text = String::new();
        for line in self.doc(
            "    ",
            &name,
            "",
            &function.docs,
            &notes,
            Some(&function.symbol),
        ) {
            let _ = writeln!(text, "{line}");
        }
        let _ = writeln!(
            text,
            "    pub fn {name}({}) {return_type} {{",
            signature.join(", ")
        );
        for line in prologue.iter().chain(rendered.body.iter()) {
            if line.is_empty() {
                text.push('\n');
            } else {
                let _ = writeln!(text, "        {line}");
            }
        }
        let _ = writeln!(text, "    }}");

        Ok(Declared {
            text,
            record: rendered.result_struct.map(|(_, decl)| decl),
            name,
            params,
            needs_allocator: rendered.needs_allocator,
            return_type,
        })
    }

    /// Writes a call again on a type that derives from the one declaring it.
    ///
    /// Zig has no inheritance, so this is what makes a method of `Item` a
    /// method of `Clip`. It is one line of body: build the parent's view of
    /// the same handle and hand the call on.
    fn forward(&self, on: &str, from: &str, declared: &Declared, function: &Function) -> String {
        let mut text = String::new();
        let summary = self.rewrite(&function.docs.summary);
        let opening = if summary.is_empty() {
            format!(
                "{} wraps the interface's call of the same name.",
                declared.name
            )
        } else {
            let mut characters = summary.chars();
            let first: String = characters
                .next()
                .map(|character| character.to_lowercase().collect())
                .unwrap_or_default();
            format!("{} {first}{}", declared.name, characters.as_str())
        };
        for line in wrap(&opening, 68) {
            let _ = writeln!(text, "    /// {line}");
        }
        let _ = writeln!(text, "    ///");
        let _ = writeln!(text, "    /// See `{from}.{}`.", declared.name);

        // The record a multi-answer call hands back is declared once, on the
        // type the call itself is declared on.
        let mut return_type = declared.return_type.clone();
        if let Some(record) = record_name(&return_type) {
            return_type = return_type.replace(&record, &format!("{from}.{record}"));
        }

        let mut signature = vec![format!("self: {on}")];
        signature.extend(declared.params.clone());
        let mut arguments: Vec<String> = Vec::new();
        if declared.needs_allocator {
            arguments.push("allocator".to_string());
        }
        for param in &declared.params {
            let name = param.split(':').next().unwrap_or(param).trim();
            if name != "allocator" {
                arguments.push(name.to_string());
            }
        }
        let held = if from == "Node" {
            "self.node".to_string()
        } else {
            format!("({from}{{ .node = self.node }})")
        };
        let _ = writeln!(
            text,
            "    pub fn {}({}) {return_type} {{",
            declared.name,
            signature.join(", ")
        );
        let _ = writeln!(
            text,
            "        return {held}.{}({});",
            declared.name,
            arguments.join(", ")
        );
        let _ = writeln!(text, "    }}");
        text
    }
}

/// The name of the record type in a return type, where there is one.
fn record_name(return_type: &str) -> Option<String> {
    let bare = return_type
        .trim_start_matches("Error!")
        .trim_start_matches('?');
    if bare.ends_with("Result") {
        Some(bare.to_string())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// The files
// ---------------------------------------------------------------------------

/// The line every generated file opens with.
const BANNER: &str = "// Code generated by otio-sdk-gen from crates/otio-capi. DO NOT EDIT.\n";

impl Backend<'_> {
    /// The value structs a caller of the SDK sees.
    fn value_structs(&self) -> impl Iterator<Item = &otio_sdk_model::Struct> {
        self.api
            .structs
            .iter()
            .filter(|item| !item.plumbing && item.name != "OtioNode")
    }

    /// `const RationalTime = values.RationalTime;` for everything a file
    /// mentions but does not declare.
    fn type_aliases(&self, declared: &BTreeSet<String>) -> String {
        let mut out = String::new();
        for item in self.value_structs() {
            let zig = type_name(&item.name);
            if !declared.contains(&zig) {
                let _ = writeln!(out, "const {zig} = values.{zig};");
            }
        }
        for item in &self.api.enums {
            let zig = type_name(&item.name);
            if !declared.contains(&zig) {
                let _ = writeln!(out, "const {zig} = enums.{zig};");
            }
        }
        out
    }

    /// Writes the `comptime` block that holds a struct's layout to what the
    /// description computed for it.
    ///
    /// A generator that marshals by offset is guessing until something
    /// checks it. Zig lays an `extern struct` out by C's rules, so these are
    /// the same numbers from two directions, and a disagreement stops the
    /// build rather than corrupting a timeline.
    fn layout_of(&self, out: &mut String, item: &otio_sdk_model::Struct) {
        let zig = match item.name.as_str() {
            "OtioNode" => "NodeHandle".to_string(),
            "OtioBuffer" => "Buffer".to_string(),
            other => type_name(other),
        };
        let widths = [item.layout.size, item.layout.align]
            .into_iter()
            .chain(item.fields.iter().map(|field| field.offset))
            .any(|width| width.pointer32 != width.pointer64);
        let _ = writeln!(out, "comptime {{");
        if widths {
            let _ = writeln!(out, "    const pointers = @sizeOf(usize);");
        }
        let _ = writeln!(
            out,
            "    std.debug.assert(@sizeOf({zig}) == {});",
            by_width(item.layout.size)
        );
        let _ = writeln!(
            out,
            "    std.debug.assert(@alignOf({zig}) == {});",
            by_width(item.layout.align)
        );
        for field in &item.fields {
            let _ = writeln!(
                out,
                "    std.debug.assert(@offsetOf({zig}, \"{}\") == {});",
                field.name,
                by_width(field.offset)
            );
        }
        let _ = writeln!(out, "}}\n");
    }
}

/// The one thing the layout assertions below cannot check about themselves.
///
/// The description carries a layout for 32-bit pointers and one for 64-bit,
/// and both were computed for an ABI that aligns a 64-bit scalar to eight
/// bytes. i386's System V ABI aligns a `double` to four, so every struct
/// here sits differently there and the assertions would be wrong rather
/// than merely unmet. Saying which targets this package describes is more
/// use to whoever hits it than an offset that does not match.
const ABI_GUARD: &str = r#"comptime {
    if (@alignOf(f64) != 8 or @alignOf(u64) != 8) @compileError(
        "otio: the struct layouts in this package were computed for an ABI " ++
            "that aligns 64-bit scalars to eight bytes. This target does not " ++
            "(i386 is the usual one: its System V ABI aligns a double to " ++
            "four), so the C library lays these structs out differently. " ++
            "Supported targets are the 64-bit ones and wasm32.",
    );
}

"#;

/// A number that may depend on how wide a pointer is, as Zig spells it.
fn by_width(width: otio_sdk_model::ByWidth) -> String {
    if width.pointer32 == width.pointer64 {
        width.pointer64.to_string()
    } else {
        format!(
            "if (pointers == 4) {} else {}",
            width.pointer32, width.pointer64
        )
    }
}

impl Backend<'_> {
    /// The library's own entry points, declared for Zig.
    fn c_declarations(&self) -> String {
        let mut out = String::new();
        out.push_str(BANNER);
        out.push_str(
            "\n//! The library's own interface, declared rather than translated.\n\
             //!\n\
             //! There is no `@cImport` here and no header is read at build time. The\n\
             //! description this is generated from carries every type and every struct\n\
             //! layout, so the declarations are written out directly and the C compiler is\n\
             //! left out of the loop entirely: the package builds wherever Zig builds, and\n\
             //! cross-compiling needs nothing but the static library.\n\
             //!\n\
             //! Nothing here is part of the SDK's surface. It is `pub` because the rest of\n\
             //! the package calls it.\n\n",
        );
        out.push_str("const std = @import(\"std\");\n");
        out.push_str("const values = @import(\"values.zig\");\n");
        out.push_str("const enums = @import(\"enums.zig\");\n\n");
        out.push_str("/// The arena a timeline's objects live in.\n");
        out.push_str("pub const Document = @import(\"document.zig\").Document;\n\n");
        out.push_str(
            "/// A handle to an object in a document: where it sits in the arena, and\n\
             /// which occupant of that slot it is. A handle to something that has been\n\
             /// removed fails a lookup rather than reaching whatever took its place.\n\
             pub const NodeHandle = extern struct {\n\
             \x20   index: u32,\n\
             \x20   generation: u32,\n\
             };\n\n",
        );
        out.push_str(
            "/// A run of bytes the library owns until `otio_buffer_free` is called on\n\
             /// it. The SDK copies out of one and frees it before its caller sees\n\
             /// anything.\n\
             pub const Buffer = extern struct {\n\
             \x20   data: ?[*]const u8,\n\
             \x20   len: usize,\n\
             };\n\n",
        );
        out.push_str(&self.type_aliases(&BTreeSet::new()));
        out.push('\n');
        out.push_str(ABI_GUARD);
        for item in &self.api.structs {
            if item.name == "OtioNode" || item.plumbing {
                self.layout_of(&mut out, item);
            }
        }

        for group in &self.api.groups {
            for function in &group.functions {
                let params: Vec<String> = function
                    .params
                    .iter()
                    .map(|param| format!("{}: {}", escape(&param.name), extern_type(param)))
                    .collect();
                let _ = writeln!(
                    out,
                    "pub extern fn {}({}) {};",
                    function.symbol,
                    params.join(", "),
                    extern_result(&function.result)
                );
            }
        }
        out
    }
}

/// The type a parameter has in an `extern fn` declaration.
fn extern_type(param: &Param) -> String {
    match param.role {
        ParamRole::DocumentIn | ParamRole::DocumentMut => "*Document".to_string(),
        ParamRole::DocumentTaken => "*?*Document".to_string(),
        ParamRole::Length | ParamRole::ListCapacity => "usize".to_string(),
        ParamRole::OutputCount => "*usize".to_string(),
        ParamRole::Error => "?*Buffer".to_string(),
        ParamRole::Output => format!("*{}", out_c_type(&param.ty)),
        ParamRole::OutputList => match &param.ty {
            Type::List(inner) => format!("?[*]{}", c_type(inner)),
            other => format!("?[*]{}", c_type(other)),
        },
        ParamRole::Bytes => "?[*]const u8".to_string(),
        ParamRole::Receiver | ParamRole::Input => match (&param.ty, param.optional) {
            (Type::Text, false) => "[*:0]const u8".to_string(),
            (Type::Text, true) => "?[*:0]const u8".to_string(),
            (Type::Struct(name), true) => format!("?*const {}", type_name(name)),
            (Type::List(inner), _) => format!("?[*]const {}", c_type(inner)),
            (ty, _) => c_type(ty),
        },
    }
}

/// What an `extern fn` declaration answers with.
fn extern_result(result: &CResult) -> String {
    match result {
        CResult::Void => "void".to_string(),
        CResult::Status => "Status".to_string(),
        CResult::StaticText => "[*:0]const u8".to_string(),
        CResult::Value(Type::Document) => "?*Document".to_string(),
        CResult::Value(ty) => c_type(ty),
    }
}

impl Backend<'_> {
    /// The error set, the failure messages, and the copying the rest calls.
    fn support(&self) -> String {
        let mut out = String::new();
        out.push_str(BANNER);
        out.push_str(
            "\n//! What every generated call leans on: the error set the C interface's\n\
             //! status codes become, and the copying that gets a library-owned buffer\n\
             //! into memory the caller owns.\n\n\
             const std = @import(\"std\");\n\n\
             const c = @import(\"c.zig\");\n\
             const enums = @import(\"enums.zig\");\n\n\
             const Status = enums.Status;\n\n",
        );

        out.push_str(
            "/// Every way a call in this package can fail.\n\
             ///\n\
             /// A Zig error carries no message, so the sentence that came back with the\n\
             /// last failure is read separately, with `lastErrorMessage`.\n\
             ///\n\
             /// `NoValue` is here for completeness. A call for which \"there is nothing\n\
             /// here\" is one of the answers hands back `null` instead, because that is\n\
             /// what Zig has optionals for.\n\
             pub const Error = error{\n",
        );
        out.push_str(
            "    /// An allocation the SDK made on the caller's behalf failed.\n\
             \x20   OutOfMemory,\n\
             \x20   /// An object from another document was passed to this one. A handle\n\
             \x20   /// is an index into one arena, so it would otherwise have resolved to\n\
             \x20   /// an unrelated object rather than failing.\n\
             \x20   ForeignObject,\n\
             \x20   /// The library reported something this package has no name for, which\n\
             \x20   /// means it is newer than the SDK generated against it.\n\
             \x20   Unexpected,\n",
        );
        for item in &self.api.enums {
            if item.name != "OtioStatus" {
                continue;
            }
            for variant in &item.variants {
                if variant.name == "Ok" {
                    continue;
                }
                for line in self.doc("    ", &variant.name, "means", &variant.docs, &[], None) {
                    let _ = writeln!(out, "{line}");
                }
                let _ = writeln!(out, "    {},", variant.name);
            }
        }
        out.push_str("};\n\n");

        out.push_str(
            "/// Turns a status the library reported into this package's error, and\n\
             /// keeps the message the same call wrote beside it for `lastErrorMessage`.\n\
             ///\n\
             /// It copies the message and leaves the buffer alone: the call that owns\n\
             /// the buffer frees it with a `defer`, on this path and every other.\n\
             pub fn statusError(status: Status, message: c.Buffer) Error {\n\
             \x20   remember(message);\n\
             \x20   return switch (status) {\n",
        );
        for item in &self.api.enums {
            if item.name != "OtioStatus" {
                continue;
            }
            for variant in &item.variants {
                if variant.name == "Ok" {
                    continue;
                }
                let _ = writeln!(
                    out,
                    "        .{} => Error.{},",
                    tag_name(&variant.name),
                    variant.name
                );
            }
        }
        out.push_str(
            "        // Never asked of a success: every generated call tests for it\n\
             \x20       // first. A status this package has no name for lands here too.\n\
             \x20       else => Error.Unexpected,\n\
             \x20   };\n\
             }\n\n",
        );

        out.push_str(MESSAGE);

        out.push_str(
            "/// Copies a buffer the library owns into memory the caller owns.\n\
             ///\n\
             /// Every call that answers with text or bytes goes through this, so what\n\
             /// reaches a caller is an ordinary slice from their own allocator, freed\n\
             /// with `allocator.free`, and the library's buffer is released before the\n\
             /// call returns.\n\
             pub fn copyBuffer(allocator: std.mem.Allocator, buffer: c.Buffer) Error![]u8 {\n\
             \x20   const data = buffer.data orelse return allocator.alloc(u8, 0);\n\
             \x20   return allocator.dupe(u8, data[0..buffer.len]);\n\
             }\n",
        );
        out
    }

    /// The enums, and the calls that are methods on one.
    fn enums(&self) -> Result<String, String> {
        let mut out = String::new();
        out.push_str(BANNER);
        out.push_str(
            "\n//! The enumerations the C interface hands back and takes.\n\
             //!\n\
             //! Every one of them is non-exhaustive. They cross an FFI boundary, where a\n\
             //! value the SDK has no name for is possible whenever the library is newer\n\
             //! than the package generated against it, and a non-exhaustive enum makes\n\
             //! that a value to handle rather than illegal behaviour.\n\n\
             const std = @import(\"std\");\n\n\
             const c = @import(\"c.zig\");\n\
             const support = @import(\"support.zig\");\n\
             const values = @import(\"values.zig\");\n\n\
             const Error = support.Error;\n\
             const Allocator = std.mem.Allocator;\n",
        );
        let declared: BTreeSet<String> = self
            .api
            .enums
            .iter()
            .map(|item| type_name(&item.name))
            .collect();
        out.push_str(&self.type_aliases(&declared));
        out.push('\n');

        for item in &self.api.enums {
            let zig = type_name(&item.name);
            for line in self.doc(
                "",
                &format!("A {zig}"),
                "is",
                &item.docs,
                &[],
                Some(&item.name),
            ) {
                let _ = writeln!(out, "{line}");
            }
            let _ = writeln!(out, "pub const {zig} = enum(i32) {{");
            for variant in &item.variants {
                for line in self.doc(
                    "    ",
                    &tag_name(&variant.name),
                    "means",
                    &variant.docs,
                    &[],
                    None,
                ) {
                    let _ = writeln!(out, "{line}");
                }
                let _ = writeln!(out, "    {} = {},", tag_name(&variant.name), variant.value);
            }
            out.push_str("    /// A value the library reported that this package has no name\n");
            out.push_str("    /// for, which means it is newer than the SDK.\n");
            out.push_str("    _,\n\n");

            let _ = writeln!(out, "    /// The name the C interface spells this by.");
            let _ = writeln!(out, "    pub fn cName(self: {zig}) []const u8 {{");
            let _ = writeln!(out, "        return switch (self) {{");
            for variant in &item.variants {
                let _ = writeln!(
                    out,
                    "            .{} => \"{}\",",
                    tag_name(&variant.name),
                    variant.c_name
                );
            }
            let _ = writeln!(out, "            else => \"(unknown)\",");
            let _ = writeln!(out, "        }};");
            let _ = writeln!(out, "    }}");

            for group in &self.api.groups {
                if group.receiver != Receiver::Value(item.name.clone()) {
                    continue;
                }
                for function in &group.functions {
                    if !Self::emitted(function) {
                        continue;
                    }
                    let declared = self.declare(&SelfKind::Value(item.name.clone()), function)?;
                    out.push('\n');
                    if let Some(record) = &declared.record {
                        out.push_str(&indented(record));
                        out.push('\n');
                    }
                    out.push_str(&declared.text);
                }
            }
            out.push_str("};\n\n");
        }
        Ok(out.trim_end().to_string() + "\n")
    }

    /// The value structs, laid out as the C interface lays them out, and the
    /// calls that are methods on one.
    fn values(&self) -> Result<String, String> {
        let mut out = String::new();
        out.push_str(BANNER);
        out.push_str(
            "\n//! The values the C interface passes by value.\n\
             //!\n\
             //! Each one is an `extern struct`, which Zig lays out by C's rules, so\n\
             //! these are the library's own structs rather than a copy of them that has\n\
             //! to be marshalled: a `RationalTime` crosses the boundary as itself. The\n\
             //! `comptime` blocks at the end hold that to the layout the description\n\
             //! computed, so a disagreement stops the build.\n\n\
             const std = @import(\"std\");\n\n\
             const c = @import(\"c.zig\");\n\
             const enums = @import(\"enums.zig\");\n\
             const support = @import(\"support.zig\");\n\n\
             const Error = support.Error;\n\
             const Allocator = std.mem.Allocator;\n",
        );
        let declared: BTreeSet<String> = self
            .value_structs()
            .map(|item| type_name(&item.name))
            .collect();
        out.push_str(&self.type_aliases(&declared));
        out.push('\n');

        for item in self.value_structs().collect::<Vec<_>>() {
            let zig = type_name(&item.name);
            for line in self.doc(
                "",
                &format!("A {zig}"),
                "is",
                &item.docs,
                &[],
                Some(&item.name),
            ) {
                let _ = writeln!(out, "{line}");
            }
            let _ = writeln!(out, "pub const {zig} = extern struct {{");
            for field in &item.fields {
                for line in self.doc("    ", &escape(&field.name), "is", &field.docs, &[], None) {
                    let _ = writeln!(out, "{line}");
                }
                let _ = writeln!(
                    out,
                    "    {}: {},",
                    escape(&field.name),
                    field_type(&field.ty)
                );
            }
            for group in &self.api.groups {
                if group.receiver != Receiver::Value(item.name.clone()) {
                    continue;
                }
                for function in &group.functions {
                    if !Self::emitted(function) {
                        continue;
                    }
                    let written = self.declare(&SelfKind::Value(item.name.clone()), function)?;
                    out.push('\n');
                    if let Some(record) = &written.record {
                        out.push_str(&indented(record));
                        out.push('\n');
                    }
                    out.push_str(&written.text);
                }
            }
            out.push_str("};\n\n");
        }

        out.push_str(ABI_GUARD);
        for item in self.value_structs().collect::<Vec<_>>() {
            self.layout_of(&mut out, item);
        }
        Ok(out.trim_end().to_string() + "\n")
    }
}

/// The type a field of a value struct has.
///
/// A field holding text is the one place a C string survives into the
/// surface, because these structs are the library's own and a Zig string
/// literal is already a sentinel-terminated pointer.
fn field_type(ty: &Type) -> String {
    match ty {
        Type::Text => "?[*:0]const u8".to_string(),
        other => zig_type(other),
    }
}

impl Backend<'_> {
    /// Every schema that derives from one, however far down.
    fn descendants(&self, schema: &str) -> Vec<&otio_sdk_model::Schema> {
        self.api
            .schema
            .iter()
            .filter(|candidate| {
                candidate.name != schema
                    && self
                        .api
                        .ancestry(&candidate.name)
                        .iter()
                        .any(|rung| rung.name == schema)
            })
            .collect()
    }

    /// The schema ladder, as Zig types that write one another's methods out.
    #[allow(clippy::too_many_lines)]
    fn schema(&self) -> Result<String, String> {
        let mut out = String::new();
        out.push_str(BANNER);
        out.push_str(
            "\n//! The OTIO schemas, as Zig types.\n\
             //!\n\
             //! Zig has no inheritance, and since 0.15 no `usingnamespace` to stand in\n\
             //! for it, so a method of `Item` is a method of `Clip` because this file\n\
             //! says so twice. That is the one place a generator earns its keep: the\n\
             //! duplication is free to write, free to keep correct, and it is what makes\n\
             //! `clip.duration()` work and show up in the documentation of `Clip`.\n\
             //!\n\
             //! Every one of these is the same two words — a handle, and the document to\n\
             //! resolve it against — so casting up and down the ladder copies a value\n\
             //! and touches nothing else.\n\n\
             const std = @import(\"std\");\n\n\
             const c = @import(\"c.zig\");\n\
             const enums = @import(\"enums.zig\");\n\
             const support = @import(\"support.zig\");\n\
             const values = @import(\"values.zig\");\n\n\
             const Allocator = std.mem.Allocator;\n\
             const Document = @import(\"document.zig\").Document;\n\
             const Error = support.Error;\n\
             const Metadata = @import(\"metadata.zig\").Metadata;\n",
        );
        out.push_str(&self.type_aliases(&BTreeSet::new()));
        out.push('\n');

        out.push_str(
            "/// The schema each one derives from, so that asking whether an object is\n\
             /// an Item can answer yes for a clip. The C interface has one handle and a\n\
             /// kind, which is all C can usefully offer; the ladder itself is real OTIO\n\
             /// and is declared in the description.\n\
             fn schemaParent(kind: NodeKind) ?NodeKind {\n\
             \x20   return switch (kind) {\n",
        );
        for schema in &self.api.schema {
            let Some(parent) = schema.parent.as_deref() else {
                continue;
            };
            let parent_kind = self
                .api
                .schema
                .iter()
                .find(|rung| rung.name == parent)
                .map_or(parent.to_string(), |rung| rung.kind.clone());
            let _ = writeln!(
                out,
                "        .{} => .{},",
                tag_name(&schema.kind),
                tag_name(&parent_kind)
            );
        }
        out.push_str("        else => null,\n    };\n}\n\n");

        for schema in &self.api.schema {
            let zig = schema_name(&schema.name);
            let root = schema.parent.is_none();
            let held = if root { "self" } else { "self.node" };

            let mut notes = Vec::new();
            if let Some(parent) = schema.parent.as_deref() {
                notes.push(format!(
                    "It is a {}, so every method of one is written out here too.",
                    schema_name(parent)
                ));
            }
            if root {
                notes.push(
                    "It is a small value: copying one, storing it and comparing two all \
                     work as they look."
                        .to_string(),
                );
            }
            for line in self.doc("", &format!("A {zig}"), "is", &schema.docs, &notes, None) {
                let _ = writeln!(out, "{line}");
            }
            let _ = writeln!(out, "pub const {zig} = struct {{");
            if root {
                out.push_str(
                    "    /// The document the object lives in, or null for one that\n\
                     \x20   /// belongs to none: the \"no object\" handle, or an object a cast\n\
                     \x20   /// declined to build.\n\
                     \x20   doc: ?*Document,\n\
                     \x20   /// Which object, within that document.\n\
                     \x20   handle: c.NodeHandle,\n",
                );
            } else {
                out.push_str("    /// The object this is, and the document it lives in.\n");
                out.push_str("    node: Node,\n");
            }

            // What this type builds. A constructor is not written out again
            // further down the ladder, because a Clip is not built by asking
            // Item for one.
            if let Some(group) = self.group_of(&schema.name) {
                for function in &group.functions {
                    if !Self::emitted(function) || function.role != Role::Constructor {
                        continue;
                    }
                    let written = self.declare(&SelfKind::Node(schema.name.clone()), function)?;
                    out.push('\n');
                    out.push_str(&written.text);
                }
            }

            if root {
                out.push_str(
                    "\n    /// Whether the object may be used with a document.\n\
                     \x20   ///\n\
                     \x20   /// A handle is an index into one document's arena, and two documents\n\
                     \x20   /// issue the same indices, so an object from elsewhere would resolve\n\
                     \x20   /// to an unrelated object rather than failing. Nothing in the handle\n\
                     \x20   /// says where it came from: this value does, and this is where it is\n\
                     \x20   /// used. The \"no object\" handle means no object at all, so it is\n\
                     \x20   /// allowed everywhere.\n\
                     \x20   pub fn belongsTo(self: Node, owner: ?*Document) bool {\n\
                     \x20       if (self.doc) |mine| {\n\
                     \x20           if (owner) |theirs| {\n\
                     \x20               if (mine == theirs) return true;\n\
                     \x20           }\n\
                     \x20       }\n\
                     \x20       return c.otio_node_is_none(self.handle);\n\
                     \x20   }\n\
                     \n\
                     \x20   /// Whether the object is of a schema, or of one deriving from it.\n\
                     \x20   ///\n\
                     \x20   /// An object whose document has gone, or whose handle no longer\n\
                     \x20   /// resolves, is of no schema at all, so this answers false rather\n\
                     \x20   /// than guessing.\n\
                     \x20   pub fn isA(self: Node, schema: NodeKind) bool {\n\
                     \x20       var kind = self.schemaKind() catch return false;\n\
                     \x20       while (true) {\n\
                     \x20           if (kind == schema) return true;\n\
                     \x20           kind = schemaParent(kind) orelse return false;\n\
                     \x20       }\n\
                     \x20   }\n",
                );
            } else {
                let _ = writeln!(
                    out,
                    "\n    /// Whether the object may be used with a document. See\n\
                     \x20   /// `Node.belongsTo`.\n\
                     \x20   pub fn belongsTo(self: {zig}, owner: ?*Document) bool {{\n\
                     \x20       return self.node.belongsTo(owner);\n\
                     \x20   }}\n\
                     \n\
                     \x20   /// Whether the object is of a schema, or of one deriving from it.\n\
                     \x20   /// See `Node.isA`.\n\
                     \x20   pub fn isA(self: {zig}, schema: NodeKind) bool {{\n\
                     \x20       return self.node.isA(schema);\n\
                     \x20   }}"
                );
            }

            if let Some(view) = self.view_group() {
                let name = &view.name;
                let _ = writeln!(
                    out,
                    "\n    /// The object's metadata, which is a dictionary of its own.\n\
                     \x20   ///\n\
                     \x20   /// A path names a value inside it, a step at a time, separated by\n\
                     \x20   /// dots: \"cmx_3600.reel\" reaches the reel of the dictionary the EDL\n\
                     \x20   /// adapter left behind, and \"takes[0]\" the first entry of a list.\n\
                     \x20   pub fn metadata(self: {zig}) {name} {{\n\
                     \x20       return {name}{{ .node = {} }};\n\
                     \x20   }}",
                    if root { "self" } else { "self.node" }
                );
            }

            // Everything this type answers to, its own first and then each
            // rung above it, written out again.
            for (group, function, inherited) in self.methods_on(&schema.name) {
                let Receiver::Node(declaring) = &group.receiver else {
                    continue;
                };
                let written = self.declare(&SelfKind::Node(declaring.clone()), function)?;
                out.push('\n');
                if inherited {
                    out.push_str(&self.forward(&zig, &schema_name(declaring), &written, function));
                } else {
                    if let Some(record) = &written.record {
                        out.push_str(&indented(record));
                        out.push('\n');
                    }
                    out.push_str(&written.text);
                }
            }

            // Up the ladder, which always works, and down it, which may not.
            for rung in self.api.ancestry(&schema.name).iter().skip(1) {
                let up = schema_name(&rung.name);
                let _ = writeln!(
                    out,
                    "\n    /// The same object, seen as the {up} it is.\n\
                     \x20   pub fn as{up}(self: {zig}) {up} {{\n\
                     \x20       return {};\n\
                     \x20   }}",
                    if up == "Node" {
                        "self.node".to_string()
                    } else {
                        format!("{up}{{ .node = {held} }}")
                    }
                );
            }
            for below in self.descendants(&schema.name) {
                if !below.concrete {
                    continue;
                }
                let down = schema_name(&below.name);
                let _ = writeln!(
                    out,
                    "\n    /// The object as a {down}, or null where it is not one.\n\
                     \x20   ///\n\
                     \x20   /// An object of another schema is declined rather than wrapped, so\n\
                     \x20   /// a {down} method is never called on something that is not one.\n\
                     \x20   pub fn as{down}(self: {zig}) ?{down} {{\n\
                     \x20       if (!{held}.isA(.{})) return null;\n\
                     \x20       return {down}{{ .node = {held} }};\n\
                     \x20   }}",
                    tag_name(&below.kind)
                );
            }
            out.push_str("};\n\n");
        }
        Ok(out.trim_end().to_string() + "\n")
    }
}

impl Backend<'_> {
    /// The document: the arena every object lives in.
    fn document(&self) -> Result<String, String> {
        let mut out = String::new();
        out.push_str(BANNER);
        out.push_str(
            "\n//! The document, which owns the objects in a timeline.\n\
             //!\n\
             //! Unlike the other SDKs generated from this interface, Zig's keeps the\n\
             //! document in the open. Hiding it means every object quietly owning a\n\
             //! document of its own and giving it up when it is appended, which needs a\n\
             //! finalizer and a cache of live handles to stay honest. Zig has neither,\n\
             //! and a Zig programmer already holds an arena and hands it to the things\n\
             //! that allocate from it — which is exactly what this is. ADR 0003 records\n\
             //! the reasoning.\n\n\
             const std = @import(\"std\");\n\n\
             const c = @import(\"c.zig\");\n\
             const enums = @import(\"enums.zig\");\n\
             const support = @import(\"support.zig\");\n\
             const values = @import(\"values.zig\");\n\n\
             const Allocator = std.mem.Allocator;\n\
             const Error = support.Error;\n\
             const Node = @import(\"schema.zig\").Node;\n",
        );
        out.push_str(&self.type_aliases(&BTreeSet::new()));
        out.push_str(&schema_aliases(self.api));
        out.push('\n');

        out.push_str(
            "/// One object that moved between documents, under the handle it had and\n\
             /// the handle it has now.\n\
             pub const Moved = struct {\n\
             \x20   /// The handle the object had in the document that was absorbed.\n\
             \x20   from: Node,\n\
             \x20   /// The handle it has in this one.\n\
             \x20   to: Node,\n\
             };\n\n",
        );

        out.push_str(
            "/// A Document owns every object in a timeline.\n\
             ///\n\
             /// It is the arena the core keeps its objects in, so an object is an index\n\
             /// into it rather than a pointer, and freeing the document frees the whole\n\
             /// graph at once. Removing one object leaves the handles that named it\n\
             /// stale rather than dangling, and a call made with one fails. Freeing the\n\
             /// document is different, and `deinit` says how.\n\
             ///\n\
             /// C: `OtioDocument`\n\
             pub const Document = opaque {\n",
        );

        for group in &self.api.groups {
            if group.receiver != Receiver::Document {
                continue;
            }
            for function in &group.functions {
                if !Self::emitted(function) || function.role != Role::Constructor {
                    continue;
                }
                let written = self.declare(&SelfKind::Document, function)?;
                out.push_str(&written.text);
                out.push('\n');
            }
        }

        out.push_str(
            "    /// Releases the document and every object in it.\n\
             \x20   ///\n\
             \x20   /// It invalidates every node of this document, and a node does not\n\
             \x20   /// know that: it holds this pointer, so calling anything on one\n\
             \x20   /// afterwards hands a freed pointer back to the library. That is a\n\
             \x20   /// use-after-free, not a `StaleHandle` — the generation check inside\n\
             \x20   /// a handle guards a slot that has been reused, which needs the\n\
             \x20   /// arena to still be there. Freeing twice is the same mistake.\n\
             \x20   ///\n\
             \x20   /// So a node lives no longer than the document it came from. The\n\
             \x20   /// usual `defer document.deinit()` at the point the document is\n\
             \x20   /// opened gives exactly that, and is why nothing here tracks it.\n\
             \x20   ///\n\
             \x20   /// C: `otio_document_free`\n\
             \x20   pub fn deinit(self: *Document) void {\n\
             \x20       c.otio_document_free(self);\n\
             \x20   }\n\n",
        );

        for group in &self.api.groups {
            if group.receiver != Receiver::Document {
                continue;
            }
            for function in &group.functions {
                if !Self::emitted(function)
                    || matches!(function.role, Role::Constructor | Role::Free)
                {
                    continue;
                }
                let written = self.declare(&SelfKind::Document, function)?;
                if let Some(record) = &written.record {
                    out.push_str(&indented(record));
                    out.push('\n');
                }
                out.push_str(&written.text);
                out.push('\n');
            }
        }

        out.push_str(SAVE.trim_start_matches('\n'));
        out.push('\n');
        out.push_str(ABSORB.trim_start_matches('\n'));
        out.push_str("};\n");
        Ok(out)
    }

    /// The metadata dictionary every object carries.
    fn metadata(&self) -> Result<String, String> {
        let Some(group) = self.view_group() else {
            return Err("the description has no metadata view".to_string());
        };
        let name = &group.name;
        let mut out = String::new();
        out.push_str(BANNER);
        out.push_str(
            "\n//! The metadata dictionary every named object carries.\n\
             //!\n\
             //! It is a view rather than part of the object, so that `len` and\n\
             //! `contains` do not sit beside `name` and `duration`, where they would\n\
             //! read as if they were about the object itself.\n\n\
             const std = @import(\"std\");\n\n\
             const c = @import(\"c.zig\");\n\
             const enums = @import(\"enums.zig\");\n\
             const support = @import(\"support.zig\");\n\
             const values = @import(\"values.zig\");\n\n\
             const Allocator = std.mem.Allocator;\n\
             const Document = @import(\"document.zig\").Document;\n\
             const Error = support.Error;\n\
             const Node = @import(\"schema.zig\").Node;\n",
        );
        out.push_str(&self.type_aliases(&BTreeSet::new()));
        out.push('\n');

        let notes = vec![
            "A path names a value inside it, a step at a time, separated by dots: \
             \"cmx_3600.reel\" reaches the reel of the dictionary the EDL adapter left \
             behind, and \"takes[0]\" the first entry of a list."
                .to_string(),
            "A path is followed, not created. Writing one step deep always works, but a \
             deeper one needs its dictionary to exist first: call setDictionary(\
             \"cmx_3600\") before setString(\"cmx_3600.reel\", \"ZZ100\")."
                .to_string(),
        ];
        for line in self.doc("", &format!("A {name}"), "is", &group.docs, &notes, None) {
            let _ = writeln!(out, "{line}");
        }
        let _ = writeln!(out, "pub const {name} = struct {{");
        out.push_str("    /// The object whose dictionary this is.\n");
        out.push_str("    node: Node,\n");
        for function in &group.functions {
            if !Self::emitted(function) {
                continue;
            }
            let written = self.declare(&SelfKind::View(name.clone()), function)?;
            out.push('\n');
            if let Some(record) = &written.record {
                out.push_str(&indented(record));
                out.push('\n');
            }
            out.push_str(&written.text);
        }
        out.push_str("};\n");
        Ok(out)
    }

    /// The package's own file: what it is, and everything it offers.
    fn root(&self) -> Result<String, String> {
        let mut out = String::new();
        out.push_str(BANNER);
        out.push_str(PACKAGE_DOC);
        out.push_str(
            "\nconst std = @import(\"std\");\n\n\
             const c = @import(\"c.zig\");\n\
             const enums = @import(\"enums.zig\");\n\
             const schema = @import(\"schema.zig\");\n\
             const support = @import(\"support.zig\");\n\
             const values = @import(\"values.zig\");\n\n\
             const Allocator = std.mem.Allocator;\n\n",
        );

        out.push_str("/// Every way a call in this package can fail.\n");
        out.push_str("pub const Error = support.Error;\n");
        out.push_str("/// The sentence that came back with the last failure on this thread.\n");
        out.push_str("pub const lastErrorMessage = support.lastErrorMessage;\n\n");
        out.push_str("/// The arena a timeline's objects live in.\n");
        out.push_str("pub const Document = @import(\"document.zig\").Document;\n");
        out.push_str("/// One object that moved between documents.\n");
        out.push_str("pub const Moved = @import(\"document.zig\").Moved;\n");
        if let Some(view) = self.view_group() {
            let _ = writeln!(
                out,
                "/// The metadata dictionary every named object carries."
            );
            let _ = writeln!(
                out,
                "pub const {0} = @import(\"metadata.zig\").{0};",
                view.name
            );
        }
        out.push('\n');
        for item in &self.api.schema {
            let zig = schema_name(&item.name);
            let _ = writeln!(out, "pub const {zig} = schema.{zig};");
        }
        out.push('\n');
        for item in self.value_structs().collect::<Vec<_>>() {
            let zig = type_name(&item.name);
            let _ = writeln!(out, "pub const {zig} = values.{zig};");
        }
        out.push('\n');
        for item in &self.api.enums {
            let zig = type_name(&item.name);
            let _ = writeln!(out, "pub const {zig} = enums.{zig};");
        }

        out.push_str(OPEN);

        for group in &self.api.groups {
            for function in &group.functions {
                if !Self::emitted(function) || owner_of(group, function) != "package" {
                    continue;
                }
                let written = self.declare(&SelfKind::None, function)?;
                out.push('\n');
                if let Some(record) = &written.record {
                    out.push_str(record);
                    out.push('\n');
                }
                // These stand at the top of the package rather than inside a
                // type, so the indentation a declaration is written with has
                // to come back off.
                for line in written.text.lines() {
                    let _ = writeln!(out, "{}", line.strip_prefix("    ").unwrap_or(line));
                }
            }
        }
        Ok(out)
    }
}

/// `const Clip = schema.Clip;` for every rung of the ladder.
fn schema_aliases(api: &Api) -> String {
    let mut out = String::new();
    for item in &api.schema {
        let zig = schema_name(&item.name);
        if zig != "Node" {
            let _ = writeln!(out, "const {zig} = @import(\"schema.zig\").{zig};");
        }
    }
    out
}

/// Writing a document out under a name that says what format it is.
const SAVE: &str = r#"
    /// Writes the document to a file, working out its format from the name.
    ///
    /// It is the short way to say writeToFile, as `open` is for
    /// readFromFile. Where the suffix belongs to no format it answers
    /// `error.NoValue`.
    pub fn save(self: *Document, path: [:0]const u8) Error!void {
        const format = (try @import("root.zig").formatOf(path)) orelse return Error.NoValue;
        return self.writeToFile(format, path, null);
    }
"#;

/// Where the message that came back with a failure is kept until it is asked
/// for.
///
/// Every fallible call hands its message back beside its status, so the
/// message belongs to the call and not to whatever thread the library ran it
/// on. A Zig error has no room for it, though, and the one public way to read
/// it, `lastErrorMessage`, takes nothing, so the package keeps a copy itself.
/// It is kept per thread because a Zig thread is an operating-system thread
/// and nothing moves a caller between two of them, so "the last failure on
/// this thread" is exactly "the last failure this caller saw".
///
/// It is a copy in a fixed buffer rather than the library's buffer held on
/// to, so that every buffer the library hands back is freed by the call that
/// received it and nothing is left owed when a thread ends. The buffer is
/// large enough for every message the library writes; a longer one keeps its
/// beginning, cut where a character starts rather than in the middle of one.
const MESSAGE: &str = r#"/// How much of a message is kept.
const message_capacity = 4096;

/// The message that came back with the last failure on this thread.
threadlocal var message_held: [message_capacity]u8 = undefined;
/// How much of `message_held` is the message.
threadlocal var message_len: usize = 0;

/// Keeps a copy of the message a call handed back, replacing the last one.
fn remember(message: c.Buffer) void {
    const data = message.data orelse {
        message_len = 0;
        return;
    };
    var len = @min(message.len, message_capacity);
    // A cut in the middle of a character would hand a caller text that is
    // not UTF-8, so it moves back to where the character began.
    if (len < message.len) {
        while (len > 0 and data[len] & 0xC0 == 0x80) len -= 1;
    }
    @memcpy(message_held[0..len], data[0..len]);
    message_len = len;
}

/// The sentence that came back with the last failure on this thread.
///
/// A Zig error is a value with no room for a message, so this is where the
/// detail is. The failing call wrote it beside its status, and the package
/// kept a copy for the thread that made the call, so no other thread's
/// failure can take its place. It is worth reading before the next call
/// fails, which replaces it; an error the package raises itself, such as
/// `ForeignObject`, never reached the library and leaves it as it was.
///
/// The slice points into storage the package owns and is valid until the
/// next failure on this thread.
pub fn lastErrorMessage() []const u8 {
    return message_held[0..message_len];
}

"#;

/// Moving objects between documents, which no backend can emit mechanically.
const ABSORB: &str = r#"
    /// Moves every object in another document into this one.
    ///
    /// It is how an object built on its own joins a timeline: build a clip
    /// in a document of its own, absorb that document into the one holding
    /// the timeline, and append the clip where it belongs. A handle means
    /// nothing outside the document it was issued for, so the objects are
    /// moved rather than pointed at, and every one of them arrives under a
    /// new handle.
    ///
    /// source is consumed. On success the document it named has been freed
    /// and source.* is set to null, so a defer that frees it does the right
    /// thing, and the answer gives the new handle for each one that moved.
    /// On failure nothing moves and source is left alone. The source's root
    /// is not adopted, because this document has its own.
    ///
    /// What this hands back was allocated with allocator, and is the
    /// caller's to free.
    ///
    /// C: `otio_document_absorb`
    pub fn absorb(self: *Document, allocator: Allocator, source: *?*Document) Error![]Moved {
        const absorbed = source.* orelse return Error.NullPointer;
        // The call cannot be asked twice to size its answer, because the
        // first ask would already have consumed the source. How many objects
        // the source holds is exactly how many will move.
        const moving = c.otio_document_node_count(absorbed);
        const from = try allocator.alloc(c.NodeHandle, moving);
        defer allocator.free(from);
        const to = try allocator.alloc(c.NodeHandle, moving);
        defer allocator.free(to);
        // Allocated before the call, not after it. Once the call has run
        // the source is gone and the objects are in here, so an allocation
        // that failed at that point would lose the only record of where
        // they went. Failing now costs nothing, because nothing has moved.
        const moved = try allocator.alloc(Moved, moving);
        errdefer allocator.free(moved);
        var count: usize = 0;
        var out_error: c.Buffer = .{ .data = null, .len = 0 };
        defer c.otio_buffer_free(out_error);
        const status = c.otio_document_absorb(
            self,
            source,
            if (from.len > 0) from.ptr else null,
            if (to.len > 0) to.ptr else null,
            moving,
            &count,
            &out_error,
        );
        if (status != .ok) return support.statusError(status, out_error);
        // Every object in the source moves, and that is what `moving`
        // counted, so a different number means the library disagrees with
        // the interface this was generated from. The objects have still
        // moved; there is just no sound table to describe it with.
        if (count != moving) return Error.Unexpected;
        for (from, to, moved) |was, now, *entry| {
            entry.* = .{
                .from = Node{ .doc = absorbed, .handle = was },
                .to = Node{ .doc = self, .handle = now },
            };
        }
        return moved;
    }
"#;

/// The package's own documentation, which is the first thing anyone reads.
const PACKAGE_DOC: &str = r#"
//! Read, write and edit OpenTimelineIO timelines.
//!
//! This package is generated from the C interface of the otio-rust core, so
//! it carries the whole data model: the schemas, the composition algorithms,
//! the ten edit operations and the file-format adapters.
//!
//! ## Documents
//!
//! Everything lives in a `Document`, which owns the objects in it. It is an
//! arena, and it is held in the open the way a Zig programmer holds any
//! other arena:
//!
//! ```zig
//! const document = try otio.Document.readFromFile(.cmx3600, "cut.edl", null);
//! defer document.deinit();
//!
//! const root = (try document.root()) orelse return error.Empty;
//! const clips = try root.findClips(allocator);
//! defer allocator.free(clips);
//! ```
//!
//! ## Objects
//!
//! An object is a `Node`: a handle, and the document it can be resolved
//! against. The OTIO schemas are Zig types holding one, and each writes out
//! the methods of every schema it derives from, so a `Clip` answers to
//! everything an `Item`, a `Composable` and a `Node` answer to. Ask a node
//! what it is with its `as` method:
//!
//! ```zig
//! if (node.asClip()) |clip| {
//!     const reference = try clip.mediaReference(null);
//! }
//! ```
//!
//! Asking an object for something it does not have fails rather than
//! answering with a zero value: a clip asked for a track's kind reports
//! `error.InvalidArgument`.
//!
//! ## Memory
//!
//! Anything the library hands back that has to be freed — a name, a JSON
//! document, a list of children — is copied into an allocator you pass, and
//! freed with `allocator.free`. A call that needs one takes it as its first
//! argument after the receiver, which is the only signal you need that it
//! allocates.
//!
//! ## Errors
//!
//! A call that can fail answers with an error union over `Error`. Where
//! "there is nothing here" is one of the answers — an item with no source
//! range, a clip with no active media reference — the answer is an optional
//! rather than an error, because that is what Zig has optionals for:
//!
//! ```zig
//! if (try clip.sourceRange()) |span| {
//!     // the clip is trimmed to span
//! } else {
//!     // it is untrimmed
//! }
//! ```
//!
//! A Zig error carries no message. The sentence the failing call handed
//! back with its status is kept, and read with `lastErrorMessage` on the
//! thread that made the call.
//!
//! ## Optional arguments
//!
//! Where the C interface accepts nothing at all, this package takes `null`:
//! `Clip.init(document, null)` makes a clip with no name, and
//! `clip.mediaReference(null)` asks for the active one.
"#;

/// Reading a document from a file whose name says what format it is.
const OPEN: &str = r#"
/// The format a filename claims, or null where its suffix names none.
///
/// The suffix is copied onto the stack on the way past, because the C
/// interface wants it terminated and a slice of a path is not.
pub fn formatOf(path: []const u8) Error!?Format {
    const extension = std.fs.path.extension(path);
    const suffix = if (extension.len > 0) extension[1..] else extension;
    var held: [32]u8 = undefined;
    if (suffix.len >= held.len) return null;
    @memcpy(held[0..suffix.len], suffix);
    held[suffix.len] = 0;
    return Format.fromSuffix(held[0..suffix.len :0]);
}

/// Reads a document from a file, working out its format from the name.
///
/// It is the short way to say Document.readFromFile when the suffix already
/// says what the file holds, which is how upstream's read_from_file behaves
/// when no adapter is named. Where the suffix belongs to no format it
/// answers `error.NoValue`.
pub fn open(path: [:0]const u8) Error!*Document {
    const format = (try formatOf(path)) orelse return Error.NoValue;
    return Document.readFromFile(format, path, null);
}
"#;

/// The build script, which is the same whatever the interface says.
fn build_zig() -> String {
    format!(
        "{BANNER}{}",
        r#"
//! Builds the OpenTimelineIO package and runs its tests.
//!
//! The package is Zig over the C ABI of the otio-rust core, and it links
//! against a static library it expects to find in `lib/`:
//!
//! ```sh
//! cargo build -p otio-capi --release
//! cp target/release/libotio.a sdk/zig/lib/
//! zig build test
//! ```

const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const library = b.option(
        []const u8,
        "library",
        "Where libotio.a is, if not lib/",
    ) orelse "lib";

    const otio = b.addModule("otio", .{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
        .optimize = optimize,
        .link_libc = true,
    });
    otio.addLibraryPath(.{ .cwd_relative = library });
    otio.linkSystemLibrary("otio", .{});
    // What the Rust standard library needs underneath it. A Rust panic
    // unwinds, and the unwinder is not in libc: on Darwin it comes with the
    // system, and everywhere else Zig's own is asked for by name.
    if (target.result.os.tag.isDarwin()) {
        otio.linkFramework("CoreFoundation", .{});
        otio.linkFramework("Security", .{});
        otio.linkSystemLibrary("iconv", .{});
    } else {
        otio.linkSystemLibrary("unwind", .{});
    }

    const tests = b.addTest(.{
        .root_module = b.createModule(.{
            .root_source_file = b.path("test/suite.zig"),
            .target = target,
            .optimize = optimize,
            .imports = &.{.{ .name = "otio", .module = otio }},
        }),
    });
    const run = b.addRunArtifact(tests);
    run.has_side_effects = true;

    const step = b.step("test", "Run the package's tests");
    step.dependOn(&run.step);
}
"#
    )
}

/// The package manifest.
fn build_zon(version: &str) -> String {
    format!(
        ".{{\n\
         \x20   .name = .otio,\n\
         \x20   .version = \"{version}\",\n\
         \x20   .minimum_zig_version = \"{ZIG_VERSION}\",\n\
         \x20   .fingerprint = 0x78cb54e25efa10a8,\n\
         \x20   .paths = .{{\n\
         \x20       \"build.zig\",\n\
         \x20       \"build.zig.zon\",\n\
         \x20       \"src\",\n\
         \x20       \"test\",\n\
         \x20       \"README.md\",\n\
         \x20   }},\n\
         }}\n"
    )
}
