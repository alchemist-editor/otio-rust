import hashlib
import os
import sys

# Run this from anywhere, with a pyaaf2 checkout as the only argument:
#
#     python3 gen_objects.py ~/src/pyaaf2
#
# It writes into the directory above this one, which is where the fixtures and
# the manifests live. It imports pyaaf2 and never this crate: a manifest built
# from our own reader would agree with any bug our reader has.
PYAAF2 = sys.argv[1] if len(sys.argv) > 1 else '.'
D = os.path.join(os.path.dirname(os.path.abspath(__file__)), os.pardir)
FIXTURES = os.path.join(PYAAF2, 'tests', 'test_files')
sys.path.insert(0, os.path.join(PYAAF2, 'src'))

import aaf2
from aaf2 import properties as P

def describe(p):
    if isinstance(p, P.StreamProperty):
        return 's:%s' % p.stream_name
    if isinstance(p, P.StrongRefProperty):
        return 'r:%s' % p.ref
    if isinstance(p, P.StrongRefVectorProperty):
        return 'v:%s:%d' % (p.index_name, len(p.references))
    if isinstance(p, P.StrongRefSetProperty):
        return 't:%s:%d:%d' % (p.index_name, len(p.references), p.key_pid)
    if isinstance(p, P.WeakRefArrayProperty):
        return 'a:%s:%d:%d:%d' % (p.index_name, len(p.references), p.weakref_index, p.key_pid)
    if isinstance(p, P.WeakRefProperty):
        return 'w:%d:%d:%s' % (p.weakref_index, p.key_pid, p.ref)
    return 'd:%d:%s' % (len(p.data), hashlib.sha256(bytes(p.data)).hexdigest()[:16])

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
    with aaf2.open(path, 'r') as f:
        stack = [f.root]
        while stack:
            obj = stack.pop()
            props = []
            for pid in sorted(obj.property_entries):
                p = obj.property_entries[pid]
                props.append('%d:%d:%s' % (pid, p.format, describe(p)))
            rows.append((obj.dir.path() or '/', str(obj.class_id), '|'.join(props)))
            stack.extend(children(obj))
    rows.sort(key=lambda r: r[0])
    with open(out, 'w') as o:
        o.write('# path\tclass_id\tpid:format:value|...\n')
        o.write('# generated from %s by pyaaf2 08dcc3d\n' % path.rsplit('/', 1)[-1])
        for r in rows:
            o.write('%s\t%s\t%s\n' % r)
    print(path, '->', len(rows), 'objects')

run(os.path.join(FIXTURES, 'empty.aaf'), os.path.join(D, 'empty.objects.tsv'))
run(os.path.join(FIXTURES, 'sector_size_512.aaf'), os.path.join(D, 'sector_size_512.objects.tsv'))
