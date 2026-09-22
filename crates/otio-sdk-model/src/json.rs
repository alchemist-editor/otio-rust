//! Writing the description out as JSON.
//!
//! The file this produces is committed, so a change to the C ABI shows up in
//! review as a change to the API surface — which functions appeared, which
//! arguments moved — rather than only as a change to the Rust that implements
//! it. It is also what a generator written in some other language would read,
//! so it is plain JSON with no cleverness in it.
//!
//! The output is deterministic: everything is in a fixed order and the
//! formatting is fixed, so the only diffs are real ones.

use std::fmt::Write as _;

use crate::model::{
    Api, ByWidth, CResult, Docs, Layout, Param, ParamRole, Placement, Receiver, Role, Type,
};

/// Renders the description as pretty-printed JSON, ending in a newline.
#[must_use]
pub fn render(api: &Api) -> String {
    let mut out = String::new();
    let mut object = Object::new(&mut out, 0);
    object.note("This file is generated from crates/otio-capi/src by otio-sdk-model.");
    object.note("Edit the C ABI, then run `cargo run -p otio-sdk-gen`.");
    object.string("version", &api.version);

    object.array("enums", |array| {
        for item in &api.enums {
            array.object(|object| {
                object.string("name", &item.name);
                object.docs("docs", &item.docs);
                object.array("variants", |array| {
                    for variant in &item.variants {
                        array.object(|object| {
                            object.string("name", &variant.name);
                            object.string("c_name", &variant.c_name);
                            object.number("value", variant.value);
                            object.docs("docs", &variant.docs);
                        });
                    }
                });
            });
        }
    });

    object.array("structs", |array| {
        for item in &api.structs {
            array.object(|object| {
                object.string("name", &item.name);
                object.docs("docs", &item.docs);
                object.boolean("plumbing", item.plumbing);
                object.layout("layout", &item.layout);
                object.array("fields", |array| {
                    for field in &item.fields {
                        array.object(|object| {
                            object.string("name", &field.name);
                            object.ty("type", &field.ty);
                            object.by_width("offset", field.offset);
                            object.docs("docs", &field.docs);
                        });
                    }
                });
            });
        }
    });

    object.array("schema", |array| {
        for schema in &api.schema {
            array.object(|object| {
                object.string("name", &schema.name);
                object.string("kind", &schema.kind);
                match schema.parent.as_deref() {
                    Some(parent) => object.string("parent", parent),
                    None => object.null("parent"),
                }
                object.boolean("concrete", schema.concrete);
                object.docs("docs", &schema.docs);
            });
        }
    });

    object.array("groups", |array| {
        for group in &api.groups {
            array.object(|object| {
                object.string("name", &group.name);
                object.docs("docs", &group.docs);
                object.strings("prefixes", &group.prefixes);
                object.receiver("receiver", &group.receiver);
                object.boolean("view", group.view);
                object.array("functions", |array| {
                    for function in &group.functions {
                        array.object(|object| {
                            object.string("symbol", &function.symbol);
                            object.string("name", &function.name);
                            object.string("role", role_name(function.role));
                            object.boolean("optional", function.optional);
                            match function.sized_by.as_deref() {
                                Some(symbol) => object.string("sized_by", symbol),
                                None => object.null("sized_by"),
                            }
                            object.result("result", &function.result);
                            object.array("params", |array| {
                                for param in &function.params {
                                    array.object(|object| param_body(object, param));
                                }
                            });
                            object.array("outputs", |array| {
                                for output in &function.outputs {
                                    array.object(|object| {
                                        object.string("name", &output.name);
                                        object.ty("type", &output.ty);
                                        object.boolean("owned_buffer", output.owned_buffer);
                                    });
                                }
                            });
                            object.docs("docs", &function.docs);
                        });
                    }
                });
            });
        }
    });
    object.finish();
    out.push('\n');
    out
}

/// Writes one parameter.
fn param_body(object: &mut Object<'_>, param: &Param) {
    object.string("name", &param.name);
    object.string("role", param_role_name(param.role));
    object.ty("type", &param.ty);
    object.boolean("optional", param.optional);
    if let Some(placement) = param.placement {
        object.string("placement", placement_name(placement));
    }
}

/// The name a placement goes by in the file.
fn placement_name(placement: Placement) -> &'static str {
    match placement {
        Placement::Adopt => "adopt",
        Placement::Require => "require",
    }
}

/// The name a role goes by in the file.
fn role_name(role: Role) -> &'static str {
    match role {
        Role::Constructor => "constructor",
        Role::Getter => "getter",
        Role::Setter => "setter",
        Role::Clearer => "clearer",
        Role::Destructor => "destructor",
        Role::Method => "method",
        Role::Free => "free",
        Role::Plumbing => "plumbing",
    }
}

/// The name a parameter's role goes by in the file.
fn param_role_name(role: ParamRole) -> &'static str {
    match role {
        ParamRole::DocumentIn => "document_in",
        ParamRole::DocumentMut => "document_mut",
        ParamRole::DocumentTaken => "document_taken",
        ParamRole::Receiver => "receiver",
        ParamRole::Input => "input",
        ParamRole::Bytes => "bytes",
        ParamRole::Length => "length",
        ParamRole::Output => "output",
        ParamRole::OutputList => "output_list",
        ParamRole::ListCapacity => "list_capacity",
        ParamRole::OutputCount => "output_count",
    }
}

/// A JSON object being written, which keeps track of its own commas.
struct Object<'a> {
    out: &'a mut String,
    depth: usize,
    first: bool,
}

impl<'a> Object<'a> {
    fn new(out: &'a mut String, depth: usize) -> Self {
        out.push('{');
        Self {
            out,
            depth,
            first: true,
        }
    }

