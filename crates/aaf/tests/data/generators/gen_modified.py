"""Changes existing AAF files with upstream pyaaf2, for the modify path to match byte for byte.

`gen_written.py` has pyaaf2 write new files. This has it change existing
ones: each scenario copies an AAF file from the directory above, opens the
copy the way pyaaf2 opens a file to change it (`aaf2.open(path, 'r+')` or
`'rw'`, or for the container scenarios `CompoundFileBinary(f, 'rb+')`), makes
a scripted set of edits modelled on pyaaf2's own tests, and closes it. The
Rust tests in `tests/modify.rs` open the same file through this crate, make
the same edits through its API, and require the result to be identical to
what pyaaf2 wrote, byte for byte.

The files pyaaf2 leaves behind are mostly the file it started from, so what
is kept is the difference: `modified/<name>.patch` holds every 512-byte
block of pyaaf2's result that differs from the file it opened, and the
length of the result. The test applies it to the same starting file to get
pyaaf2's bytes back. The patch is made from pyaaf2's output and nothing else;
applying it to the base is exact, so the comparison is still against the
file upstream wrote. A scenario whose base is `@name` starts from what
scenario `name` left, so that one change can be made to what another made.

As in `gen_written.py`, `uuid.uuid4` and `datetime.now` are replaced with
deterministic sequences, and `modified/<name>.calls.tsv` lists every value
pyaaf2 asked for, in order, after a `base` line naming the file it started
from. Opening a file and changing it asks for fewer values than writing one:
pyaaf2 does not touch the header's identification or modification time when
it saves an existing file. A new mob still asks for a `MobID` and the time,
and detaching an object that owns a stream parks the stream under a
`/tmp/<uuid>` storage, which asks for a UUID.

Run it with a pyaaf2 checkout as the only argument:

    python3 gen_modified.py ~/src/pyaaf2

Pass `--keep DIR` after it to also keep each whole file pyaaf2 wrote in DIR,
which is handy when a test fails and the file is wanted in another tool.
"""

import datetime
import os
import random
import shutil
import struct
import sys
import tempfile
import uuid

PYAAF2 = sys.argv[1] if len(sys.argv) > 1 else '.'
KEEP = sys.argv[sys.argv.index('--keep') + 1] if '--keep' in sys.argv else None
HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, os.pardir)
OUT = os.path.join(DATA, 'modified')
sys.path.insert(0, os.path.join(PYAAF2, 'src'))

# As in gen_written.py: pyaaf2 encodes AAF sets by iterating a Python `set`,
# whose order depends on hash randomisation.
if os.environ.get('PYTHONHASHSEED') != '0':
    os.environ['PYTHONHASHSEED'] = '0'
    os.execv(sys.executable, [sys.executable] + sys.argv)

random.seed(0)

import aaf2  # noqa: E402
import aaf2.file  # noqa: E402
import aaf2.mobs  # noqa: E402
from aaf2.auid import AUID  # noqa: E402
from aaf2.cfb import CompoundFileBinary  # noqa: E402
from aaf2.mobid import MobID  # noqa: E402
from aaf2.rational import AAFRational  # noqa: E402

BLOCK = 512


class Sources:
    """The deterministic stand-ins for `uuid4` and `now`, with a log of use.

    The sequences start where gen_written.py's do, offset so that a value
    handed out here is never one already in a starting file.
    """

    EPOCH = datetime.datetime(2025, 1, 2, 3, 4, 5)

    def __init__(self):
        self.ids = 0
        self.times = 0
        self.log = []

    def uuid4(self):
        self.ids += 1
        n = self.ids
        value = uuid.UUID('%08x-0000-4000-8000-%012x' % (0x0dd50000 + n, n))
        self.log.append(('uuid4', str(value)))
        return value

    def now(self):
        value = self.EPOCH + datetime.timedelta(seconds=self.times)
        self.times += 1
        self.log.append(('now', value.strftime('%Y-%m-%dT%H:%M:%S')))
        return value


def patch_sources(sources):
    """Points every place pyaaf2 reads the time or a fresh UUID at `sources`."""

    class FixedDateTime(datetime.datetime):
        @classmethod
        def now(cls, tz=None):
            return sources.now()

    class FixedDateTimeModule:
        datetime = FixedDateTime

    uuid.uuid4 = sources.uuid4
    aaf2.file.uuid4 = sources.uuid4
    aaf2.file.datetime = FixedDateTimeModule
    aaf2.mobs.datetime = FixedDateTime
    aaf2.file.sys = type('sys', (), {'platform': 'linux'})


