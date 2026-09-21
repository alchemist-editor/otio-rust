import os
import sys

# Run this from anywhere, with a pyaaf2 checkout as the only argument:
#
#     python3 gen_merged.py ~/src/pyaaf2
#
# It writes into the directory above this one, which is where the fixtures and
# the manifests live. It imports pyaaf2 and never this crate: a manifest built
# from our own reader would agree with any bug our reader has.
PYAAF2 = sys.argv[1] if len(sys.argv) > 1 else '.'
D = os.path.join(os.path.dirname(os.path.abspath(__file__)), os.pardir)
FIXTURES = os.path.join(PYAAF2, 'tests', 'test_files')
sys.path.insert(0, os.path.join(PYAAF2, 'src'))

import aaf2
from aaf2.auid import AUID
from aaf2.model import classdefs as model_classdefs

# Properties AAF defines with no identifier of their own. A file that uses one
# assigns it an identifier in its own dictionary; pyaaf2 assigns every one of
# them a placeholder counting down from 0xffff whether the file uses it or not.
# A placeholder is pyaaf2's bookkeeping rather than anything in the file, so
# one is left out unless the file itself defines that property.
DYNAMIC = set()
for _cname, _cargs in model_classdefs.classdefs.items():
    for _pname, _pargs in (_cargs[3] or {}).items():
        if _pargs[1] is None:
            DYNAMIC.add(str(AUID(_pargs[0])).lower())
from aaf2.types import iter_utf16_array
from aaf2.utils import decode_utf16le
from struct import unpack

P = dict(AUID=0x05, NAME=0x06, PARENT=0x08, PROPERTIES=0x09, CONCRETE=0x0a,
         TYPE=0x0b, OPTIONAL=0x0c, PID=0x0d, UNIQUE=0x0e,
         INT_SIZE=0x0f, INT_SIGNED=0x10, SREF=0x11, WREF=0x12, WREF_SET=0x13,
         ENUM_TYPE=0x14, ENUM_NAMES=0x15, ENUM_VALUES=0x16,
         FIXED_TYPE=0x17, FIXED_COUNT=0x18, VAR_TYPE=0x19, SET_TYPE=0x1a,
         STR_TYPE=0x1b, REC_TYPES=0x1c, REC_NAMES=0x1d, RENAME=0x1e,
         EXT_NAMES=0x1f, EXT_VALUES=0x20)

def ent(o, pid): return o.property_entries.get(pid)
def s(o, pid):
    e = ent(o, pid); return decode_utf16le(e.data) if e else ''
def a(o, pid):
    e = ent(o, pid); return str(AUID(bytes_le=e.data)) if e else '-'
def wref(o, pid):
    e = ent(o, pid)
    if e is None: return '-'
    return str(e.ref) if hasattr(e, 'ref') and e.ref is not None else a(o, pid)
def b(o, pid):
    e = ent(o, pid); return '1' if e and e.data == b'\x01' else '0'
def u16(o, pid):
    e = ent(o, pid); return str(unpack(b'<H', e.data)[0]) if e else '-'
def u32(o, pid):
    e = ent(o, pid); return str(unpack(b'<I', e.data)[0]) if e else '-'
def byte(o, pid):
    e = ent(o, pid); return str(e.data[0]) if e else '-'
def names(o, pid):
    e = ent(o, pid); return list(iter_utf16_array(e.data)) if e else []
def auids(o, pid):
    e = ent(o, pid)
    if not e: return []
    d = bytes(e.data)
    return [str(AUID(bytes_le=d[i:i+16])) for i in range(0, len(d), 16)]

CAT = {'0204':'int','0205':'strongref','0206':'weakref','0207':'enum','0208':'fixed',
       '0209':'var','020a':'set','020b':'string','020c':'stream','020d':'record',
       '020e':'rename','0220':'extenum','0221':'indirect','0222':'opaque','0223':'char'}

