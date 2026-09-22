"""Translates the parts of pyaaf2's model only its writer uses into Rust.

`gen_builtin.py` covers the definitions every AAF file takes as given, which
the reader needs. A *new* file needs more: pyaaf2 registers an extension model
of Avid's classes and types on every file it opens for writing, and seeds the
new file's dictionary with the data, container and codec definitions it ships.
This emits those, in the order pyaaf2 registers them, as
`src/builtin/write_tables.rs`.

Order matters here in a way it does not for reading. Every definition becomes
an object with a storage of its own in the file, and the storages are created
in registration order, so the order is part of the file's bytes.

Run it from anywhere, with a pyaaf2 checkout as the only argument:

    python3 gen_write_tables.py ~/src/pyaaf2
"""

import os
import subprocess
import sys

PYAAF2 = sys.argv[1] if len(sys.argv) > 1 else '.'
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(PYAAF2, 'src'))

from aaf2 import types  # noqa: E402
from aaf2.model import datadefs, typedefs  # noqa: E402
from aaf2.model.ext import classdefs as ext_classdefs  # noqa: E402
from aaf2.model.ext import typedefs as ext_typedefs  # noqa: E402

OUT = os.path.join(HERE, os.pardir, os.pardir, os.pardir, 'src', 'builtin', 'write_tables.rs')


def q(text):
    return '"%s"' % text.replace('\\', '\\\\').replace('"', '\\"')


def a(text):
    return 'auid(%s)' % q(str(text).lower())


def emit_classes(o):
    o.write('/// The extension classes, and the extension properties of standard\n')
    o.write('/// classes, in the order pyaaf2 registers them.\n')
    o.write('pub(crate) const EXT_CLASSES: &[Class] = &[\n')
    for name, (class_auid, parent, concrete, props) in ext_classdefs.classdefs.items():
        parent = 'Some(%s)' % a(parent) if parent else 'None'
        o.write('    Class {\n')
        o.write('        name: %s,\n' % q(name))
        o.write('        auid: %s,\n' % a(class_auid))
        o.write('        parent: %s,\n' % parent)
        o.write('        concrete: %s,\n' % ('true' if concrete else 'false'))
        o.write('        properties: &[\n')
        for pname, (pauid, pid, ptype, optional, unique) in (props or {}).items():
            o.write('            Prop { name: %s, auid: %s, pid: %s, type_id: %s, '
                    'optional: %s, unique: %s },\n'
                    % (q(pname), a(pauid),
                       'Some(%#06x)' % pid if pid is not None else 'None',
                       a(ptype),
                       'true' if optional else 'false',
                       'true' if unique else 'false'))
        o.write('        ],\n')
        o.write('    },\n')
    o.write('];\n\n')

    o.write('/// Other names the extension classes are known by.\n')
    o.write('pub(crate) const EXT_CLASS_ALIASES: &[(&str, &str)] = &[\n')
    for alias, name in ext_classdefs.aliases.items():
        o.write('    (%s, %s),\n' % (q(alias), q(name)))
    o.write('];\n\n')


def emit_types(o):
    """The extension types, as one list in pyaaf2's registration order.

    pyaaf2 walks its type categories in a fixed order and each category's
    table in its own order, so the whole model flattens to one sequence.
    """
    o.write('/// The extension types, in the order pyaaf2 registers them.\n')
    o.write('///\n')
    o.write('/// An enumeration that already exists is not redefined: its new elements\n')
    o.write('/// are added to the one there is.\n')
    o.write('pub(crate) const EXT_TYPES: &[ExtType] = &[\n')
    for cat in types.categories:
        for name, args in getattr(ext_typedefs, cat, {}).items():
            if not isinstance(args, (tuple, list)):
                args = [args]
            n = q(name)
            if cat == 'ints':
                o.write('    ExtType::Int(IntType { name: %s, auid: %s, size: %d, signed: %s }),\n'
                        % (n, a(args[0]), args[1], 'true' if args[2] else 'false'))
            elif cat == 'enums':
                o.write('    ExtType::Enum(EnumType { name: %s, auid: %s, element_type: %s, elements: &[%s] }),\n'
                        % (n, a(args[0]), a(args[1]),
                           ', '.join('(%d, %s)' % (v, q(e)) for v, e in args[2].items())))
            elif cat == 'records':
                o.write('    ExtType::Record(RecordType { name: %s, auid: %s, members: &[%s] }),\n'
                        % (n, a(args[0]),
                           ', '.join('(%s, %s)' % (q(m), a(t)) for m, t in args[1])))
            elif cat == 'fixed_arrays':
                o.write('    ExtType::FixedArray(FixedArrayType { name: %s, auid: %s, element_type: %s, count: %d }),\n'
                        % (n, a(args[0]), a(args[1]), args[2]))
            elif cat in ('var_arrays', 'renames', 'strings', 'sets', 'strongrefs'):
                variant = {'var_arrays': 'VarArray', 'renames': 'Rename', 'strings': 'String',
                           'sets': 'Set', 'strongrefs': 'StrongRef'}[cat]
                o.write('    ExtType::%s(PairType { name: %s, auid: %s, other: %s }),\n'
                        % (variant, n, a(args[0]), a(args[1])))
            elif cat in ('streams', 'opaques', 'chars', 'indirects'):
                variant = {'streams': 'Stream', 'opaques': 'Opaque', 'chars': 'Character',
                           'indirects': 'Indirect'}[cat]
                o.write('    ExtType::%s(SoloType { name: %s, auid: %s }),\n' % (variant, n, a(args[0])))
            elif cat == 'extenums':
                o.write('    ExtType::ExtEnum(ExtEnumType { name: %s, auid: %s, elements: &[%s] }),\n'
                        % (n, a(args[0]),
                           ', '.join('(%s, %s)' % (a(v), q(e)) for v, e in args[1].items())))
            elif cat == 'weakrefs':
                o.write('    ExtType::WeakRef(WeakRefType { name: %s, auid: %s, target: %s, target_set: &[%s] }),\n'
                        % (n, a(args[0]), a(args[1]), ', '.join(a(s) for s in args[2])))
            else:
                raise NotImplementedError('no emitter for extension %s' % cat)
    o.write('];\n\n')