def make_patch(base, result):
    """The blocks of `result` that differ from `base`, and its length."""
    blocks = []
    for index in range((len(result) + BLOCK - 1) // BLOCK):
        start = index * BLOCK
        new = result[start:start + BLOCK]
        if new != base[start:start + BLOCK]:
            blocks.append((index, new.ljust(BLOCK, b'\0')))
    out = [b'AAFPATCH', struct.pack('<QII', len(result), BLOCK, len(blocks))]
    for index, data in blocks:
        out.append(struct.pack('<I', index))
        out.append(data)
    return b''.join(out), len(blocks)


RESULTS = {}


def read_base(base):
    """The bytes of a starting file: a fixture, or `@name` for what an
    earlier scenario left."""
    if base.startswith('@'):
        return RESULTS[base[1:]]
    with open(os.path.join(DATA, base), 'rb') as f:
        return f.read()


def scenario(name, base, edit, options=()):
    """Runs `edit` on a copy of `base` and records what pyaaf2 made of it."""
    sources = Sources()
    patch_sources(sources)
    original = read_base(base)
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, name + '.aaf')
        with open(path, 'wb') as f:
            f.write(original)
        edit(path)
        with open(path, 'rb') as f:
            result = f.read()
        if KEEP:
            os.makedirs(KEEP, exist_ok=True)
            shutil.copy(path, os.path.join(KEEP, name + '.aaf'))
    RESULTS[name] = result

    patch, count = make_patch(original, result)
    with open(os.path.join(OUT, name + '.patch'), 'wb') as f:
        f.write(patch)
    with open(os.path.join(OUT, name + '.calls.tsv'), 'w', newline='\n') as out:
        out.write('# Every nondeterministic value pyaaf2 asked for while changing '
                  '%s into %s, in order.\n' % (base, name))
        out.write('base\t%s\n' % base)
        for option, on in options:
            out.write('option\t%s\t%s\n' % (option, 'true' if on else 'false'))
        for kind, value in sources.log:
            out.write('%s\t%s\n' % (kind, value))
    print('%-28s %-22s %7d -> %7d bytes, %4d blocks differ, %d calls'
          % (name, base, len(original), len(result), count, len(sources.log)))


# --- the container alone ------------------------------------------------------

def cfb(edit):
    def run(path):
        with open(path, 'rb+') as f:
            c = CompoundFileBinary(f, 'rb+')
            edit(c)
            c.close()
    return run


def cfb_noop(c):
    pass


def pattern(n, seed):
    return bytes((i * 31 + seed) % 251 for i in range(n))


def cfb_edits(c):
    # Grow a mini stream past the cutoff, so it moves into the FAT.
    s = c.find('/Header-2/properties').open('rw')
    data = bytes(s.read())
    s = c.find('/Header-2/properties').open('rw')
    s.write(data + pattern(5000, 1))
    s.truncate()
    # Shrink another mini stream, freeing mini sectors.
    entry = c.find('/MetaDictionary-1/properties')
    s = entry.open('rw')
    s.write(pattern(10, 2))
    s.truncate()
    # A new storage with a stream written a piece at a time.
    c.makedir('/Added')
    s = c.open('/Added/data', 'w')
    for i in range(9):
        s.write(pattern(700, i))
    # Move a stream into it, then take a storage out and a stream.
    c.move('/Header-2/Content-3b03/Mobs-1901 index', '/Added/moved')
    c.rmtree('/Header-2/Dictionary-3b04/DataDefinitions-2605{0}')
    c.remove('/Added/moved')
    # And allocate again, into what was freed.
    s = c.open('/Added/again', 'w')
    s.write(pattern(300, 7))
    s = c.open('/Added/big', 'w')
    s.write(pattern(9000, 8))


# --- files ------------------------------------------------------------------------

def aaf(mode, edit, **kwargs):
    def run(path):
        with aaf2.open(path, mode, **kwargs) as f:
            edit(f)
    return run


def noop(f):
    pass


def mobs_by_name(f):
    """written_mobs.aaf's mobs, in the order the file lists them."""
    return list(f.content.mobs)