def typedef_row(t):
    cat = CAT.get(str(t.class_id)[9:13], 'unknown')
    if cat == 'int': d = '%s,%s' % (byte(t, P['INT_SIZE']), b(t, P['INT_SIGNED']))
    elif cat == 'strongref': d = wref(t, P['SREF'])
    elif cat == 'weakref': d = '%s;%s' % (wref(t, P['WREF']), ','.join(auids(t, P['WREF_SET'])))
    elif cat == 'enum':
        e = ent(t, P['ENUM_VALUES'])
        vals = unpack(b'<%dq' % (len(e.data)//8), bytes(e.data)) if e else ()
        d = '%s;%s' % (wref(t, P['ENUM_TYPE']),
                       ','.join('%d=%s' % (v, n) for v, n in zip(vals, names(t, P['ENUM_NAMES']))))
    elif cat == 'fixed': d = '%s,%s' % (wref(t, P['FIXED_TYPE']), u32(t, P['FIXED_COUNT']))
    elif cat == 'var': d = wref(t, P['VAR_TYPE'])
    elif cat == 'set': d = wref(t, P['SET_TYPE'])
    elif cat == 'string': d = wref(t, P['STR_TYPE'])
    elif cat == 'record':
        e = ent(t, P['REC_TYPES'])
        refs = [str(r) for r in e.references] if e is not None and hasattr(e, 'references') else []
        d = ','.join('%s=%s' % (n, r) for n, r in zip(names(t, P['REC_NAMES']), refs))
    elif cat == 'rename': d = wref(t, P['RENAME'])
    elif cat == 'extenum':
        d = ','.join('%s=%s' % (v, n) for v, n in zip(auids(t, P['EXT_VALUES']), names(t, P['EXT_NAMES'])))
    else: d = '-'
    return 'T\t%s\t%s\t%s\t%s' % (a(t, P['AUID']), s(t, P['NAME']), cat, d)

def run(path, out):
    rows = []
    with aaf2.open(path, 'r') as f:
        md = f.metadict
        # What the file itself stores, so a dynamic pid it assigned is kept.
        in_file = set()
        for c in md['ClassDefinitions'].values():
            pd = c.property_entries.get(P['PROPERTIES'])
            if pd is not None:
                for p in pd.values():
                    in_file.add(a(p, P['AUID']).lower())
        for c in md.classdefs_by_auid.values():
            props = []
            pd = c.property_entries.get(P['PROPERTIES'])
            if pd is not None:
                for p in sorted(pd.values(), key=lambda p: int(u16(p, P['PID']))):
                    prop_auid = a(p, P['AUID']).lower()
                    if prop_auid in DYNAMIC and prop_auid not in in_file:
                        continue
                    props.append(':'.join([u16(p, P['PID']), s(p, P['NAME']), a(p, P['AUID']),
                                           a(p, P['TYPE']), b(p, P['OPTIONAL']), b(p, P['UNIQUE'])]))
            rows.append('C\t%s\t%s\t%s\t%s\t%s' % (a(c, P['AUID']), s(c, P['NAME']),
                                                   wref(c, P['PARENT']), b(c, P['CONCRETE']),
                                                   '|'.join(props)))
        for t in md.typedefs_by_auid.values():
            rows.append(typedef_row(t))
    rows.sort()
    with open(out, 'w') as o:
        o.write('# C\tauid\tname\tparent\tconcrete\tpid:name:auid:type:optional:unique|...\n')
        o.write('# T\tauid\tname\tcategory\tdetail\n')
        o.write('# generated from %s by pyaaf2 08dcc3d\n' % path.rsplit('/', 1)[-1])
        for r in rows:
            o.write(r + '\n')
    print(path, len([r for r in rows if r[0] == 'C']), 'classes', len([r for r in rows if r[0] == 'T']), 'types')

run(os.path.join(FIXTURES, 'empty.aaf'), os.path.join(D, 'empty.merged.tsv'))
run(os.path.join(FIXTURES, 'sector_size_512.aaf'), os.path.join(D, 'sector_size_512.merged.tsv'))
