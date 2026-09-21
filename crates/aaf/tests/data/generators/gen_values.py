"""Ground truth for decoded property values, generated from pyaaf2 itself.

Every value here is produced by pyaaf2's own `TypeDef.decode`. The one place
this generator steps around pyaaf2 is the post-processing `TypeDefRecord.decode`
applies after it has decoded a record's members: it packages TimeStruct,
DateStruct and TimeStamp into Python `datetime` objects and Rational into a
`Fraction`. Those are a presentation layer on top of the decode, and the Rust
port deliberately hands back the members instead, so the manifest records the
members. The members themselves are still decoded by pyaaf2, by the same field
loop pyaaf2 runs. AUID and MobID records are decoded whole, as both sides do.
"""

import os
import sys

# Run this from anywhere, with a pyaaf2 checkout as the only argument:
#
#     python3 gen_values.py ~/src/pyaaf2
#
# It writes into the directory above this one, which is where the fixtures and
# the manifests live. It imports pyaaf2 and never this crate: a manifest built
# from our own reader would agree with any bug our reader has.
PYAAF2 = sys.argv[1] if len(sys.argv) > 1 else '.'
D = os.path.join(os.path.dirname(os.path.abspath(__file__)), os.pardir)
FIXTURES = os.path.join(PYAAF2, 'tests', 'test_files')
sys.path.insert(0, os.path.join(PYAAF2, 'src'))

import aaf2
from aaf2 import properties as P, types as T
from aaf2.auid import AUID
from aaf2.mobid import MobID

AUID_AUID = AUID("01030100-0000-0000-060e-2b3401040101")
MOBID_AUID = AUID("01030200-0000-0000-060e-2b3401040101")
CHARACTER_AUID = AUID("01100100-0000-0000-060e-2b3401040101")


def decode(typedef, data):
    """pyaaf2's decode, minus the record post-processing described above."""
    if isinstance(typedef, T.TypeDefRecord) and typedef.auid not in (AUID_AUID, MOBID_AUID):
        out = {}
        start = 0
        for key, typedef_name in typedef.fields:
            member = typedef.root.metadict.lookup_typedef(typedef_name)
            end = start + member.byte_size
            out[key] = decode(member, data[start:end])
            start = end
        return out
    if isinstance(typedef, T.TypeDefRename):
        return decode(typedef.renamed_typedef, data)
    if isinstance(typedef, T.TypeDefIndirect):
        return decode(typedef.decode_typedef(data), data[17:])
    if isinstance(typedef, (T.TypeDefFixedArray, T.TypeDefVarArray)):
        element = typedef.element_typedef
        if isinstance(typedef, T.TypeDefVarArray) and element.auid == CHARACTER_AUID:
            return typedef.decode(data)
        size = element.byte_size
        return [decode(element, data[i:i + size]) for i in range(0, len(data) - size + 1, size)]
    if isinstance(typedef, T.TypeDefSet):
        element = typedef.element_typedef
        size = element.byte_size
        return set(render(decode(element, data[i:i + size]))
                   for i in range(0, len(data) - size + 1, size))
    return typedef.decode(data)


def escape(text):
    return (text.replace('\\', '\\\\').replace('\t', '\\t')
                .replace('\n', '\\n').replace('\r', '\\r'))


def render(value):
    if isinstance(value, bool):
        return 'True' if value else 'False'
    if isinstance(value, int):
        return str(value)
    if isinstance(value, (AUID, MobID)):
        return str(value)
    if isinstance(value, str):
        return escape(value)
    if isinstance(value, (list, tuple)):
        return '[%s]' % ', '.join(render(v) for v in value)
    if isinstance(value, set):
        return '{%s}' % ', '.join(sorted(value))
    if isinstance(value, dict):
        return '{%s}' % ', '.join('%s=%s' % (k, render(v)) for k, v in value.items())
    raise TypeError('unrendered %r' % type(value))


def children(obj):
    for pid in sorted(obj.property_entries):
        p = obj.property_entries[pid]
        if isinstance(p, P.StrongRefProperty):
            yield p.value
        elif isinstance(p, P.StrongRefVectorProperty):
            for v in p:
                yield v
        elif isinstance(p, P.StrongRefSetProperty):
            for v in p.values():
                yield v


def run(path, out):
    rows = []
    skipped = 0
    with aaf2.open(path, 'r') as f:
        stack = [f.root]
        while stack:
            obj = stack.pop()
            where = obj.dir.path() or '/'
            for pid in sorted(obj.property_entries):
                p = obj.property_entries[pid]
                if p.format != P.SF_DATA or p.data is None:
                    continue
                typedef = p.typedef
                if typedef is None:
                    skipped += 1
                    continue
                if isinstance(typedef, T.TypeDefStream):
                    continue
                rows.append((where, pid, typedef.type_name, render(decode(typedef, p.data))))
            stack.extend(children(obj))
    rows.sort(key=lambda r: (r[0], r[1]))
    with open(out, 'w') as o:
        o.write('# path\tpid\ttype_name\tvalue\n')
        o.write('# generated from %s by pyaaf2 08dcc3d\n' % path.rsplit('/', 1)[-1])
        for where, pid, name, value in rows:
            o.write('%s\t%d\t%s\t%s\n' % (where, pid, name, value))
    print(path, '->', len(rows), 'values,', skipped, 'without a type')


run(os.path.join(FIXTURES, 'empty.aaf'), os.path.join(D, 'empty.values.tsv'))
run(os.path.join(FIXTURES, 'sector_size_512.aaf'), os.path.join(D, 'sector_size_512.values.tsv'))