    fn key(&mut self, name: &str) {
        if !self.first {
            self.out.push(',');
        }
        self.first = false;
        self.newline(self.depth + 1);
        let _ = write!(self.out, "{}: ", quote(name));
    }

    fn newline(&mut self, depth: usize) {
        self.out.push('\n');
        for _ in 0..depth {
            self.out.push_str("  ");
        }
    }

    fn note(&mut self, text: &str) {
        self.key("//");
        let _ = write!(self.out, "{}", quote(text));
    }

    fn string(&mut self, name: &str, value: &str) {
        self.key(name);
        let _ = write!(self.out, "{}", quote(value));
    }

    fn null(&mut self, name: &str) {
        self.key(name);
        self.out.push_str("null");
    }

    fn number(&mut self, name: &str, value: i64) {
        self.key(name);
        let _ = write!(self.out, "{value}");
    }

    fn boolean(&mut self, name: &str, value: bool) {
        self.key(name);
        let _ = write!(self.out, "{value}");
    }

    fn strings(&mut self, name: &str, values: &[String]) {
        self.key(name);
        self.out.push('[');
        for (index, value) in values.iter().enumerate() {
            if index > 0 {
                self.out.push_str(", ");
            }
            let _ = write!(self.out, "{}", quote(value));
        }
        self.out.push(']');
    }

    fn ty(&mut self, name: &str, value: &Type) {
        self.key(name);
        let _ = write!(self.out, "{}", quote(&type_name(value)));
    }

    fn result(&mut self, name: &str, value: &CResult) {
        self.key(name);
        let spelled = match value {
            CResult::Void => "void".to_string(),
            CResult::Status => "status".to_string(),
            CResult::StaticText => "static_text".to_string(),
            CResult::Value(ty) => format!("value:{}", type_name(ty)),
        };
        let _ = write!(self.out, "{}", quote(&spelled));
    }

    fn receiver(&mut self, name: &str, value: &Receiver) {
        self.key(name);
        let spelled = match value {
            Receiver::None => "none".to_string(),
            Receiver::Document => "document".to_string(),
            Receiver::Node(schema) => format!("node:{schema}"),
            Receiver::Value(what) => format!("value:{what}"),
        };
        let _ = write!(self.out, "{}", quote(&spelled));
    }

    fn docs(&mut self, name: &str, value: &Docs) {
        self.key(name);
        if value.is_empty() && value.references.is_empty() {
            self.out.push_str("null");
            return;
        }
        let depth = self.depth + 1;
        let mut nested = Object::new(self.out, depth);
        nested.string("summary", &value.summary);
        nested.strings("body", &value.body);
        nested.strings("references", &value.references);
        nested.finish();
    }

    /// Writes a number that depends on the target's pointer width.
    fn by_width(&mut self, name: &str, value: ByWidth) {
        self.key(name);
        let depth = self.depth + 1;
        let mut nested = Object::new(self.out, depth);
        nested.number("pointer32", value.pointer32 as i64);
        nested.number("pointer64", value.pointer64 as i64);
        nested.finish();
    }

    /// Writes a struct's size and alignment.
    fn layout(&mut self, name: &str, value: &Layout) {
        self.key(name);
        let depth = self.depth + 1;
        let mut nested = Object::new(self.out, depth);
        nested.by_width("size", value.size);
        nested.by_width("align", value.align);
        nested.finish();
    }

    fn array(&mut self, name: &str, fill: impl FnOnce(&mut Array<'_>)) {
        self.key(name);
        let depth = self.depth + 1;
        let mut array = Array::new(self.out, depth);
        fill(&mut array);
        array.finish();
    }

    fn finish(self) {
        if !self.first {
            self.out.push('\n');
            for _ in 0..self.depth {
                self.out.push_str("  ");
            }
        }
        self.out.push('}');
    }
}

/// A JSON array being written.
struct Array<'a> {
    out: &'a mut String,
    depth: usize,
    first: bool,
}

impl<'a> Array<'a> {
    fn new(out: &'a mut String, depth: usize) -> Self {
        out.push('[');
        Self {
            out,
            depth,
            first: true,
        }
    }

    fn object(&mut self, fill: impl FnOnce(&mut Object<'_>)) {
        if !self.first {
            self.out.push(',');
        }
        self.first = false;
        self.out.push('\n');
        for _ in 0..=self.depth {
            self.out.push_str("  ");
        }
        let depth = self.depth + 1;
        let mut object = Object::new(self.out, depth);
        fill(&mut object);
        object.finish();
    }

    fn finish(self) {
        if !self.first {
            self.out.push('\n');
            for _ in 0..self.depth {
                self.out.push_str("  ");
            }
        }
        self.out.push(']');
    }
}

/// The name a type goes by in the file.
fn type_name(ty: &Type) -> String {
    match ty {
        Type::Bool => "bool".to_string(),
        Type::Double => "double".to_string(),
        Type::Int64 => "int64".to_string(),
        Type::Uint64 => "uint64".to_string(),
        Type::Int32 => "int32".to_string(),
        Type::Uint32 => "uint32".to_string(),
        Type::Size => "size".to_string(),
        Type::Text => "text".to_string(),
        Type::Bytes => "bytes".to_string(),
        Type::Node => "node".to_string(),
        Type::Document => "document".to_string(),
        Type::Struct(name) | Type::Enum(name) => name.clone(),
        Type::List(inner) => format!("list<{}>", type_name(inner)),
    }
}

/// Renders a string as a JSON string.
fn quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                let _ = write!(quoted, "\\u{:04x}", control as u32);
            }
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}
