"""AAF files written by upstream's adapter, for the Rust writer to match byte for byte.

`gen_otio.py` runs upstream's adapter as a *reader*. This runs it as a
*writer*: for each input it takes an OTIO timeline, hands it to the adapter's
`write_to_file`, and keeps the AAF that pyaaf2 saved. The Rust tests write the
same timeline with this crate and assert the two files are identical, byte for
byte.

There are two kinds of input:

- AAF files already vendored beside this directory. Each is read with the
  adapter's defaults, and the timeline that comes out is written back. The
  timeline is first put through OTIO JSON and read again, so that what is
  written is exactly what the vendored `<name>.otio.json` baseline holds, and
  the script checks that the baseline is still what the adapter reads. The
  Rust test writes from that baseline.
- Timelines built here, in Python, for the parts of the writer no sample
  reaches: a clip without an AAF behind it, a marker made from scratch, a
  transition the writer skips, and the writer's options. Each is saved as
  `written/<name>.otio.json`, as OTIO 0.18 writes it, and written back from
  that file. The Rust test reads the same file, which OTIO 0.19 upgrades.

The adapter and pyaaf2 are not deterministic on their own. pyaaf2 gives a new
file a random `GenerationAUID` and every new mob a random `MobID` and the time
it was made, and stamps the file with the time it was saved; the adapter reads
the clock for a new marker's date and asks the operating system who the user
is. So this replaces `uuid.uuid4`, `datetime.now` and `getpass.getuser` with
deterministic stand-ins before anything is written, and writes a sidecar
beside each file listing every time and identifier handed out, in order, and
the options and user name the file was written with. The Rust test feeds its
writer from the sidecar and checks it asked for exactly that sequence.

The adapter reads a new marker's date as local time and also converts it to
seconds since the epoch, so the script runs with `TZ=UTC`, where the two
agree with what the Rust writer does with the same clock reading.

For each written file it also writes `<name>.roundtrip.otio.json`: what the
adapter reads back out of the file it just wrote, upgraded to OTIO 0.19 as
`gen_otio.py` upgrades its baselines. The Rust test reads its own output back
and compares.

Run it from anywhere, with a pyaaf2 checkout and an otio-aaf-adapter
checkout, and `opentimelineio` 0.18 installed:

    python3 gen_written.py ~/src/pyaaf2 ~/src/otio-aaf-adapter

One input names a file by a path relative to the crate, as a clip's media, so
the script changes into the crate's directory before writing; `cargo test`
runs the tests from there too.

With `--all DIR` it instead writes every sample AAF in the adapter's own test
data that the adapter can write back into DIR, with the input timeline beside
each, for checking the port against the whole corpus. See the README.
"""

import datetime
import glob
import os
import random
import sys
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, os.pardir)
OUT = os.path.join(DATA, 'written')
CRATE = os.path.join(DATA, os.pardir, os.pardir)
AAF_CRATE = os.path.join(CRATE, os.pardir, 'aaf', 'tests', 'data')

# Hash randomisation changes the iteration order of a Python `set`, which
# pyaaf2 encodes AAF set types by iterating, and the adapter's marker dates
# depend on the time zone. Pin both, then start again under them.
if os.environ.get('PYTHONHASHSEED') != '0' or os.environ.get('TZ') != 'UTC':
    os.environ['PYTHONHASHSEED'] = '0'
    os.environ['TZ'] = 'UTC'
    os.execv(sys.executable, [sys.executable] + sys.argv)

random.seed(0)

# `gen_otio.py` reads the same arguments, puts both checkouts on the path and
# imports the adapter; importing it here does all of that once.
sys.path.insert(0, HERE)
import gen_otio  # noqa: E402

import aaf2.file  # noqa: E402
import aaf2.mobs  # noqa: E402
import opentimelineio as otio  # noqa: E402
from otio_aaf_adapter.adapters import advanced_authoring_format as adapter  # noqa: E402
from otio_aaf_adapter.adapters.aaf_adapter import aaf_writer  # noqa: E402

