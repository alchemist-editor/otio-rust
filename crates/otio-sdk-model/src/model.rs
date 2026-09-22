//! What the generators see.
//!
//! The C ABI is flat by necessity: every object is a handle, every failure is
//! a status code, every list is a two-pass call, and every relationship
//! between functions lives in their names. This module is the same interface
//! with those relationships written down — which function is a constructor,
//! what a call is a method *on*, which parameters a caller actually supplies
//! and which exist only to receive a result.
//!
//! A backend that walks this can emit a method on a `Clip` returning a
//! `[]Clip` and an `error`. A backend given only the C header can emit a free
//! function taking six pointers.

use std::collections::BTreeMap;

/// The whole interface.
#[derive(Debug, Clone)]
pub struct Api {
    /// The version of the library the description was taken from.
    pub version: String,
    /// Every `#[repr(C)]` enum.
    pub enums: Vec<Enum>,
    /// Every `#[repr(C)]` struct that crosses the boundary by value.
    pub structs: Vec<Struct>,
    /// The OTIO schema hierarchy, which the flat C ABI cannot express.
    pub schema: Vec<Schema>,
    /// The entry points, gathered into the types they belong to.
    pub groups: Vec<Group>,
}

impl Api {
    /// Finds a group by name.
    #[must_use]
    pub fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|group| group.name == name)
    }

    /// Finds a struct by its C name.
    #[must_use]
    pub fn structure(&self, name: &str) -> Option<&Struct> {
        self.structs.iter().find(|item| item.name == name)
    }

    /// Finds an enum by its C name.
    #[must_use]
    pub fn enumeration(&self, name: &str) -> Option<&Enum> {
        self.enums.iter().find(|item| item.name == name)
    }

    /// The variant of an enum whose discriminant is zero: what a zeroed
    /// field of that enum holds.
    #[must_use]
    pub fn zero_variant(&self, name: &str) -> Option<&Variant> {
        self.enumeration(name)?
            .variants
            .iter()
            .find(|variant| variant.value == 0)
    }

    /// Every function in the interface, whatever group it landed in.
    pub fn functions(&self) -> impl Iterator<Item = &Function> {
        self.groups.iter().flat_map(|group| group.functions.iter())
    }

    /// Maps each schema to the schemas that derive from it, directly.
    #[must_use]
    pub fn subtypes(&self) -> BTreeMap<&str, Vec<&Schema>> {
        let mut map: BTreeMap<&str, Vec<&Schema>> = BTreeMap::new();
        for schema in &self.schema {
            if let Some(parent) = schema.parent.as_deref() {
                map.entry(parent).or_default().push(schema);
            }
        }
        map
    }

    /// Walks a schema and everything it derives from, nearest first.
    #[must_use]
    pub fn ancestry(&self, name: &str) -> Vec<&Schema> {
        let mut chain = Vec::new();
        let mut current = Some(name);
        while let Some(step) = current {
            let Some(schema) = self.schema.iter().find(|schema| schema.name == step) else {
                break;
            };
            chain.push(schema);
            current = schema.parent.as_deref();
        }
        chain
    }
}

/// One rung of the OTIO schema ladder.
///
/// The C ABI has a single `OtioNode` handle and an `OtioNodeKind` telling you
/// what it points at, which is all C can usefully offer. A language with
/// types wants `Clip` and `Track`, and wants to know that both are `Item`s.
/// That relationship is real OTIO, not something the C ABI carries, so it is
/// declared once and checked against `OtioNodeKind` — a schema added to the
/// core cannot reach the SDKs without being placed here first.
#[derive(Debug, Clone)]
pub struct Schema {
    /// The schema's name, such as `Clip`.
    pub name: String,
    /// The `OtioNodeKind` variant that names it.
    pub kind: String,
    /// The schema it derives from, if any.
    pub parent: Option<String>,
    /// Whether a file can legitimately hold one of exactly this schema, as
    /// opposed to it existing only as something to derive from.
    pub concrete: bool,
    /// What it is, for the generated documentation.
    pub docs: Docs,
}

/// A `#[repr(C)]` enum.
#[derive(Debug, Clone)]
pub struct Enum {
    /// Its C name, such as `OtioStatus`.
    pub name: String,
    /// What it is.
    pub docs: Docs,
    /// Its variants, in declaration order.
    pub variants: Vec<Variant>,
}

