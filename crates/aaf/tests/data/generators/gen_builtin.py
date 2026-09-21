"""Translates pyaaf2's built-in definition tables into Rust.

These are the class, property and type definitions AAF takes as given: every
file uses them and no file stores them. pyaaf2 keeps them as Python dicts under
`aaf2/model`, plus the `Root` class in `aaf2/metadict.py`. This emits the same
data as Rust tables, so the port carries the definitions rather than a
transcription of them.

The extension model under `aaf2/model/ext` is left out: pyaaf2 registers it
only on a writeable file, so it belongs with the write path.
"""

import os
import sys

# Run this from anywhere, with a pyaaf2 checkout as the only argument:
#
#     python3 gen_builtin.py ~/src/pyaaf2
#
# It rewrites the generated table in the crate's source. It imports pyaaf2 and
# never this crate, so what lands is a translation of upstream's own data.
PYAAF2 = sys.argv[1] if len(sys.argv) > 1 else '.'
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(PYAAF2, 'src'))

from aaf2.model import classdefs, typedefs
from aaf2.metadict import root_classes, root_types

OUT = os.path.join(HERE, os.pardir, os.pardir, os.pardir, 'src', 'builtin', 'tables.rs')


def q(text):
    return '"%s"' % text.replace('\\', '\\\\').replace('"', '\\"')


def a(text):
    return 'auid(%s)' % q(str(text).lower())


def emit_classes(o):
    rows = dict(root_classes)
    rows.update(classdefs.classdefs)
    o.write('/// Every class AAF defines, with the properties each one adds.\n')
    o.write('pub(super) const CLASSES: &[Class] = &[\n')
    for name, (class_auid, parent, concrete, props) in rows.items():
        parent = 'Some(%s)' % a(parent) if parent else 'None'
        o.write('    Class {\n')
        o.write('        name: %s,\n' % q(name))
        o.write('        auid: %s,\n' % a(class_auid))
        o.write('        parent: %s,\n' % parent)
        o.write('        concrete: %s,\n' % ('true' if concrete else 'false'))
        if not props:
            o.write('        properties: &[],\n')
        else:
            o.write('        properties: &[\n')
            for pname, (pauid, pid, ptype, optional, unique) in props.items():
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
    return len(rows)


def emit_aliases(o):
    o.write('/// Other names the same classes are known by.\n')
    o.write('pub(super) const CLASS_ALIASES: &[(&str, &str)] = &[\n')
    for alias, name in classdefs.aliases.items():
        o.write('    (%s, %s),\n' % (q(alias), q(name)))
    o.write('];\n\n')
    return len(classdefs.aliases)


def emit_simple(o, const, doc, rows, fields, render):
    o.write('/// %s\n' % doc)
    o.write('pub(super) const %s: &[%s] = &[\n' % (const, fields))
    for name, args in rows.items():
        o.write('    %s\n' % render(name, args))
    o.write('];\n\n')
    return len(rows)