# The name the adapter records on a marker it makes from scratch.
USER = 'editor'


class Sources:
    """The deterministic stand-ins for `uuid4` and `now`, with a log of use.

    The same sequences as the `aaf` crate's `gen_written.py`, so that a
    fixture from either reads the same way.
    """

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
    """Points every place pyaaf2 or the adapter reads the time, a fresh UUID
    or the user's name at `sources`."""

    class FixedDateTime(datetime.datetime):
        @classmethod
        def now(cls, tz=None):
            return sources.now()

    class FixedDateTimeModule:
        datetime = FixedDateTime

    class FixedGetpass:
        @staticmethod
        def getuser():
            return USER

    # `mobid` looks `uuid.uuid4` up at call time; `file` imported the name.
    uuid.uuid4 = sources.uuid4
    aaf2.file.uuid4 = sources.uuid4
    # `file` and the adapter's writer call `datetime.datetime.now()`; `mobs`
    # imported the class. One clock serves all three, as it does in Rust.
    aaf2.file.datetime = FixedDateTimeModule
    aaf2.mobs.datetime = FixedDateTime
    aaf_writer.datetime = FixedDateTimeModule
    aaf_writer.getpass = FixedGetpass
    # The platform is written into the file's identification.
    aaf2.file.sys = type('sys', (), {'platform': 'linux'})


def write(name, timeline, out_dir, **options):
    """Writes `timeline` as `<name>.aaf` with the adapter, and its sidecar and
    round-trip baseline beside it."""
    sources = Sources()
    patch(sources)
    path = os.path.join(out_dir, name + '.aaf')
    adapter.write_to_file(timeline, path, **options)

    with open(os.path.join(out_dir, name + '.calls.tsv'), 'w', newline='\n') as out:
        out.write('# The options %s.aaf was written with, and every nondeterministic '
                  'value the adapter and pyaaf2 asked for while writing it, in order.\n'
                  % name)
        for option, value in sorted(options.items()):
            out.write('option\t%s\t%s\n' % (option, 'true' if value else 'false'))
        out.write('user\t%s\n' % USER)
        for kind, value in sources.log:
            out.write('%s\t%s\n' % (kind, value))

    back = adapter.read_from_file(path)
    text = gen_otio.to_019(otio.adapters.write_to_string(back, 'otio_json'))
    if not text.endswith('\n'):
        text += '\n'
    with open(os.path.join(out_dir, name + '.roundtrip.otio.json'), 'w',
              newline='\n', encoding='utf-8') as out:
        out.write(text)
    print(name, os.path.getsize(path), 'bytes,', len(sources.log), 'calls')


def from_sample(path, baseline=None):
    """The timeline the adapter reads from `path`, by way of OTIO JSON.

    With `baseline`, also checks that the text is what that vendored baseline
    holds, so that the Rust test, which writes from the baseline, starts from
    the same timeline.
    """
    text = otio.adapters.write_to_string(adapter.read_from_file(path), 'otio_json')
    upgraded = gen_otio.to_019(text)
    if not upgraded.endswith('\n'):
        upgraded += '\n'
    if baseline is not None:
        with open(baseline, encoding='utf-8') as f:
            if f.read() != upgraded:
                sys.exit('%s is stale; run gen_otio.py first' % baseline)
    return otio.adapters.read_from_string(text, 'otio_json'), upgraded


# --- timelines built here -----------------------------------------------------

def rt(value, rate=24):
    return otio.opentime.RationalTime(value, rate)


def tr(start, duration, rate=24):
    return otio.opentime.TimeRange(rt(start, rate), rt(duration, rate))


def umid(tail):
    return ('urn:smpte:umid:060a2b34.01010105.01010f20.13000000.' + tail)


