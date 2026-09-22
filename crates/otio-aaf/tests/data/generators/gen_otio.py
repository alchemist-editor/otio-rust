"""Baselines for reading AAF as OTIO, from upstream's own Python adapter.

Each fixture gets two baselines:

- `<name>.structural.otio.json`, read with `simplify=False` and
  `attach_markers=False`. That is the transcription on its own, with only the
  one pass upstream always runs, so a mismatch there is in the mapping and
  not in the passes that reshape it.
- `<name>.otio.json`, read with upstream's defaults, which is what a caller
  of either library gets.
- `<name>.baked.otio.json`, read with `bake_keyframed_properties=True`, for
  the fixtures where that changes anything: those with keyframed effects.
- `<name>.log` and `<name>.structural.log`, what upstream prints with
  `transcribe_log=True` in the two ways of reading.

Run with a checkout of pyaaf2 and of otio-aaf-adapter, and `opentimelineio`
installed:

    python3 gen_otio.py ~/src/pyaaf2 ~/src/otio-aaf-adapter

With `--all DIR` it instead writes both baselines for every sample AAF in the
adapter's own test data into DIR, for checking the port against the whole
corpus rather than the part of it vendored here.
"""

import contextlib
import glob
import io
import os
import re
import sys

args = [a for a in sys.argv[1:] if a != '--all']
ALL = sys.argv[sys.argv.index('--all') + 1] if '--all' in sys.argv else None
if ALL:
    args.remove(ALL)
PYAAF2 = args[0] if len(args) > 0 else '.'
ADAPTER = args[1] if len(args) > 1 else '.'
sys.path.insert(0, os.path.join(PYAAF2, 'src'))
sys.path.insert(0, os.path.join(ADAPTER, 'src'))

import opentimelineio as otio  # noqa: E402
from otio_aaf_adapter.adapters import advanced_authoring_format as adapter  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, os.pardir)
# The two fixtures from pyaaf2's test suite live in the `aaf` crate.
AAF_CRATE = os.path.join(HERE, os.pardir, os.pardir, os.pardir, os.pardir,
                         'aaf', 'tests', 'data')

# OTIO 0.18, which the adapter targets, names a marker's colour; OTIO 0.19,
# which this workspace targets, gives it a colour object, and gives a
# transition an `enabled` flag. OTIO 0.19 upgrades the one to the other on
# reading, so this writes what that upgrade gives: the same file, as 0.19
# writes it. Every name OTIO 0.18 allows is here.
COLORS = {
    'PINK': ('1.0', '0.0', '1.0', 'Pink'),
    'RED': ('1.0', '0.0', '0.0', 'Red'),
    'ORANGE': ('1.0', '0.5', '0.0', 'Orange'),
    'YELLOW': ('1.0', '1.0', '0.0', 'Yellow'),
    'GREEN': ('0.0', '1.0', '0.0', 'Green'),
    'CYAN': ('0.0', '1.0', '1.0', 'Cyan'),
    'BLUE': ('0.0', '0.0', '1.0', 'Blue'),
    'PURPLE': ('0.5', '0.0', '0.5', 'Purple'),
    'MAGENTA': ('1.0', '0.0', '1.0', 'Magenta'),
    'BLACK': ('0.0', '0.0', '0.0', 'Black'),
    'WHITE': ('1.0', '1.0', '1.0', 'White'),
}


def to_019(text):
    if not otio.__version__.startswith('0.18'):
        # Anything else needs this function checked against it first.
        sys.exit('written against OTIO 0.18, found ' + otio.__version__)
    text = text.replace('"OTIO_SCHEMA": "Marker.2"', '"OTIO_SCHEMA": "Marker.3"')

    def color(m):
        indent, (r, g, b, name) = m.group(1), COLORS[m.group(2)]
        inner = indent + '    '
        return (f'{indent}"color": {{\n'
                f'{inner}"OTIO_SCHEMA": "Color.1",\n'
                f'{inner}"r": {r},\n{inner}"g": {g},\n{inner}"b": {b},\n'
                f'{inner}"a": 1.0,\n{inner}"name": "{name}"\n'
                f'{indent}}},')

    text = re.sub(r'^( *)"color": "(' + '|'.join(COLORS) + r')",$',
                  color, text, flags=re.M)
    return re.sub(r'^( *)"transition_type": (".*")$',
                  lambda m: (f'{m.group(1)}"transition_type": {m.group(2)},\n'
                             f'{m.group(1)}"enabled": true'),
                  text, flags=re.M)


def read(path, **options):
    """What upstream reads, as the OTIO 0.19 JSON the tests compare with."""
    result = adapter.read_from_file(path, **options)
    text = to_019(otio.adapters.write_to_string(result, 'otio_json'))
    return text if text.endswith('\n') else text + '\n'


def log(path, **options):
    """What upstream prints while it reads with `transcribe_log=True`."""
    printed = io.StringIO()
    with contextlib.redirect_stdout(printed):
        adapter.read_from_file(path, transcribe_log=True, **options)
    return printed.getvalue()


def save(out, text):
    with open(out, 'w', newline='\n', encoding='utf-8') as o:
        o.write(text)
    print(out, len(text), 'bytes')


def run(path, out_dir):
    name = os.path.basename(path)[:-len('.aaf')]
    structural = dict(simplify=False, attach_markers=False)
    default = read(path)
    save(os.path.join(out_dir, name + '.structural.otio.json'), read(path, **structural))
    save(os.path.join(out_dir, name + '.otio.json'), default)
    baked = read(path, bake_keyframed_properties=True)
    if baked != default:
        save(os.path.join(out_dir, name + '.baked.otio.json'), baked)
    save(os.path.join(out_dir, name + '.structural.log'), log(path, **structural))
    save(os.path.join(out_dir, name + '.log'), log(path))


def main():
    if ALL:
        for path in sorted(glob.glob(os.path.join(ADAPTER, 'tests', 'sample_data', '*.aaf'))):
            run(path, ALL)
    else:
        for name in ['empty', 'sector_size_512']:
            run(os.path.join(AAF_CRATE, name + '.aaf'), DATA)
        for path in sorted(glob.glob(os.path.join(DATA, '*.aaf'))):
            run(path, DATA)


# `gen_written.py` imports this module for `to_019`, so the baselines and the
# inputs it writes from are upgraded by the one function.
if __name__ == '__main__':
    main()