def edit_properties(f):
    # Change, add and delete properties on objects already in the file.
    comp, _, _, _, master = mobs_by_name(f)
    comp.name = 'Mobs Test, renamed'
    comp['AppCode'].value = 7
    timecode_slot = comp.slots[0]
    del timecode_slot['SlotName']
    picture_slot = comp.slots[1]
    picture_slot['PhysicalTrackNumber'].value = 9
    sequence = picture_slot.segment
    clip = sequence['Components'][0]
    clip['StartTime'].value = 12
    clip['Length'].value = 48
    sequence['Length'].value = 48
    master.comments['Scene'] = '14B'
    master.comments['Camera'] = 'B'
    marker = comp.slots[3].segment['Components'][0]
    marker['Comment'].value = 'Changed'


def add_mob(f):
    # A new composition with a clip of a master mob already in the file.
    master = mobs_by_name(f)[4]
    comp = f.create.CompositionMob('Added')
    comp.usage = 'Usage_TopLevel'
    f.content.mobs.append(comp)
    slot = comp.create_timeline_slot(edit_rate=24)
    slot.name = 'V1'
    sequence = f.create.Sequence(media_kind='picture')
    sequence.components.value = []
    slot.segment = sequence
    clip = master.create_source_clip(1, start=0, length=24)
    sequence.components.append(clip)
    sequence.components.append(f.create.Filler('picture', 12))
    sequence.length = 36
    timecode = comp.create_timeline_slot(edit_rate=24)
    timecode.segment = f.create.Timecode(24, False, 36)
    comp.comments['Added'] = 'yes'


def remove_mob(f):
    # Take a mob out, a slot out of another, and a marker out of a sequence.
    comp, _, source, _, _ = mobs_by_name(f)
    f.content.mobs.pop(source.mob_id)
    comp.slots.pop(0)
    events = comp.slots[2].segment
    events['Components'].pop(0)
    events['Length'].value = 0


def mob_id_swap(f):
    # pyaaf2's test_mob_id_swap: a new MobID re-keys the mob in its set.
    comp = mobs_by_name(f)[0]
    comp.mob_id = MobID.new()


BLUR = AUID('6e2b1e36-0b43-4bd1-9c5a-8f3c2a1d0e01')
BLUR_AMOUNT = AUID('6e2b1e36-0b43-4bd1-9c5a-8f3c2a1d0e02')
LINEAR_INTERP = AUID('5b6c85a4-0ede-11d3-80a9-006008143e6f')
REEL_PROPERTY = AUID('6e2b1e36-0b43-4bd1-9c5a-8f3c2a1d0e03')
STRING_TYPE = AUID('01100200-0000-0000-060e-2b3401040101')


def definitions(f):
    # New definitions in the dictionary and a new property on a class in
    # the file, then objects that use them.
    comp = mobs_by_name(f)[0]
    mob_class = f.metadict.lookup_classdef('Mob')
    mob_class.register_propertydef('ReelTag', REEL_PROPERTY, None,
                                   STRING_TYPE, True, False)
    comp['ReelTag'].value = 'Reel 7'

    opdef = f.create.OperationDef(BLUR, 'Blur', 'A blur')
    f.dictionary.register_def(opdef)
    opdef.media_kind = 'picture'
    opdef['IsTimeWarp'].value = False
    opdef['NumberInputs'].value = 1
    rational = f.dictionary.lookup_typedef('Rational')
    amount = f.create.ParameterDef(BLUR_AMOUNT, 'Amount', 'How much', rational)
    f.dictionary.register_def(amount)
    opdef['ParametersDefined'].append(amount)
    interp = f.create.InterpolationDef(LINEAR_INTERP, 'LinearInterp', 'LinearInterp')
    f.dictionary.register_def(interp)

    slot = comp.create_timeline_slot(edit_rate=24)
    opgroup = f.create.OperationGroup(opdef, 24)
    slot.segment = opgroup
    opgroup.segments.append(f.create.Filler('picture', 24))
    constant = f.create.ConstantValue(amount, AAFRational(3, 4))
    opgroup.parameters.append(constant)


SHOT_NOTES = AUID('6e2b1e36-0b43-4bd1-9c5a-8f3c2a1d0e04')
NOTE_PROPERTY = AUID('6e2b1e36-0b43-4bd1-9c5a-8f3c2a1d0e05')


def new_class(f):
    # pyaaf2's test_register, on a file that exists: a class of our own,
    # with a property of its own, and an object of it on a marker.
    classdef = f.metadict.register_classdef('ShotNotes', SHOT_NOTES,
                                            'DescriptiveFramework', True)
    classdef.register_propertydef('Note', NOTE_PROPERTY, None, STRING_TYPE,
                                  True, False)
    comp = mobs_by_name(f)[0]
    marker = comp.slots[3].segment['Components'][0]
    notes = f.create.ShotNotes()
    notes['Note'].value = 'Soft focus'
    marker['Description'].value = notes