def dissolve_metadata(kind, cut_point):
    """What the reader leaves on a dissolve, and the writer requires."""
    return {'AAF': {
        'CutPoint': cut_point,
        'PointList': [{'Time': -0.02127659574468085, 'Value': 0.0},
                      {'Time': 1.0212765957446808, 'Value': 100.0}],
        'OperationGroup': {'Operation': {
            'DataDefinition': {'Name': kind},
            'Description': '',
            'Identification': '89d9b67e-5584-302d-9abd-8bd330c46841',
            'IsTimeWarp': False,
            'Name': 'VideoDissolve_2' if kind == 'Picture' else 'Mono Audio Dissolve',
            'NumberInputs': 2,
            'OperationCategory': 'OperationCategory_Effect',
        }},
    }}


def build_edit():
    """A cut made in Python: clips, gaps, dissolves, a nested track, markers
    old and new, comments, a clip colour, and sound."""
    timeline = otio.schema.Timeline(name='Written Edit')
    timeline.global_start_time = rt(86400)
    timeline.metadata['AAF'] = {
        'UserComments': {'Project': 'Port', 'Take': 3, 'Speed': 23.976},
        'MobAttributeList': {'_IMPORTSETTING': 1},
    }

    a003 = umid('aaaaaaaa.bbbb.cccc.dddd.eeeeeeeeeeee')
    video = otio.schema.Track(name='V1', kind=otio.schema.TrackKind.Video)
    timeline.tracks.append(video)

    first = otio.schema.Clip(
        name='A001C003',
        media_reference=otio.schema.ExternalReference(
            target_url='file:///media/A001C003.mov',
            available_range=tr(86400, 240)),
        source_range=tr(86410, 48),
        metadata={'AAF': {'MobID': a003, 'UserComments': {'Scene': '12A'}}},
    )
    first.color = otio.core.Color(1.0, 0.5, 0.0, 1.0, 'Orange')
    first.markers.append(otio.schema.Marker(
        name='Check focus', marked_range=tr(86420, 1),
        color=otio.schema.MarkerColor.GREEN))
    video.append(first)

    video.append(otio.schema.Transition(
        transition_type=otio.schema.TransitionTypes.SMPTE_Dissolve,
        in_offset=rt(6), out_offset=rt(6),
        metadata=dissolve_metadata('Picture', 6)))

    video.append(otio.schema.Clip(
        name='A001C004',
        media_reference=otio.schema.ExternalReference(
            target_url='file:///media/A001C004.mov',
            available_range=tr(0, 36),
            metadata={'AAF': {'EssenceDescription': {
                'ClassName': 'CDCIDescriptor',
                'ComponentWidth': 10,
                'StoredWidth': 3840,
                'StoredHeight': 2160,
                'ImageAspectRatio': '16/9',
                'VideoLineMap': [42, 0],
                'ColorSiting': 'CoSiting',
                'Summary': 'not a CDCI property',
            }}}),
        metadata={'AAF': {'MobID': umid('aaaaaaaa.bbbb.cccc.dddd.eeeeeeeeeeef')}},
    ))

    video.append(otio.schema.Gap(source_range=tr(0, 24)))

    # A track directly inside a track, which the writer wraps in a stack.
    nested = otio.schema.Track(name='Nested', kind=otio.schema.TrackKind.Video)
    nested.append(otio.schema.Clip(
        name='B002C001',
        media_reference=otio.schema.ExternalReference(
            target_url='file:///media/B002C001.mov',
            available_range=tr(1000, 100)),
        source_range=tr(1010, 20),
        metadata={'AAF': {'MobID': umid('bbbbbbbb.bbbb.cccc.dddd.eeeeeeeeeeee')}},
    ))
    nested.append(otio.schema.Clip(
        name='Slug',
        media_reference=otio.schema.GeneratorReference(
            generator_kind='Slug', available_range=tr(0, 12)),
        source_range=tr(0, 12),
    ))
    video.append(nested)

    video.append(otio.schema.Clip(
        name='Offline',
        media_reference=otio.schema.MissingReference(available_range=tr(0, 30)),
        source_range=tr(5, 20),
        metadata={'AAF': {'SourceID': umid('cccccccc.bbbb.cccc.dddd.eeeeeeeeeeee')}},
    ))

    # A transition of a kind the writer does not know, which it skips.
    video.append(otio.schema.Transition(
        transition_type='Custom_Wipe',
        in_offset=rt(4), out_offset=rt(4),
        metadata=dissolve_metadata('Picture', 4)))

    # The first clip's media again: its master mob is shared.
    video.append(otio.schema.Clip(
        name='A001C003',
        media_reference=otio.schema.ExternalReference(
            target_url='file:///media/A001C003.mov',
            available_range=tr(86400, 240)),
        source_range=tr(86500, 24),
        metadata={'AAF': {'MobID': a003}},
    ))

    video.markers.append(otio.schema.Marker(
        name='Track note', marked_range=tr(30, 1),
        color=otio.schema.MarkerColor.BLUE,
        metadata={'AAF': {
            'CommentMarkerUSer': 'assistant',
            'CommentMarkerDate': '01/02/2024',
            'CommentMarkerTime': '09:30',
            'CommentMarkerAttributeList': {
                '_ATN_CRM_LONG_CREATE_DATE': 1704187800,
                '_ATN_CRM_ID': '0123456789abcdef',
            },
        }}))

    audio = otio.schema.Track(name='A1', kind=otio.schema.TrackKind.Audio)
    timeline.tracks.append(audio)
    audio.append(otio.schema.Clip(
        name='A001C003',
        media_reference=otio.schema.ExternalReference(
            target_url='file:///media/A001C003.mov',
            available_range=tr(86400, 240)),
        source_range=tr(86410, 48),
        metadata={'AAF': {'MobID': a003}},
    ))
    audio.append(otio.schema.Transition(
        transition_type=otio.schema.TransitionTypes.SMPTE_Dissolve,
        in_offset=rt(3), out_offset=rt(3),
        metadata=dissolve_metadata('Sound', 3)))
    music = otio.schema.Clip(
        name='Music',
        media_reference=otio.schema.ExternalReference(
            target_url='file:///media/music.wav',
            available_range=tr(0, 480),
            metadata={'AAF': {'EssenceDescription': {
                'Channels': 2,
                'AudioSamplingRate': 44100,
                'Summary': 'not a PCM property',
            }}}),
        source_range=tr(0, 36),
        metadata={'AAF': {
            'MobID': umid('dddddddd.bbbb.cccc.dddd.eeeeeeeeeeee'),
            'Pan': {'ControlPoints': [
                {'ControlPointSource': 2, 'Time': '0/36', 'Value': '0/1'},
                {'ControlPointSource': 2, 'Time': '35/36', 'Value': '1/1'},
            ]},
        }},
    )
    music.markers.append(otio.schema.Marker(
        name='Downbeat', marked_range=tr(12, 1),
        color=otio.schema.MarkerColor.PURPLE))
    audio.append(music)
    audio.append(otio.schema.Gap(source_range=tr(0, 12)))
    return timeline


