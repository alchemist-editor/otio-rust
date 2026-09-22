"""Writes AAF files with upstream pyaaf2, for the write path to match byte for byte.

Everything else in this directory *reads* an AAF file with pyaaf2 and records
what it found. This one runs pyaaf2's *writer*: it builds a few small files
through the same API the OpenTimelineIO adapter's `aaf_writer.py` uses, and
the Rust tests build the same logical content through this crate's write API
and assert the two files are identical, byte for byte.

pyaaf2 is not deterministic on its own. A new file carries a random
`GenerationAUID`, every new mob a random `MobID` and the time it was made, and
the header the time it was saved. So this replaces `uuid.uuid4` and
`datetime.now` with deterministic sequences before any of that runs, and writes
a sidecar beside each file listing every value it handed out, in the order it
handed them out. The Rust test feeds its writer from that sidecar and checks it
used every entry, so a writer that asks for an identifier pyaaf2 did not, or
asks at a different point, fails there with a readable message rather than as
a byte mismatch somewhere in the middle of a directory.

pyaaf2 also imports `random`, for one routine that rebalances a storage's
directory tree by inserting its children in shuffled order. Nothing in pyaaf2
calls that routine, so no file written here depends on it; the seed below is
set anyway, so that a future pyaaf2 that does call it fails reproducibly.

Run it with a pyaaf2 checkout as the only argument:

    python3 gen_written.py ~/src/pyaaf2
"""

import datetime
import os
import random
import struct
import sys
import uuid
import wave

PYAAF2 = sys.argv[1] if len(sys.argv) > 1 else '.'
HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, os.pardir)
sys.path.insert(0, os.path.join(PYAAF2, 'src'))

# Hash randomisation changes the iteration order of a Python `set`, and
# pyaaf2 encodes AAF set types by iterating one. Nothing written here has more
# than one element in a set, but re-running under a fixed seed means a future
# fixture that does cannot silently depend on the run.
if os.environ.get('PYTHONHASHSEED') != '0':
    os.environ['PYTHONHASHSEED'] = '0'
    os.execv(sys.executable, [sys.executable] + sys.argv)

random.seed(0)

import aaf2  # noqa: E402
import aaf2.file  # noqa: E402
import aaf2.mobs  # noqa: E402
from aaf2.auid import AUID  # noqa: E402
from aaf2.mobid import MobID  # noqa: E402
from aaf2.rational import AAFRational  # noqa: E402


class Sources:
    """The deterministic stand-ins for `uuid4` and `now`, with a log of use."""

    # The first time handed out; each call to `now` is one second later.
    EPOCH = datetime.datetime(2024, 5, 6, 7, 8, 9)

    def __init__(self):
        self.ids = 0
        self.times = 0
        self.log = []

    def uuid4(self):
        self.ids += 1
        n = self.ids
        value = uuid.UUID('%08x-0000-4000-8000-%012x' % (0x5eed0000 + n, n))
        self.log.append(('uuid4', str(value)))
        return value

    def now(self):
        value = self.EPOCH + datetime.timedelta(seconds=self.times)
        self.times += 1
        self.log.append(('now', value.strftime('%Y-%m-%dT%H:%M:%S')))
        return value


def patch(sources):
    """Points every place pyaaf2 reads the time or a fresh UUID at `sources`."""

    class FixedDateTime(datetime.datetime):
        @classmethod
        def now(cls, tz=None):
            return sources.now()

    class FixedDateTimeModule:
        datetime = FixedDateTime

    # `mobid` looks `uuid.uuid4` up at call time; `file` imported the name.
    uuid.uuid4 = sources.uuid4
    aaf2.file.uuid4 = sources.uuid4
    # `file` calls `datetime.datetime.now()`; `mobs` imported the class.
    aaf2.file.datetime = FixedDateTimeModule
    aaf2.mobs.datetime = FixedDateTime
    # The platform is written into the file's identification.
    aaf2.file.sys = type('sys', (), {'platform': 'linux'})