/// One variant of an enum.
#[derive(Debug, Clone)]
pub struct Variant {
    /// Its Rust name, such as `NullPointer`.
    pub name: String,
    /// Its C name, such as `OTIO_STATUS_NULL_POINTER`.
    pub c_name: String,
    /// Its discriminant.
    pub value: i64,
    /// What it means.
    pub docs: Docs,
}

/// A `#[repr(C)]` struct that crosses the boundary by value.
#[derive(Debug, Clone)]
pub struct Struct {
    /// Its C name, such as `OtioRationalTime`.
    pub name: String,
    /// What it is.
    pub docs: Docs,
    /// Its fields, in declaration order.
    pub fields: Vec<Field>,
    /// How big it is and what it is aligned to.
    pub layout: Layout,
    /// Whether it is plumbing the SDKs hide rather than a value they expose.
    ///
    /// `OtioBuffer` is how the library hands back a string; no SDK should
    /// make its users think about one.
    pub plumbing: bool,
}

impl Struct {
    /// Whether a caller fills in only the fields it cares about.
    ///
    /// The options structs are documented so that zero is the usual
    /// behaviour for every field, and they gain fields as formats gain
    /// options. A target that makes a caller spell every field — Zig's
    /// struct literals, Swift's and C#'s constructors — gives these fields a
    /// zero default, so that a new field is not a break for every caller
    /// that builds one.
    #[must_use]
    pub fn fields_default_to_zero(&self) -> bool {
        self.name.ends_with("Options")
    }
}

/// One field of a struct.
#[derive(Debug, Clone)]
pub struct Field {
    /// Its name.
    pub name: String,
    /// Its type.
    pub ty: Type,
    /// Where it sits in the struct, in bytes from the start.
    pub offset: ByWidth,
    /// What it holds.
    pub docs: Docs,
}

/// A number that depends on how wide a pointer is on the target.
///
/// A struct holding no pointer has the same layout everywhere, and both
/// numbers agree. `OtioReadOptions` holds a `const char *`, so it does not —
/// and a target compiled to `wasm32` reads the 32-bit one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByWidth {
    /// On a target whose pointers are four bytes, such as `wasm32`.
    pub pointer32: usize,
    /// On a target whose pointers are eight bytes.
    pub pointer64: usize,
}

impl ByWidth {
    /// Reads whichever number applies to a pointer of `width` bytes.
    ///
    /// # Panics
    ///
    /// Panics on any width other than four or eight, which this ABI has no
    /// target for.
    #[must_use]
    pub fn at(self, width: usize) -> usize {
        match width {
            4 => self.pointer32,
            8 => self.pointer64,
            other => panic!("no target has {other}-byte pointers"),
        }
    }
}

/// How big a struct is, and what it is aligned to.
///
/// A generator that marshals a struct field by field — which a target
/// without a C compiler, such as one going through WebAssembly, has to do —
/// needs these and the field offsets rather than just the field names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// Its size in bytes, padding included.
    pub size: ByWidth,
    /// What it is aligned to, in bytes.
    pub align: ByWidth,
}

/// The type a value has as it crosses the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// A `bool`.
    Bool,
    /// A `double`.
    Double,
    /// A signed 64-bit integer.
    Int64,
    /// An unsigned 64-bit integer.
    Uint64,
    /// A signed 32-bit integer.
    Int32,
    /// An unsigned 32-bit integer.
    Uint32,
    /// A `size_t`.
    Size,
    /// UTF-8 text.
    Text,
    /// A borrowed run of bytes.
    Bytes,
    /// A handle to an object in a document.
    Node,
    /// A document.
    Document,
    /// One of the `#[repr(C)]` structs, by value.
    Struct(String),
    /// One of the `#[repr(C)]` enums.
    Enum(String),
    /// Several of something.
    List(Box<Type>),
}