def build_options():
    """A cut for the writer's options: master mob IDs from a file, made up
    where there is none, and edge code. At 29.97, so that the tape's timecode
    is drop frame and the composition's is rounded to 30."""
    rate = 30000 / 1001
    timeline = otio.schema.Timeline(name='Written Options')

    video = otio.schema.Track(name='V1', kind=otio.schema.TrackKind.Video)
    timeline.tracks.append(video)
    video.append(otio.schema.Clip(
        name='From a file',
        # A path relative to the crate, where both this script and `cargo
        # test` run: the `aaf` crate's fixture with a single master mob,
        # whose MobID the writer takes.
        media_reference=otio.schema.ExternalReference(
            target_url='../aaf/tests/data/written_mobs.aaf',
            available_range=tr(0, 240, rate)),
        source_range=tr(10, 48, rate),
    ))
    video.append(otio.schema.Gap(source_range=tr(0, 15, rate)))
    video.append(otio.schema.Clip(
        name='No MobID',
        media_reference=otio.schema.MissingReference(
            available_range=tr(100, 60, rate)),
    ))

    audio = otio.schema.Track(name='A1', kind=otio.schema.TrackKind.Audio)
    timeline.tracks.append(audio)
    audio.append(otio.schema.Clip(
        name='No MobID either',
        media_reference=otio.schema.ExternalReference(
            target_url='file:///media/sound.wav',
            available_range=tr(0, 100, rate)),
        source_range=tr(0, 63, rate),
    ))
    return timeline