def write(name, build, sector_size=4096):
    sources = Sources()
    patch(sources)
    path = os.path.join(DATA, name + '.aaf')
    with aaf2.open(path, 'w', sector_size=sector_size) as f:
        build(f)

    with open(os.path.join(DATA, name + '.calls.tsv'), 'w', newline='\n') as out:
        out.write('# Every nondeterministic value pyaaf2 asked for while writing '
                  '%s.aaf, in order.\n' % name)
        out.write('sector_size\t%d\n' % sector_size)
        for kind, value in sources.log:
            out.write('%s\t%s\n' % (kind, value))
    print(name, os.path.getsize(path), 'bytes,', len(sources.log), 'calls')


# --- (a) a new file with nothing added ---------------------------------------

def build_empty(f):
    pass


# --- (b) a composition: clips, a filler and a dissolve in one sequence -------

CLIP_MOB = MobID('urn:smpte:umid:060a2b34.01010105.01010f20.13000000.'
                 '11111111.2222.3333.4444.555555555555')
DISSOLVE = AUID('0c3bea40-fc05-11d2-8a29-0050040ef7d2')


def build_sequence(f):
    comp = f.create.CompositionMob()
    comp.name = 'Sequence Test'
    comp.usage = 'Usage_TopLevel'
    f.content.mobs.append(comp)

    slot = comp.create_timeline_slot(edit_rate=24)
    slot.name = 'V1'
    slot['PhysicalTrackNumber'].value = 1

    sequence = f.create.Sequence(media_kind='picture')
    sequence.components.value = []
    slot.segment = sequence

    clip = f.create.SourceClip(start=10, length=48, mob_id=CLIP_MOB,
                               slot_id=1, media_kind='picture')
    sequence.components.append(clip)

    sequence.components.append(f.create.Filler('picture', 24))

    opdef = f.create.OperationDef(DISSOLVE, 'VideoDissolve', 'Video dissolve')
    f.dictionary.register_def(opdef)
    opdef.media_kind = 'picture'
    opdef['IsTimeWarp'].value = False
    opdef['NumberInputs'].value = 2
    opdef['OperationCategory'].value = 'OperationCategory_Effect'
    opdef['Bypass'].value = 1

    opgroup = f.create.OperationGroup(opdef, 12)
    transition = f.create.Transition('picture', 12)
    transition['OperationGroup'].value = opgroup
    transition['CutPoint'].value = 6
    sequence.components.append(transition)

    clip = f.create.SourceClip(start=0, length=36, mob_id=CLIP_MOB,
                               slot_id=1, media_kind='picture')
    sequence.components.append(clip)
    sequence.length = 48 + 24 - 12 + 36


# --- (c) the source chain, as the OpenTimelineIO adapter writes one ----------

PAN_PARAMETER = AUID('e4962322-2267-11d3-8a4c-0050040ef7d2')
MONO_AUDIO_PAN = AUID('9d2ea893-0968-11d3-8a38-0050040ef7d2')
LEVEL_PARAMETER = AUID('e4962320-2267-11d3-8a4c-0050040ef7d2')
EXTRAPOLATION = AUID('0e24dd54-66cd-4f1a-b0a0-670ac3a7a0b3')
LINEAR_INTERP = AUID('5b6c85a4-0ede-11d3-80a9-006008143e6f')


