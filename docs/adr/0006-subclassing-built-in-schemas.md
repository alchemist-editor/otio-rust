# ADR 0006: Subclasses of built-in schemas

- **Status:** Accepted
- **Date:** 2026-09-22
- **Deciders:** Jeff Hodges

## Context

Upstream's Python API lets a program subclass a concrete schema and register
the subclass as a schema of its own:

```python
@otio.core.register_type
class TakeClip(otio.schema.Clip):
    _serializable_label = "TakeClip.1"
    take = otio.core.serializable_field("take", int)
```

Upstream's C++ holds such an object as an ordinary `Clip`. The registry's
factory for `TakeClip` builds one by calling the Python class, whose
`__init__` ends in `set_type_record`, which points the object's type record
at `TakeClip`. From then on `schema_name()` and `schema_version()` answer
from that record, so the object is written as `TakeClip.1`; the
`serializable_field`s live in the dynamic fields every `SerializableObject`
has, and are written before the `Clip`'s own fields; and everything that
works on a `Clip` works on it, because it is one. Reading such a file where
`TakeClip` is not registered gives an `UnknownSchema`, holding every field.

Before this, `otio-core` held a registered schema only as a
[`DynamicObject`](../../crates/otio-core/src/schema.rs): a name, a version, an
optional name and metadata, and a field map. That is exactly upstream's
subclass of `SerializableObject` or `SerializableObjectWithMetadata`, and it
is all a subclass of either can be. It cannot be a clip: a composition, an
algorithm or an adapter matches on `Node::Clip`, and a dynamic object is not
one. So registering a subclass of `Clip` raised `NotImplementedError`
(issue #87).

ADR 0001 puts every object in an arena as a `Node`, and the core never holds
a Python object. The question is where, in that model, an object records that
it is an instance of a subclass, and where the subclass's fields go.

## Options

**A. A new `Node` variant wrapping the built-in** (`Subclass { schema,
fields, node: Box<Node> }`). Faithful to "a `TakeClip` is something else",
and wrong for exactly that reason: every `match` on `Node::Clip` in the core,
the adapters and the bindings stops seeing it, and each would have to learn to
look inside. That is the problem the issue is about.

**B. A side table in the arena**, from node to subclass record. `Node` stays
as it is, but the table has to be kept in step with everything that moves
nodes: copying, absorbing one document into another, removing a subtree.
The field map may hold objects, and `Node::visit_links_mut` — the one place
that knows where a handle can hide — would not see those, so a copy or a
move would leave them pointing into the wrong document.

**C. A field on `Base`**, the name and metadata every schema a subclass could
derive from already carries. The object stays `Node::Clip`, so nothing that
matches on it changes. Copying, absorbing and deep-cloning copy `Base` with
the rest of the node, and `visit_links_mut` already walks `Base`'s metadata,
so it walks the field map beside it.

## Decision

**Option C.** `Base` gains `extension: Option<Box<Extension>>`, where an
`Extension` holds:

- `schema: Option<(String, u32)>` — the subclass's name and version, which
  `Node::schema_name` and `Node::schema_version` answer with. `None` keeps
  the built-in's own; `Node::built_in_schema_name` always gives the
  built-in's.
- `fields: AnyDictionary` — upstream's dynamic fields.

It is boxed and optional so that the objects that have none, almost all of
them, pay one pointer.

The registry gains `SchemaKind::Subclass(built_in)` and `register_subclass`.
Reading an object of such a schema reads it as the built-in, then keeps every
field the built-in's reader did not take as the extension's fields, and sets
the extension's schema. Writing writes the extension's fields straight after
`OTIO_SCHEMA`, as upstream does. Upgrade and downgrade functions are looked up
under the subclass's name, as upstream's are, since that is the name the
object is written with. Anywhere a file may hold a clip, it may hold a
subclass of one; a subclass of `Clip` among a track's markers is refused, as
it would be upstream.

In Python, `register_type` registers a subclass of a concrete class with
`register_subclass`, and `set_type_record` sets the extension's schema. The
class is found again by the subclass's schema name when the node is wrapped,
as it is for a dynamic object. The two root classes stay dynamic objects: a
subclass of either has no built-in behaviour to keep.

## Consequences

- **Dynamic fields work on every object with a name.** Upstream gives every
  object dynamic fields; the extension is where they now live, with or
  without a subclass. `_dynamic_fields` stops refusing a `Clip`, and a
  built-in read from a file keeps the fields it does not know and writes them
  back, as upstream's does — the deserializer's "nothing is dropped" now
  holds inside known schemas too, not only for unknown ones.
- **`Base` has a third field.** Every struct literal of it names the new
  field or uses `Base::new(name, metadata)`. That is a breaking change for a
  Rust user building `Base` literally; the C ABI and the generated SDKs do
  not change, since they never build a `Base`.
- **The subclass is invisible to the C ABI except by name.** `otio_node_kind`
  says `Clip`, `otio_node_schema_name` says `TakeClip`, as upstream's C++
  would. The extension's fields are written and read with the object, but
  there is no C entry point to reach them yet.
- **Python constructors move to `__init__` for subclasses.** Each class here
  builds its object in `__new__`, which Python calls with the subclass's
  arguments rather than the ones its `__init__` passes on. A Python subclass
  of a concrete class is given a `__new__` that builds a default object, and
  `SerializableObject.__init__` builds it again from the arguments passed on,
  which is where pybind11's constructors take them upstream.
- **A subclass cannot redefine a built-in field.** A `serializable_field`
  named after one of the built-in's own fields is written twice, as upstream
  writes it, and read back as the built-in's.
