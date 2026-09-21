"""The content tree of each fixture, read by name rather than by number.

pyaaf2 reads a file the way people talk about AAF: the mobs of the content
storage, the slots of a mob, the segment of a slot. This records what that
reading finds, so the crate's named-access layer can be checked against it.

Values are read through the property name and pyaaf2's own decode, not through
its convenience properties, so that both sides are doing the same thing. A
Rational is recorded as its two members rather than as the Fraction pyaaf2
packages it into, for the reason `gen_values.py` explains.
"""

import os
import sys

# Run this from anywhere, with a pyaaf2 checkout as the only argument:
#
#     python3 gen_content.py ~/src/pyaaf2
#
# It writes into the directory above this one, which is where the fixtures and
# the manifests live. It imports pyaaf2 and never this crate: a manifest built
# from our own reader would agree with any bug our reader has.
PYAAF2 = sys.argv[1] if len(sys.argv) > 1 else '.'
D = os.path.join(os.path.dirname(os.path.abspath(__file__)), os.pardir)
FIXTURES = os.path.join(PYAAF2, 'tests', 'test_files')
sys.path.insert(0, os.path.join(PYAAF2, 'src'))

import aaf2
from aaf2.rational import AAFRational


def escape(text):
    return (text.replace('\\', '\\\\').replace('\t', '\\t')
                .replace('\n', '\\n').replace('\r', '\\r'))


def field(obj, name):
    """A named property, rendered the way the crate renders it, or '-'."""
    p = obj.get(name)
    if p is None:
        return '-'
    # A weak reference names another object by key. Resolving it is a
    # different job from decoding bytes, so the key is what is recorded.
    if hasattr(p, 'ref') and not hasattr(p, 'references'):
        return '-' if p.ref is None else escape(str(p.ref))
    if p.data is None:
        return '-'
    value = p.value
    if value is None:
        return '-'
    if isinstance(value, AAFRational):
        return '%d/%d' % (value.numerator, value.denominator)
    if isinstance(value, bool):
        return 'True' if value else 'False'
    if isinstance(value, int):
        return str(value)
    return escape(str(value))


def kind(obj):
    return obj.classdef.class_name


def run(path, out):
    rows = []
    with aaf2.open(path, 'r') as f:
        mobs = sorted(f.content.mobs, key=lambda m: str(m['MobID'].value))
        for mob in mobs:
            rows.append('M\t%s\t%s\t%s\t%s' % (
                field(mob, 'MobID'), kind(mob), field(mob, 'Name'),
                field(mob, 'UsageCode')))
            for slot in sorted(mob['Slots'].value, key=lambda s: s['SlotID'].value):
                rows.append('S\t%s\t%s\t%s\t%s\t%s\t%s' % (
                    field(mob, 'MobID'), field(slot, 'SlotID'), kind(slot),
                    field(slot, 'SlotName'), field(slot, 'EditRate'),
                    field(slot, 'PhysicalTrackNumber')))
                segment = slot['Segment'].value
                rows.append('G\t%s\t%s\t%s\t%s\t%s' % (
                    field(mob, 'MobID'), field(slot, 'SlotID'), kind(segment),
                    field(segment, 'Length'), field(segment, 'DataDefinition')))
                if 'Components' in segment:
                    for i, component in enumerate(segment['Components'].value):
                        rows.append('C\t%s\t%s\t%d\t%s\t%s\t%s' % (
                            field(mob, 'MobID'), field(slot, 'SlotID'), i,
                            kind(component), field(component, 'Length'),
                            field(component, 'SourceID')))
    with open(out, 'w') as o:
        o.write('# M\tmob_id\tclass\tname\tusage\n')
        o.write('# S\tmob_id\tslot_id\tclass\tslot_name\tedit_rate\ttrack\n')
        o.write('# G\tmob_id\tslot_id\tclass\tlength\tdata_def\n')
        o.write('# C\tmob_id\tslot_id\tindex\tclass\tlength\tsource_id\n')
        o.write('# generated from %s by pyaaf2 08dcc3d\n' % path.rsplit('/', 1)[-1])
        for r in rows:
            o.write(r + '\n')
    print(path, '->', len(rows), 'rows')


run(os.path.join(FIXTURES, 'empty.aaf'), os.path.join(D, 'empty.content.tsv'))
run(os.path.join(FIXTURES, 'sector_size_512.aaf'), os.path.join(D, 'sector_size_512.content.tsv'))