def build_mobs(f):
    # The adapter registers Avid's extended marker colour first.
    comment_marker = f.metadict.lookup_classdef('CommentMarker')
    comment_marker.register_propertydef(
        'CommentMarkerColorExtended',
        'e96e6d45-c383-11d3-a069-006094eb75cb',
        0xffda,
        'e96e6d43-c383-11d3-a069-006094eb75cb',
        False,
        False,
    )

    comp = f.create.CompositionMob()
    comp.name = 'Mobs Test'
    comp.usage = 'Usage_TopLevel'
    f.content.mobs.append(comp)
    comp.comments['Project'] = 'Writing test'
    attributes = aaf2.misc.TaggedValueHelper(comp['MobAttributeList'])
    attributes['_IMPORTSETTING'] = 1

    # A tape: an import descriptor replaced by a tape descriptor, a picture
    # slot twelve hours long and a timecode slot.
    tape = f.create.SourceMob()
    tape.name = 'A001C003'
    tape.descriptor = f.create.ImportDescriptor()
    tape_slot, tc_slot = tape.create_tape_slots('A001C003', edit_rate=24,
                                                timecode_fps=24,
                                                drop_frame=False)
    tc_slot.segment.start = 86400
    tc_slot.segment.length = 240
    f.content.mobs.append(tape)
    locator = f.create.NetworkLocator()
    locator['URLString'].value = 'file:///media/A001C003.mov'
    tape.descriptor['Locator'].append(locator)

    tape_clip_slot = tape.create_empty_slot(24, 'picture')
    tape_clip_slot.segment.length = 240
    tape_clip_slot.segment.start = 86400

    # A file: a CDCI picture descriptor with a network locator.
    filemob = f.create.SourceMob()
    f.content.mobs.append(filemob)
    descriptor = f.create.CDCIDescriptor()
    descriptor['ComponentWidth'].value = 8
    descriptor['HorizontalSubsampling'].value = 2
    descriptor['ImageAspectRatio'].value = '16/9'
    descriptor['StoredWidth'].value = 1920
    descriptor['StoredHeight'].value = 1080
    descriptor['FrameLayout'].value = 'FullFrame'
    descriptor['VideoLineMap'].value = [42, 0]
    descriptor['SampleRate'].value = str(AAFRational(24))
    descriptor['Length'].value = 240
    locator = f.create.NetworkLocator()
    locator['URLString'].value = 'file:///media/A001C003.mov'
    descriptor['Locator'].append(locator)
    filemob.descriptor = descriptor
    file_slot = filemob.create_timeline_slot(24)
    file_clip = filemob.create_source_clip(slot_id=file_slot.slot_id,
                                           length=240, media_kind='picture')
    file_clip.mob = tape
    file_clip.slot = tape_clip_slot
    file_clip.slot_id = tape_clip_slot.slot_id
    file_slot.segment = file_clip

    # A sound file, described by PCM.
    soundmob = f.create.SourceMob()
    f.content.mobs.append(soundmob)
    pcm = f.create.PCMDescriptor()
    pcm['AverageBPS'].value = 96000
    pcm['BlockAlign'].value = 2
    pcm['QuantizationBits'].value = 16
    pcm['AudioSamplingRate'].value = 48000
    pcm['Channels'].value = 1
    pcm['SampleRate'].value = 48000
    pcm['Length'].value = 480000
    soundmob.descriptor = pcm
    sound_slot = soundmob.create_timeline_slot(24)
    sound_slot.segment = f.create.SourceClip(length=240, media_kind='sound')

    # The master mob, with comments of each kind the adapter writes.
    master = f.create.MasterMob()
    master.name = 'A001C003'
    master.mob_id = MobID('urn:smpte:umid:060a2b34.01010105.01010f20.13000000.'
                          'aaaaaaaa.bbbb.cccc.dddd.eeeeeeeeeeee')
    f.content.mobs.append(master)
    master.comments['Scene'] = '12A'
    master.comments['Take'] = 3
    master.comments['Speed'] = AAFRational(24000, 1001)
    master_slot = master.create_timeline_slot(edit_rate=24, slot_id=1)
    master_clip = master.create_source_clip(slot_id=master_slot.slot_id,
                                            length=240, media_kind='picture')
    master_clip.mob = filemob
    master_clip.slot = file_slot
    master_clip.slot_id = file_slot.slot_id
    master_slot.segment = master_clip
    master_slot['MarkIn'].value = 10
    master_slot['MarkOut'].value = 58

    # The composition's timecode track.
    tc = comp.create_timeline_slot(24)
    tc.name = 'TC'
    tc['PhysicalTrackNumber'].value = 1
    timecode = f.create.Timecode()
    timecode.fps = 24
    timecode.drop = False
    timecode.start = 86400
    tc.segment = timecode

    # A picture track holding one clip of the master mob.
    video = comp.create_timeline_slot(edit_rate=24)
    sequence = f.create.Sequence(media_kind='picture')
    sequence.components.value = []
    video.segment = sequence
    video.name = 'V1'
    video['PhysicalTrackNumber'].value = 1
    clip = comp.create_source_clip(slot_id=video.slot_id, start=10,
                                   length=48, media_kind='picture')
    clip.mob = master
    clip.slot = master_slot
    clip.slot_id = master_slot.slot_id
    colours = aaf2.misc.TaggedValueHelper(clip['ComponentAttributeList'])
    colours['_COLOR_R'] = 65535
    sequence.components.append(clip)
    sequence.length = 48

    # A sound track: a mono pan operation around a sequence, with keyframes.
    audio = comp.create_sound_slot(edit_rate=24)
    pan = f.create.OperationDef(MONO_AUDIO_PAN, 'Audio Pan')
    pan.media_kind = 'sound'
    pan['NumberInputs'].value = 1
    f.dictionary.register_def(pan)
    opgroup = f.create.OperationGroup(pan)
    opgroup.media_kind = 'sound'
    opgroup.length = 48
    audio.segment = opgroup
    audio.name = 'A1'
    audio['PhysicalTrackNumber'].value = 1
    sound_sequence = f.create.Sequence(media_kind='sound')
    sound_sequence.components.value = []
    sound_sequence.length = 48
    opgroup.segments.append(sound_sequence)

    rational = f.dictionary.lookup_typedef('Rational')
    param_def = f.create.ParameterDef(PAN_PARAMETER, 'Pan', 'Pan', rational)
    f.dictionary.register_def(param_def)
    interp = f.create.InterpolationDef(LINEAR_INTERP, 'LinearInterp',
                                       'LinearInterp')
    f.dictionary.register_def(interp)
    varying = f.create.VaryingValue()
    varying.parameterdef = param_def
    varying['Interpolation'].value = interp
    varying['VVal_Extrapolation'].value = EXTRAPOLATION
    varying['VVal_FieldCount'].value = 1
    for time, value in (('0/48', '1/2'), ('47/48', '1/2')):
        point = f.create.ControlPoint()
        point['Time'].value = AAFRational(time)
        point['Value'].value = AAFRational(value)
        point['ControlPointSource'].value = 2
        point['EditHint'].value = 'Proportional'
        varying['PointList'].append(point)
    opgroup.parameters.append(varying)

    level = f.create.ParameterDef(LEVEL_PARAMETER, 'ParameterDef_Level', '',
                                  rational)
    f.dictionary.register_def(level)
    pan['ParametersDefined'].extend([param_def, level])
    constant = f.create.ConstantValue(level, AAFRational(1, 2))
    opgroup.parameters.append(constant)

    sound_clip = comp.create_source_clip(slot_id=audio.slot_id, length=48,
                                         media_kind='sound')
    sound_clip.mob = soundmob
    sound_clip.slot = sound_slot
    sound_clip.slot_id = sound_slot.slot_id
    sound_sequence.components.append(sound_clip)

    # A marker on the picture track, in an event slot of its own.
    events = f.create.EventMobSlot()
    events['EditRate'].value = 24
    events['SlotID'].value = 1000
    events['PhysicalTrackNumber'].value = 1
    marker_sequence = f.create.Sequence('DescriptiveMetadata')
    marker = f.create.DescriptiveMarker()
    marker['Length'].value = 1
    marker['DescribedSlots'].value = {int(video.slot_id)}
    marker['Position'].value = 12
    marker['Comment'].value = 'Check focus'
    marker['CommentMarkerUser'].value = 'editor'
    marker['CommentMarkerColor'].value = {'red': 41471, 'green': 12134,
                                          'blue': 6564}
    marker['CommentMarkerColorExtended'].value = {'red': 41471,
                                                  'green': 12134,
                                                  'blue': 6564}
    marker['CommentMarkerTime'].value = '07:08'
    marker['CommentMarkerDate'].value = '05/06/2024'
    marker_attributes = aaf2.misc.TaggedValueHelper(
        marker['CommentMarkerAttributeList'])
    marker_attributes['_ATN_CRM_COM'] = 'Check focus'
    marker_attributes['_ATN_CRM_LONG_CREATE_DATE'] = 1714979289
    marker_comments = aaf2.misc.TaggedValueHelper(marker['UserComments'])
    marker_comments['Comment'] = 'Check focus'
    marker_sequence.components.append(marker)
    events.segment = marker_sequence
    comp.slots.append(events)