# --- embedding essence -----------------------------------------------------------

# The media the embedding timelines name, by paths relative to the crate like
# `options`'s: the `aaf` crate's fixtures, which its own tests check pyaaf2's
# import of. The DNxHD stream is two frames long, and `written_dnxhd.aaf` holds
# those two frames embedded under the master mob `EMBEDDED_MOB`, as the sample
# upstream's tests embed from does.
DNX = '../aaf/tests/data/picchu_seq0100_snippet_dnx_2frames.dnx'
EMBEDDED_AAF = '../aaf/tests/data/written_dnxhd.aaf'
WAV = '../aaf/tests/data/tone.wav'
EMBEDDED_MOB = umid('d118caad.97b44c06.807ef723.fd32dc64')


def embedding(url, available, source, kind=otio.schema.TrackKind.Video,
              clip_metadata=None, media_metadata=None):
    """One clip named `EmbeddedClip` on one track, as each of upstream's
    `test_transcribe_embed_*` tests builds it."""
    media = otio.schema.ExternalReference(target_url=url, available_range=available)
    if media_metadata:
        media.metadata['AAF'] = media_metadata
    clip = otio.schema.Clip(name='EmbeddedClip', source_range=source,
                            media_reference=media)
    if clip_metadata:
        clip.metadata['AAF'] = clip_metadata
    track = otio.schema.Track(children=[clip], kind=kind)
    return otio.schema.Timeline(tracks=[track])


def build_embed_dnx():
    """`test_transcribe_embed_dnx_data`: a raw DNxHD stream imported, starting
    a frame into its tape."""
    return embedding(DNX, tr(1, 2), tr(1, 2))


def build_embed_aaf_clip_mob_id():
    """`test_transcribe_embed_aaf_clip_mob_id`: the essence, its source mob and
    its master mob copied out of another AAF, found by the MobID on the
    clip."""
    return embedding(EMBEDDED_AAF, tr(0, 2), tr(0, 2),
                     clip_metadata={'SourceID': EMBEDDED_MOB})


def build_embed_aaf_media_ref_mob_id():
    """`test_transcribe_embed_aaf_media_ref_mob_id`, with the MobID on the
    media, using one frame of the two, so that the copied master mob's slot is
    marked in and out, and with edge code, which goes on the copied mob."""
    return embedding(EMBEDDED_AAF, tr(0, 2), tr(1, 1),
                     media_metadata={'SourceID': EMBEDDED_MOB})


# What upstream raises embedding media it cannot: each case is one clip,
# written with `use_empty_mob_ids` so that it gets as far as the embedding.
# A path is named relative to the crate, as the written fixtures name theirs.
EMBED_ERRORS = [
    # No file there at all.
    ('missing', 'Video', 'missing.dnx', ''),
    # A file of a kind that is neither AAF, DNxHD nor WAV.
    ('unsupported', 'Video', 'Cargo.toml', ''),
    # An AAF without the master mob the clip names.
    ('no_master_mob', 'Video', EMBEDDED_AAF,
     umid('00000000.0000.0000.0000.000000000000')),
    # The audio transcriber has no import of its own, so upstream fails on
    # the result of the one it inherits, which returns nothing.
    ('wav_on_audio', 'Audio', WAV, ''),
    ('dnx_on_audio', 'Audio', DNX, ''),
    # A WAV on a video track goes to the DNxHD import, which refuses it.
    ('wav_on_video', 'Video', WAV, ''),
]