impl Type {
    /// The name of the C type this is spelled with.
    #[must_use]
    pub fn c_name(&self) -> String {
        match self {
            Self::Bool => "bool".to_string(),
            Self::Double => "double".to_string(),
            Self::Int64 => "int64_t".to_string(),
            Self::Uint64 => "uint64_t".to_string(),
            Self::Int32 => "int32_t".to_string(),
            Self::Uint32 => "uint32_t".to_string(),
            Self::Size => "size_t".to_string(),
            Self::Text => "const char *".to_string(),
            Self::Bytes => "const uint8_t *".to_string(),
            Self::Node => "OtioNode".to_string(),
            Self::Document => "OtioDocument *".to_string(),
            Self::Struct(name) | Self::Enum(name) => name.clone(),
            Self::List(inner) => format!("{} *", inner.c_name()),
        }
    }
}

/// A set of entry points that belong to one type in the generated SDKs.
#[derive(Debug, Clone)]
pub struct Group {
    /// The type's name in the SDKs, such as `Clip`.
    pub name: String,
    /// The `otio_` symbol prefixes that feed it.
    pub prefixes: Vec<String>,
    /// What a call in this group is a method on.
    pub receiver: Receiver,
    /// Whether the group is a view of its receiver rather than part of it.
    ///
    /// The thirty-odd metadata calls are one dictionary API that every object
    /// carries. Folding them into every type would put `len` and `contains`
    /// beside `name` and `duration`, where they read as if they were about
    /// the object. So a backend hangs them off a view — `clip.metadata()` —
    /// and their names never collide with the object's own.
    pub view: bool,
    /// What the type is.
    pub docs: Docs,
    /// Its entry points, in symbol order.
    pub functions: Vec<Function>,
}

/// What the calls in a group are methods on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Receiver {
    /// Nothing: these are free functions.
    None,
    /// The document.
    Document,
    /// An object in a document, of the named schema or one deriving from it.
    Node(String),
    /// One of the value structs, by value.
    Value(String),
}

/// One entry point.
#[derive(Debug, Clone)]
pub struct Function {
    /// The exported symbol, such as `otio_clip_media_reference`.
    pub symbol: String,
    /// The symbol with its group's prefix removed, such as
    /// `media_reference`. This is what a backend spells in its own case.
    pub name: String,
    /// What part the call plays in its group.
    pub role: Role,
    /// Every C parameter, in order, labelled with the part it plays.
    pub params: Vec<Param>,
    /// What the C function returns.
    pub result: CResult,
    /// What a caller of the generated SDK gets back, in order.
    pub outputs: Vec<Output>,
    /// The call that says how long this one's list will be, for a list call
    /// that also edits the document.
    ///
    /// A two-pass list call asks for the length, then asks again for the
    /// contents. That only works where asking twice is free. A call that
    /// edits as it answers — clearing a composition's children and handing
    /// them back — has nothing left to give on the second pass, so the buffer
    /// has to be the right size the first time. This names the call that
    /// sizes it.
    pub sized_by: Option<String>,
    /// Whether "there is no value" is one of this call's answers.
    ///
    /// `OTIO_STATUS_NO_VALUE` is not a failure: an item with no source range
    /// reports it. A language with a nullable or optional type should use it
    /// here rather than raising.
    pub optional: bool,
    /// What the call does.
    pub docs: Docs,
}

impl Function {
    /// The parameters a caller of the generated SDK supplies.
    pub fn inputs(&self) -> impl Iterator<Item = &Param> {
        self.params
            .iter()
            .filter(|param| matches!(param.role, ParamRole::Input | ParamRole::Bytes))
    }

    /// The parameter the call is a method on, if it has one.
    #[must_use]
    pub fn receiver(&self) -> Option<&Param> {
        self.params
            .iter()
            .find(|param| param.role == ParamRole::Receiver)
    }

    /// Whether the call reports failure through an `OtioStatus`.
    #[must_use]
    pub fn fallible(&self) -> bool {
        self.result == CResult::Status
    }

    /// The parameter the call writes its failure message to, which every
    /// fallible call has and no other call does.
    #[must_use]
    pub fn error(&self) -> Option<&Param> {
        self.params
            .iter()
            .find(|param| param.role == ParamRole::Error)
    }
}

/// What part an entry point plays in its group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// It builds a new object.
    Constructor,
    /// It reads a property.
    Getter,
    /// It writes a property.
    Setter,
    /// It unsets a property.
    Clearer,
    /// It releases something.
    Destructor,
    /// It does something else to its receiver.
    Method,
    /// It belongs to no object.
    Free,
    /// It exists for the generated code rather than for the people using it:
    /// freeing a buffer, naming a status. A backend calls these; it does not
    /// surface them.
    Plumbing,
}