# --- (d) essence imported from a raw DNxHD stream ------------------------------

# The first two frames of the DNxHD stream in otio-aaf-adapter's test data,
# vendored beside this directory; the README says how it was cut.
DNX = os.path.join(DATA, 'picchu_seq0100_snippet_dnx_2frames.dnx')

# The MobID of the master mob in otio-aaf-adapter's own embedded sample, which
# its tests name on a clip to have that mob copied out of the file.
EMBEDDED_MOB = MobID('urn:smpte:umid:060a2b34.01010105.01010f20.13000000.'
                     'd118caad.97b44c06.807ef723.fd32dc64')


def build_dnxhd(f):
    """A master mob with DNxHD essence embedded, behind a tape, as in the
    sample otio-aaf-adapter embeds from. The OpenTimelineIO adapter's tests
    copy the essence out of this file."""
    master = f.create.MasterMob('EmbeddedClip')
    master.mob_id = EMBEDDED_MOB
    f.content.mobs.append(master)

    tape_mob = f.create.SourceMob()
    tape_mob.create_tape_slots('EmbeddedClip', 24, 24)
    f.content.mobs.append(tape_mob)
    tape = tape_mob.create_source_clip(slot_id=1, length=2)

    master.import_dnxhd_essence(DNX, 24, tape)