def write_embed_errors(out_dir):
    """Runs each of `EMBED_ERRORS` through the adapter and records what it
    raised, in `embed_errors.tsv`."""
    rows = []
    for case, kind, url, mob_id in EMBED_ERRORS:
        timeline = embedding(url, tr(0, 2), tr(0, 2), kind=kind,
                             clip_metadata={'SourceID': mob_id} if mob_id else None)
        path = os.path.join(out_dir, 'embed_error.aaf')
        try:
            adapter.write_to_file(timeline, path, embed_essence=True,
                                  use_empty_mob_ids=True)
        except Exception as e:
            rows.append((case, kind, url, mob_id, type(e).__name__, str(e)))
        else:
            sys.exit('%s: upstream wrote it' % case)
        finally:
            if os.path.exists(path):
                os.remove(path)
    with open(os.path.join(out_dir, 'embed_errors.tsv'), 'w', newline='\n',
              encoding='utf-8') as out:
        out.write('# What upstream raises embedding one clip on a track of the kind '
                  'given, with its MobID if given: case, kind, target_url, MobID, '
                  'exception, message.\n')
        for row in rows:
            assert not any('\t' in field or '\n' in field for field in row)
            out.write('\t'.join(row) + '\n')
    print('embed_errors', len(rows), 'cases')


# The vendored samples written back, each chosen for a part of the writer:
# clip colours, an essence group's clip, markers either side of a transition,
# clips under speed effects, a dissolve in sound, nesting, and a file from
# pyaaf2's own tests with user comments and sound. `2997fps-DFTC` and `empty`
# are left out because the adapter refuses them: the Rust tests check it does
# too.
SAMPLES = [
    'colored_clips',
    'essence_group',
    'marker-over-transition',
    'misc_speed_effects',
    'nested_audio_dissolve',
    'nesting_test',
    'sector_size_512',
]

BUILT = [
    ('edit', build_edit, {}),
    ('options', build_options,
     dict(prefer_file_mob_id=True, use_empty_mob_ids=True, create_edgecode=True)),
    ('embed_dnx', build_embed_dnx, dict(embed_essence=True, use_empty_mob_ids=True)),
    ('embed_aaf_clip_mob_id', build_embed_aaf_clip_mob_id, dict(embed_essence=True)),
    ('embed_aaf_media_ref_mob_id', build_embed_aaf_media_ref_mob_id,
     dict(embed_essence=True, create_edgecode=True)),
]


def main():
    os.chdir(CRATE)
    if gen_otio.ALL:
        out = gen_otio.ALL
        os.makedirs(out, exist_ok=True)
        pattern = os.path.join(gen_otio.ADAPTER, 'tests', 'sample_data', '*.aaf')
        for path in sorted(glob.glob(pattern)):
            name = os.path.basename(path)[:-len('.aaf')]
            timeline, text = from_sample(path)
            try:
                write(name, timeline, out)
            except Exception as e:  # the adapter refuses some samples
                print(name, 'not written:', type(e).__name__, e)
                for leftover in glob.glob(os.path.join(out, name + '.*')):
                    os.remove(leftover)
                continue
            with open(os.path.join(out, name + '.otio.json'), 'w', newline='\n',
                      encoding='utf-8') as f:
                f.write(text)
        return

    os.makedirs(OUT, exist_ok=True)
    for name in SAMPLES:
        path = os.path.join(DATA, name + '.aaf')
        if not os.path.exists(path):
            path = os.path.join(AAF_CRATE, name + '.aaf')
        timeline, _ = from_sample(path, os.path.join(DATA, name + '.otio.json'))
        write(name, timeline, OUT)

    for name, build, options in BUILT:
        text = otio.adapters.write_to_string(build(), 'otio_json')
        if not text.endswith('\n'):
            text += '\n'
        with open(os.path.join(OUT, name + '.otio.json'), 'w', newline='\n',
                  encoding='utf-8') as f:
            f.write(text)
        write(name, otio.adapters.read_from_string(text, 'otio_json'), OUT, **options)

    write_embed_errors(OUT)


if __name__ == '__main__':
    main()