def emit_generic_char_sizes(o):
    o.write('/// The width, in bytes, of each built-in generic character type.\n')
    o.write('pub(crate) const GENERIC_CHARACTER_SIZES: &[(Auid, u8)] = &[\n')
    for name, (type_auid, size) in typedefs.generic_chars.items():
        o.write('    (%s, %d), // %s\n' % (a(type_auid), size, name))
    o.write('];\n\n')


def emit_defs(o):
    for const, doc, rows in (
        ('DATA_DEFS', 'The data definitions a new file starts with.', datadefs.DataDefs),
        ('CONTAINER_DEFS', 'The container definitions a new file starts with.', datadefs.ContainerDefs),
    ):
        o.write('/// %s\n' % doc)
        o.write('pub(crate) const %s: &[Definition] = &[\n' % const)
        for key, (name, description) in rows.items():
            o.write('    Definition { auid: %s, name: %s, description: %s },\n'
                    % (a(key), q(name), q(description)))
        o.write('];\n\n')

    o.write('/// The codec definitions a new file starts with.\n')
    o.write('///\n')
    o.write('/// pyaaf2 carries many more, but registers only the ones it knows the\n')
    o.write('/// descriptor class and data kinds of.\n')
    o.write('pub(crate) const CODEC_DEFS: &[CodecDefinition] = &[\n')
    for key, args in datadefs.CodecDefs.items():
        if len(args) <= 2:
            continue
        name, description, classdef, datadef_names = args
        o.write('    CodecDefinition { auid: %s, name: %s, description: %s, file_descriptor_class: %s, data_definitions: &[%s] },\n'
                % (a(key), q(name), q(description), q(classdef),
                   ', '.join(q(d) for d in datadef_names)))
    o.write('];\n')


import io  # noqa: E402

body = io.StringIO()
emit_classes(body)
emit_types(body)
emit_generic_char_sizes(body)
emit_defs(body)
body = body.getvalue()

# Import only the row types the tables use, so the output has no unused
# imports whatever the extension model holds.
ROW_TYPES = ['Class', 'CodecDefinition', 'Definition', 'EnumType', 'ExtEnumType', 'ExtType',
             'FixedArrayType', 'IntType', 'PairType', 'Prop', 'RecordType', 'SoloType',
             'WeakRefType']
used = [t for t in ROW_TYPES if (t + ' {') in body or (t + '(') in body or ('ExtType::' in body and t == 'ExtType')]

with open(OUT, 'w') as o:
    o.write('//! The parts of pyaaf2\'s model that only a new file needs.\n')
    o.write('//!\n')
    o.write('//! Generated from `pyaaf2` 08dcc3d by `gen_write_tables.py`; do not edit by\n')
    o.write('//! hand. Rows are in pyaaf2\'s registration order, which the write path\n')
    o.write('//! depends on: each definition is stored in a storage of its own, created\n')
    o.write('//! in that order.\n\n')
    o.write('use super::{%s, auid};\n' % ', '.join(used))
    o.write('use crate::Auid;\n\n')
    o.write(body)

subprocess.run(['rustfmt', '--edition', '2024', OUT], check=True)
print('wrote', OUT)
