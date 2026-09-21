"""Baselines for the AAF to OTIO mapping, from upstream's own Python adapter.

`simplify` and `attach_markers` are off on purpose. With them on, a baseline
would exercise the whole pipeline at once — transcription, then three passes
that reshape the result — and a mismatch would not say which part disagrees.
Off, it is the structural transcription alone, which is the piece being ported
first. The passes get their own baselines when they get ported.

Run with a checkout of pyaaf2 and of otio-aaf-adapter:

    python3 gen_otio.py ~/src/pyaaf2 ~/src/otio-aaf-adapter
"""

import os
import sys

PYAAF2 = sys.argv[1] if len(sys.argv) > 1 else '.'
ADAPTER = sys.argv[2] if len(sys.argv) > 2 else '.'
D = os.path.join(os.path.dirname(os.path.abspath(__file__)), os.pardir)
sys.path.insert(0, os.path.join(PYAAF2, 'src'))
sys.path.insert(0, os.path.join(ADAPTER, 'src'))

import opentimelineio as otio
from otio_aaf_adapter.adapters import advanced_authoring_format as adapter


def run(path, out):
    result = adapter.read_from_file(path, simplify=False, attach_markers=False)
    text = otio.adapters.write_to_string(result, 'otio_json')
    with open(out, 'w') as o:
        o.write(text)
        if not text.endswith('\n'):
            o.write('\n')
    print(path, '->', out, len(text), 'bytes')


FIXTURES = os.path.join(PYAAF2, 'tests', 'test_files')
D = os.path.dirname(os.path.abspath(__file__))

run(os.path.join(FIXTURES, 'empty.aaf'), os.path.join(D, 'empty.otio.json'))
run(os.path.join(FIXTURES, 'sector_size_512.aaf'),
    os.path.join(D, 'sector_size_512.otio.json'))