def rewrite_all(f):
    # pyaaf2's test_rewrite: every object written again, changed or not.
    for obj, streams in f.root.walk_references():
        f.manager.add_modified(obj)


def grow(f):
    # A name long enough to push the mob's properties out of the mini stream.
    comp = mobs_by_name(f)[0]
    comp.name = 'Grown ' * 500


def shrink(f):
    # And back, freeing the sectors it took.
    comp = mobs_by_name(f)[0]
    comp.name = 'Shrunk'


def reattach(f):
    # pyaaf2's test_reattach: every mob out of the file and back in.
    mobs = list(f.content['Mobs'].value)
    f.content['Mobs'].value = []
    f.content['Mobs'].value = mobs


def reattach_and_grow(f):
    # And in a 512-byte-sector file, a name that moves one mob's properties
    # out of the mini stream into sectors of their own.
    reattach(f)
    mobs = list(f.content.mobs)
    mobs[0].name = 'Grown ' * 800


def essence_parking(f):
    # Essence data owns a stream. Taking it out of the file parks the
    # stream under /tmp; putting it back moves it home; saving drops the rest.
    _, source_a, source_b, _, _ = mobs_by_name(f)
    kept = f.create.EssenceData()
    kept.mob_id = source_a.mob_id
    f.content.essencedata.append(kept)
    s = kept.open('w')
    s.write(pattern(6000, 3))
    dropped = f.create.EssenceData()
    dropped.mob_id = source_b.mob_id
    f.content.essencedata.append(dropped)
    s = dropped.open('w')
    s.write(pattern(900, 4))
    f.content.essencedata.pop(source_a.mob_id)
    f.content.essencedata.pop(source_b.mob_id)
    f.content.essencedata.append(kept)


def rewrite_essence(f):
    # Essence already in the file, written again shorter: the stream moves
    # from sectors of its own into the mini stream.
    data = list(f.content.essencedata)[0]
    s = data.open('w')
    s.write(pattern(700, 5))


def drop_essence(f):
    # Essence already in the file, taken out: its stream is parked, then
    # dropped when the file is saved.
    data = list(f.content.essencedata)[0]
    f.content.essencedata.pop(data.mob_id)


def rename_only(f):
    comp = mobs_by_name(f)[0]
    comp.name = 'Without extensions'


# --- running them --------------------------------------------------------------

os.makedirs(OUT, exist_ok=True)
for stale in os.listdir(OUT):
    os.remove(os.path.join(OUT, stale))

scenario('cfb_noop_4096', 'empty.aaf', cfb(cfb_noop))
scenario('cfb_noop_512', 'sector_size_512.aaf', cfb(cfb_noop))
scenario('cfb_edits_512', 'sector_size_512.aaf', cfb(cfb_edits))
scenario('cfb_edits_4096', 'written_mobs.aaf', cfb(cfb_edits))

scenario('noop_empty', 'empty.aaf', aaf('r+', noop))
scenario('noop_written', 'written_mobs.aaf', aaf('r+', noop))
scenario('edit_properties', 'written_mobs.aaf', aaf('r+', edit_properties))
scenario('add_mob', 'written_mobs.aaf', aaf('rw', add_mob))
scenario('remove_mob', 'written_mobs.aaf', aaf('r+', remove_mob))
scenario('mob_id_swap', 'written_mobs.aaf', aaf('rw', mob_id_swap))
scenario('definitions', 'written_mobs.aaf', aaf('rw', definitions))
scenario('new_class', 'written_mobs.aaf', aaf('rw', new_class))
scenario('rewrite_all', 'written_mobs.aaf', aaf('rw', rewrite_all))
scenario('grow', 'written_mobs.aaf', aaf('r+', grow))
scenario('shrink', '@grow', aaf('r+', shrink))
scenario('essence_parking', 'written_mobs.aaf', aaf('rw', essence_parking))
scenario('rewrite_essence', '@essence_parking', aaf('r+', rewrite_essence))
scenario('drop_essence', '@essence_parking', aaf('r+', drop_essence))
scenario('reattach_512', 'written_sequence.aaf', aaf('r+', reattach_and_grow))
scenario('without_extensions', 'written_mobs.aaf',
         aaf('r+', rename_only, extensions=False), [('extensions', False)])