def build_copy(f):
    """The master mob in `written_dnxhd.aaf`, its source mob and its essence,
    copied into a new file the way the OpenTimelineIO adapter copies essence
    out of another AAF: the essence, then the source mob, then the master
    mob."""
    with aaf2.open(os.path.join(DATA, 'written_dnxhd.aaf'), 'r') as src:
        master = next(src.content.mastermobs())
        source_mob = master.slots[0].segment.mob
        f.content.essencedata.append(source_mob.essence.copy(root=f))
        f.content.mobs.append(source_mob.copy(root=f))
        f.content.mobs.append(master.copy(root=f))


# --- (e) essence imported from a WAV file ---------------------------------------

# A tone written by this script with Python's `wave` module: mono, 16-bit, at
# 2000 Hz for two and a half seconds. pyaaf2 copies a WAV one second at a time,
# and at this rate a second is 4000 bytes, under the 4096 at which a stream
# leaves the mini stream, so the essence starts in the mini stream and moves
# out of it on the second write, which is the case worth reproducing.
WAV = os.path.join(DATA, 'tone.wav')


def write_tone():
    samples = bytearray()
    for i in range(5000):
        # A triangle wave, in integers, so every platform writes the same.
        phase = (i * 64) % 4096
        value = phase * 16 if phase < 2048 else (4096 - phase) * 16
        samples += struct.pack('<h', value - 16384)
    with wave.open(WAV, 'wb') as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(2000)
        w.writeframes(bytes(samples))


def build_audio(f):
    """A master mob with the tone's samples embedded, at 24 edit units a
    second, and a second one that describes the same file offline."""
    master = f.create.MasterMob('Tone')
    f.content.mobs.append(master)
    master.import_audio_essence(WAV, 24)

    offline = f.create.MasterMob('Tone offline')
    f.content.mobs.append(offline)
    offline.import_audio_essence(WAV, offline=True)


write('written_empty', build_empty)
# The one 512-byte-sector file, so both sector sizes are written.
write('written_sequence', build_sequence, sector_size=512)
write('written_mobs', build_mobs)
write('written_dnxhd', build_dnxhd)
write('written_copy', build_copy)
write_tone()
write('written_audio', build_audio)