/// One C parameter and the part it plays.
#[derive(Debug, Clone)]
pub struct Param {
    /// Its name in the C ABI.
    pub name: String,
    /// What it is for.
    pub role: ParamRole,
    /// The type it carries.
    pub ty: Type,
    /// Whether the call accepts "nothing" here.
    ///
    /// For a pointer that is a null pointer, and for an `OtioNode` that is
    /// `otio_node_none()`.
    pub optional: bool,
    /// What it is, from the doc comment, when the comment says.
    pub docs: Docs,
    /// What the call does with it, when it is an object and the answer
    /// matters.
    ///
    /// `None` for everything that is not an object the caller hands over: a
    /// number, a string, an out-parameter. See [`Placement`].
    pub placement: Option<Placement>,
    /// Whether this is the object whose document the call works in.
    ///
    /// A binding that hides the document has to get one from somewhere, and
    /// the only place left is the objects it was handed. Exactly one object
    /// argument of a call can be that one, and which it is is not free
    /// choice: see `placement.rs`.
    pub anchor: bool,
}

/// What an editing call does with an object handed to it.
///
/// A binding that hides the document has to decide, for each object
/// argument, whether the call is putting that object somewhere — in which
/// case it must first be moved into the receiver's document — or only
/// naming one that has to be there already. Neither answer is safe as a
/// default and neither fails loudly, so the description states it per
/// parameter; see `placement.rs` for why it cannot be read off the
/// signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// The call puts the object into the document, so a binding moves it
    /// there first.
    Adopt,
    /// The call only names the object, so a binding refuses one that belongs
    /// to another document rather than dragging it over.
    Require,
}

/// What part a C parameter plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamRole {
    /// The document the call reads.
    DocumentIn,
    /// The document the call edits.
    DocumentMut,
    /// A document the call consumes.
    ///
    /// It arrives as a pointer to the caller's own pointer, because on
    /// success the call frees the document and sets that pointer to null, so
    /// the caller is left with nothing to free. `otio_document_absorb` is the
    /// one call that takes one.
    DocumentTaken,
    /// The object the call is a method on.
    Receiver,
    /// An argument the caller supplies.
    Input,
    /// The pointer half of a borrowed run of bytes; the next parameter is its
    /// length.
    Bytes,
    /// The length of whatever the parameter before it lends: a run of bytes,
    /// or a list.
    Length,
    /// Where one result is written.
    Output,
    /// Where a list of results is written; the next two parameters are how
    /// much room it has and how many there turned out to be.
    OutputList,
    /// How much room a list buffer has.
    ListCapacity,
    /// Where the length of a list is written.
    OutputCount,
    /// Where a call that can fail writes the sentence saying why.
    ///
    /// Every call that returns a status takes one, last, as an `OtioBuffer`
    /// the caller frees. A binding passes one on every call and builds its
    /// error from the status and what was written there; the message never
    /// comes from a second call, so it does not matter which thread a
    /// binding's runtime ran the call on.
    Error,
}

/// What the C function itself returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CResult {
    /// Nothing.
    Void,
    /// An `OtioStatus`: the call can fail, and its results come back through
    /// out-parameters.
    Status,
    /// A value, directly. These calls cannot fail.
    Value(Type),
    /// A pointer to text the library owns forever and the caller never frees.
    StaticText,
}

/// One of the things a caller of the generated SDK gets back.
#[derive(Debug, Clone)]
pub struct Output {
    /// A name for it, taken from the out-parameter with its `out_` removed.
    pub name: String,
    /// What it is.
    pub ty: Type,
    /// Whether it arrives as an `OtioBuffer` the SDK must free after copying.
    pub owned_buffer: bool,
}

/// A doc comment, and the interface symbols it mentions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Docs {
    /// The first sentence, which most languages want on its own.
    pub summary: String,
    /// The rest, as paragraphs.
    pub body: Vec<String>,
    /// Every `otio_` function and `OTIO_` constant the text mentions, so a
    /// backend can rewrite them into its own spelling rather than leaving C
    /// names in a Go or Swift doc comment.
    pub references: Vec<String>,
}

impl Docs {
    /// Whether there is anything to say at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.summary.is_empty() && self.body.is_empty()
    }
}