def emit_types(o):
    counts = {}
    counts['ints'] = emit_simple(
        o, 'INTS', 'Integers, by width and signedness.', typedefs.ints, 'IntType',
        lambda n, x: 'IntType { name: %s, auid: %s, size: %d, signed: %s },'
                     % (q(n), a(x[0]), x[1], 'true' if x[2] else 'false'))

    o.write('/// Enumerations, with the name of each value.\n')
    o.write('pub(super) const ENUMS: &[EnumType] = &[\n')
    for name, (type_auid, element_type, elements) in typedefs.enums.items():
        o.write('    EnumType {\n        name: %s,\n        auid: %s,\n'
                '        element_type: %s,\n        elements: &[\n'
                % (q(name), a(type_auid), a(element_type)))
        for value, element in elements.items():
            o.write('            (%d, %s),\n' % (value, q(element)))
        o.write('        ],\n    },\n')
    o.write('];\n\n')
    counts['enums'] = len(typedefs.enums)

    o.write('/// Records, with their members in storage order.\n')
    o.write('pub(super) const RECORDS: &[RecordType] = &[\n')
    for name, (type_auid, members) in typedefs.records.items():
        o.write('    RecordType {\n        name: %s,\n        auid: %s,\n        members: &[\n'
                % (q(name), a(type_auid)))
        for member, member_type in members:
            o.write('            (%s, %s),\n' % (q(member), a(member_type)))
        o.write('        ],\n    },\n')
    o.write('];\n\n')
    counts['records'] = len(typedefs.records)

    counts['fixed_arrays'] = emit_simple(
        o, 'FIXED_ARRAYS', 'Arrays of a fixed length.', typedefs.fixed_arrays, 'FixedArrayType',
        lambda n, x: 'FixedArrayType { name: %s, auid: %s, element_type: %s, count: %d },'
                     % (q(n), a(x[0]), a(x[1]), x[2]))

    for const, doc, rows in [
        ('VAR_ARRAYS', 'Arrays of any length.', typedefs.var_arrays),
        ('SETS', 'Unordered collections.', typedefs.sets),
        ('RENAMES', 'Other names for existing types.', typedefs.renames),
        ('STRINGS', 'Strings, by their character type.', typedefs.strings),
        ('STRONG_REFS', 'References to an object this one owns.', typedefs.strongrefs),
    ]:
        counts[const] = emit_simple(
            o, const, doc, rows, 'PairType',
            lambda n, x: 'PairType { name: %s, auid: %s, other: %s },' % (q(n), a(x[0]), a(x[1])))

    for const, doc, rows in [
        ('STREAMS', 'Streams stored outside the property.', typedefs.streams),
        ('OPAQUES', 'Values whose type the reader is not expected to know.', typedefs.opaques),
        ('CHARACTERS', 'Single characters.', typedefs.chars),
        ('INDIRECTS', 'Values that carry their own type.', typedefs.indirects),
    ]:
        counts[const] = emit_simple(
            o, const, doc, rows, 'SoloType',
            lambda n, x: 'SoloType { name: %s, auid: %s },' % (q(n), a(x)))

    counts['generic_chars'] = emit_simple(
        o, 'GENERIC_CHARACTERS',
        'Characters of a width the reader is told rather than knows.',
        typedefs.generic_chars, 'SoloType',
        lambda n, x: 'SoloType { name: %s, auid: %s },' % (q(n), a(x[0])))

    o.write('/// Extendible enumerations, with the name of each value.\n')
    o.write('pub(super) const EXT_ENUMS: &[ExtEnumType] = &[\n')
    for name, (type_auid, elements) in typedefs.extenums.items():
        o.write('    ExtEnumType {\n        name: %s,\n        auid: %s,\n        elements: &[\n'
                % (q(name), a(type_auid)))
        for value, element in elements.items():
            o.write('            (%s, %s),\n' % (a(value), q(element)))
        o.write('        ],\n    },\n')
    o.write('];\n\n')
    counts['extenums'] = len(typedefs.extenums)

    weakrefs = dict(typedefs.weakrefs)
    o.write('/// References to an object owned elsewhere, and where it is owned.\n')
    o.write('pub(super) const WEAK_REFS: &[WeakRefType] = &[\n')
    for name, args in weakrefs.items():
        type_auid, target = args[0], args[1]
        target_set = args[2] if len(args) > 2 else ()
        o.write('    WeakRefType {\n        name: %s,\n        auid: %s,\n'
                '        target: %s,\n        target_set: &[' % (q(name), a(type_auid), a(target)))
        o.write(', '.join(a(step) for step in target_set))
        o.write('],\n    },\n')
    o.write('];\n\n')
    counts['weakrefs'] = len(weakrefs)

    # The Root class's two strong reference types are defined with it, not in
    # the shared model, so they are appended to the strong reference table.
    o.write('/// The strong reference types the `Root` class needs.\n')
    o.write('pub(super) const ROOT_STRONG_REFS: &[PairType] = &[\n')
    for name, (type_auid, target) in root_types.items():
        o.write('    PairType { name: %s, auid: %s, other: %s },\n'
                % (q(name), a(type_auid), a(target)))
    o.write('];\n')
    counts['root_types'] = len(root_types)
    return counts


with open(OUT, 'w') as o:
    o.write('//! The class, property and type definitions AAF takes as given.\n')
    o.write('//!\n')
    o.write('//! Generated from `pyaaf2` 08dcc3d by `gen_builtin.py`; do not edit by hand.\n')
    o.write('//! The extension model under `aaf2/model/ext` is not here: pyaaf2 registers it\n')
    o.write('//! only on a writeable file, so it belongs with the write path.\n\n')
    o.write('use super::{Class, EnumType, ExtEnumType, FixedArrayType, IntType, PairType, Prop,\n')
    o.write('            RecordType, SoloType, WeakRefType, auid};\n\n')
    classes = emit_classes(o)
    aliases = emit_aliases(o)
    counts = emit_types(o)

print('classes', classes, 'aliases', aliases)
print('types', sum(counts.values()) - counts['root_types'], counts)
