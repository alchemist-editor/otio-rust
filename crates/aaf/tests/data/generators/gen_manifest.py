import hashlib
import os
import sys

# Run this from anywhere, with a pyaaf2 checkout as the only argument:
#
#     python3 gen_manifest.py ~/src/pyaaf2
#
# It writes into the directory above this one, which is where the fixtures and
# the manifests live. It imports pyaaf2 and never this crate: a manifest built
# from our own reader would agree with any bug our reader has.
PYAAF2 = sys.argv[1] if len(sys.argv) > 1 else '.'
D = os.path.join(os.path.dirname(os.path.abspath(__file__)), os.pardir)
FIXTURES = os.path.join(PYAAF2, 'tests', 'test_files')
sys.path.insert(0, os.path.join(PYAAF2, 'src'))

from aaf2.cfb import CompoundFileBinary

def run(name, out):
    with open(name, 'rb') as f:
        c = CompoundFileBinary(f, 'rb')
        rows = []
        def visit(entry):
            path = entry.path() or '/'
            cid = str(entry.class_id) if entry.class_id else '-'
            if entry.isdir():
                kind = 'root' if entry.isroot() else 'storage'
                rows.append((kind, path, '-', cid))
                for child in c.listdir_dict(entry).values():
                    visit(child)
            else:
                data = bytes(entry.open('r').read())
                assert len(data) == entry.byte_size, (path, len(data), entry.byte_size)
                digest = hashlib.sha256(data).hexdigest()[:16]
                rows.append(('stream', path, '%d:%s' % (entry.byte_size, digest), cid))
        visit(c.root)
        rows.sort(key=lambda r: r[1])
        with open(out, 'w') as o:
            o.write('# path\tkind\tsize:sha256-prefix\tclass_id\n')
            o.write('# generated from %s by pyaaf2 08dcc3d\n' % name.rsplit('/', 1)[-1])
            for kind, path, sz, cid in rows:
                o.write('%s\t%s\t%s\t%s\n' % (path, kind, sz, cid))
        print(name, '->', out, len(rows))

run(os.path.join(FIXTURES, 'empty.aaf'), os.path.join(D, 'empty.manifest.tsv'))
run(os.path.join(FIXTURES, 'sector_size_512.aaf'), os.path.join(D, 'sector_size_512.manifest.tsv'))
