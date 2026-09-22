//! The write path, checked byte for byte against files upstream pyaaf2 wrote.
//!
//! `tests/data/generators/gen_written.py` builds three files with pyaaf2 and
//! records, beside each, every time and identifier pyaaf2 asked for while it
//! did. Each test here builds the same content through [`AafWriter`], feeding
//! it those same values in the same order, and asserts the two files are
//! identical. A mismatch is reported by the first byte that differs and the
//! part of the compound file it falls in, which is usually enough to say
//! which object or stream went wrong.
//!
//! The replaying and the comparing live in `written/mod.rs`, which the
//! OpenTimelineIO adapter's tests share.
//!
//! Every file is then read back through this crate's own reader.

mod common;
mod written;

use std::io::Cursor;

use aaf::cfb::CompoundFile;
use aaf::write::{
    AafWriter, Rational, SequentialIds, SteppingClock, Timestamp, WriteOptions, WriteValue,
};
use aaf::{Aaf, AafFile, Auid, MobId};

use written::{Replay, Sidecar, assert_identical};

/// A writer set up as pyaaf2 was for fixture `name`, the replay feeding it,
/// and the file pyaaf2 wrote.
fn writer_for(name: &'static str) -> (AafWriter, Replay, Vec<u8>) {
    let dir = common::data_dir();
    let expected = std::fs::read(dir.join(format!("{name}.aaf"))).expect("fixture exists");
    let sidecar = Sidecar::read(name, &dir.join(format!("{name}.calls.tsv")));
    let replay = sidecar.replay;
    let writer = AafWriter::with_options(WriteOptions {
        sector_size: sidecar.sector_size,
        // The generator pins Python's sys.platform, which is what pyaaf2
        // records, so the fixtures are the same wherever they were made.
        platform: "linux".to_owned(),
        extensions: true,
        clock: Box::new(replay.clone()),
        ids: Box::new(replay.clone()),
    })
    .expect("a new file");
    (writer, replay, expected)
}

/// Reads a written file back through both of this crate's readers.
type InMemory = Cursor<Vec<u8>>;

fn read_back(bytes: &[u8]) -> (AafFile<InMemory>, Aaf<InMemory>) {
    let file = AafFile::open(Cursor::new(bytes.to_vec())).expect("the written file opens");
    let aaf = Aaf::open(Cursor::new(bytes.to_vec())).expect("the written file opens by name");
    (file, aaf)
}

fn check(name: &'static str, build: impl FnOnce(&mut AafWriter)) -> Vec<u8> {
    let (mut w, replay, expected) = writer_for(name);
    build(&mut w);
    let ours = w.finish().expect("the file finishes");
    replay.assert_used_up();
    assert_identical(name, &ours, &expected);
    ours
}

// --- (a) a new file with nothing added ------------------------------------------

#[test]
fn an_empty_file_matches_pyaaf2() {
    let bytes = check("written_empty", |_| {});
    let (mut file, mut aaf) = read_back(&bytes);
    let root = file.root().unwrap();
    assert_eq!(file.children(&root).unwrap().len(), 2);
    assert!(aaf.mobs().unwrap().is_empty());
}

// --- (b) a composition: clips, a filler and a dissolve --------------------------

const CLIP_MOB: &str =
    "urn:smpte:umid:060a2b34.01010105.01010f20.13000000.11111111.2222.3333.4444.555555555555";
const DISSOLVE: &str = "0c3bea40-fc05-11d2-8a29-0050040ef7d2";

fn build_sequence(w: &mut AafWriter) -> aaf::Result<()> {
    let clip_mob: MobId = CLIP_MOB.parse().unwrap();

    let comp = w.create("CompositionMob")?;
    w.set(comp, "Name", "Sequence Test")?;
    w.set(comp, "UsageCode", "Usage_TopLevel")?;
    w.add_mob(comp)?;

    let slot = w.create_timeline_slot(comp, 24, None)?;
    w.set(slot, "SlotName", "V1")?;
    w.set(slot, "PhysicalTrackNumber", 1)?;

    let sequence = w.create_sequence("picture")?;
    w.set(sequence, "Components", Vec::new())?;
    w.set(slot, "Segment", sequence)?;

    let clip = w.create_source_clip(10, 48, clip_mob, 1, "picture")?;
    w.append(sequence, "Components", clip)?;
    let filler = w.create_filler("picture", 24)?;
    w.append(sequence, "Components", filler)?;

    let opdef = w.create_definition(
        "OperationDef",
        Some(DISSOLVE.parse().unwrap()),
        Some("VideoDissolve"),
        Some("Video dissolve"),
    )?;
    w.register_def(opdef)?;
    w.set_media_kind(opdef, "picture")?;
    w.set(opdef, "IsTimeWarp", false)?;
    w.set(opdef, "NumberInputs", 2)?;
    w.set(opdef, "OperationCategory", "OperationCategory_Effect")?;
    w.set(opdef, "Bypass", 1)?;

    let opgroup = w.create_operation_group(opdef, 12, None)?;
    let transition = w.create_transition("picture", 12)?;
    w.set(transition, "OperationGroup", opgroup)?;
    w.set(transition, "CutPoint", 6)?;
    w.append(sequence, "Components", transition)?;

    let clip = w.create_source_clip(0, 36, clip_mob, 1, "picture")?;
    w.append(sequence, "Components", clip)?;
    w.set(sequence, "Length", 48 + 24 - 12 + 36)?;
    Ok(())
}

#[test]
fn a_sequence_matches_pyaaf2() {
    let bytes = check("written_sequence", |w| build_sequence(w).unwrap());
    let (_, mut aaf) = read_back(&bytes);
    let mobs = aaf.top_level_mobs().unwrap();
    assert_eq!(mobs.len(), 1);
    assert_eq!(
        aaf.name(&mobs[0]).unwrap().as_deref(),
        Some("Sequence Test")
    );
    let slots = aaf.slots(&mobs[0]).unwrap();
    let sequence = aaf.child(&slots[0], "Segment").unwrap().unwrap();
    assert_eq!(aaf.class_name(&sequence), Some("Sequence"));
}

// --- (c) the source chain, as the OpenTimelineIO adapter writes one -------------

const PAN_PARAMETER: &str = "e4962322-2267-11d3-8a4c-0050040ef7d2";
const MONO_AUDIO_PAN: &str = "9d2ea893-0968-11d3-8a38-0050040ef7d2";
const LEVEL_PARAMETER: &str = "e4962320-2267-11d3-8a4c-0050040ef7d2";
const EXTRAPOLATION: &str = "0e24dd54-66cd-4f1a-b0a0-670ac3a7a0b3";
const LINEAR_INTERP: &str = "5b6c85a4-0ede-11d3-80a9-006008143e6f";

fn id(text: &str) -> Auid {
    text.parse().unwrap()
}

fn segment(w: &AafWriter, slot: aaf::write::ObjRef) -> aaf::write::ObjRef {
    w.get_object(slot, "Segment").unwrap().unwrap()
}

fn slot_id(w: &AafWriter, slot: aaf::write::ObjRef) -> u32 {
    u32::try_from(w.get_int(slot, "SlotID").unwrap().unwrap()).unwrap()
}

#[allow(clippy::too_many_lines)]
fn build_mobs(w: &mut AafWriter) -> aaf::Result<()> {
    // The adapter registers Avid's extended marker colour first.
    w.register_propertydef(
        "CommentMarker",
        "CommentMarkerColorExtended",
        id("e96e6d45-c383-11d3-a069-006094eb75cb"),
        Some(0xffda),
        id("e96e6d43-c383-11d3-a069-006094eb75cb"),
        false,
        false,
    )?;

    let comp = w.create("CompositionMob")?;
    w.set(comp, "Name", "Mobs Test")?;
    w.set(comp, "UsageCode", "Usage_TopLevel")?;
    w.add_mob(comp)?;
    w.set_tagged_value(comp, "UserComments", "Project", "Writing test")?;
    w.set_tagged_value(comp, "MobAttributeList", "_IMPORTSETTING", 1)?;

    // A tape.
    let tape = w.create("SourceMob")?;
    w.set(tape, "Name", "A001C003")?;
    let import = w.create("ImportDescriptor")?;
    w.set(tape, "EssenceDescription", import)?;
    let (_, tc_slot) = w.create_tape_slots(tape, "A001C003", 24, 24, false, None, None)?;
    let tc = segment(w, tc_slot);
    w.set(tc, "Start", 86400)?;
    w.set(tc, "Length", 240)?;
    w.add_mob(tape)?;
    let locator = w.create("NetworkLocator")?;
    w.set(locator, "URLString", "file:///media/A001C003.mov")?;
    let tape_descriptor = w.get_object(tape, "EssenceDescription")?.unwrap();
    w.append(tape_descriptor, "Locator", locator)?;

    let tape_clip_slot = w.create_empty_slot(tape, 24, Some("picture"), None)?;
    let tape_clip = segment(w, tape_clip_slot);
    w.set(tape_clip, "Length", 240)?;
    w.set(tape_clip, "StartTime", 86400)?;

    // A file.
    let filemob = w.create("SourceMob")?;
    w.add_mob(filemob)?;
    let descriptor = w.create("CDCIDescriptor")?;
    w.set(descriptor, "ComponentWidth", 8)?;
    w.set(descriptor, "HorizontalSubsampling", 2)?;
    w.set(descriptor, "ImageAspectRatio", "16/9")?;
    w.set(descriptor, "StoredWidth", 1920)?;
    w.set(descriptor, "StoredHeight", 1080)?;
    w.set(descriptor, "FrameLayout", "FullFrame")?;
    w.set(descriptor, "VideoLineMap", WriteValue::array([42, 0]))?;
    w.set(descriptor, "SampleRate", "24")?;
    w.set(descriptor, "Length", 240)?;
    let locator = w.create("NetworkLocator")?;
    w.set(locator, "URLString", "file:///media/A001C003.mov")?;
    w.append(descriptor, "Locator", locator)?;
    w.set(filemob, "EssenceDescription", descriptor)?;
    let file_slot = w.create_timeline_slot(filemob, 24, None)?;
    let file_clip = w.create_mob_source_clip(
        filemob,
        slot_id(w, file_slot),
        None,
        Some(240),
        Some("picture"),
    )?;
    w.set_source_mob(file_clip, tape)?;
    w.set_source_slot(file_clip, tape_clip_slot)?;
    w.set(file_clip, "SourceMobSlotID", slot_id(w, tape_clip_slot))?;
    w.set(file_slot, "Segment", file_clip)?;

    // A sound file.
    let soundmob = w.create("SourceMob")?;
    w.add_mob(soundmob)?;
    let pcm = w.create("PCMDescriptor")?;
    w.set(pcm, "AverageBPS", 96000)?;
    w.set(pcm, "BlockAlign", 2)?;
    w.set(pcm, "QuantizationBits", 16)?;
    w.set(pcm, "AudioSamplingRate", 48000)?;
    w.set(pcm, "Channels", 1)?;
    w.set(pcm, "SampleRate", 48000)?;
    w.set(pcm, "Length", 480_000)?;
    w.set(soundmob, "EssenceDescription", pcm)?;
    let sound_slot = w.create_timeline_slot(soundmob, 24, None)?;
    let sound_file_clip = w.create_source_clip(0, 240, MobId::NIL, 0, "sound")?;
    w.set(sound_slot, "Segment", sound_file_clip)?;

    // The master mob.
    let master = w.create("MasterMob")?;
    w.set(master, "Name", "A001C003")?;
    let master_id: MobId =
        "urn:smpte:umid:060a2b34.01010105.01010f20.13000000.aaaaaaaa.bbbb.cccc.dddd.eeeeeeeeeeee"
            .parse()
            .unwrap();
    w.set(master, "MobID", master_id)?;
    w.add_mob(master)?;
    w.set_tagged_value(master, "UserComments", "Scene", "12A")?;
    w.set_tagged_value(master, "UserComments", "Take", 3)?;
    w.set_tagged_value(master, "UserComments", "Speed", Rational::new(24000, 1001))?;
    let master_slot = w.create_timeline_slot(master, 24, Some(1))?;
    let master_clip = w.create_mob_source_clip(
        master,
        slot_id(w, master_slot),
        None,
        Some(240),
        Some("picture"),
    )?;
    w.set_source_mob(master_clip, filemob)?;
    w.set_source_slot(master_clip, file_slot)?;
    w.set(master_clip, "SourceMobSlotID", slot_id(w, file_slot))?;
    w.set(master_slot, "Segment", master_clip)?;
    w.set(master_slot, "MarkIn", 10)?;
    w.set(master_slot, "MarkOut", 58)?;

    // The composition's timecode track.
    let tc = w.create_timeline_slot(comp, 24, None)?;
    w.set(tc, "SlotName", "TC")?;
    w.set(tc, "PhysicalTrackNumber", 1)?;
    let timecode = w.create("Timecode")?;
    w.set(timecode, "FPS", 24)?;
    w.set(timecode, "Drop", false)?;
    w.set(timecode, "Start", 86400)?;
    w.set(tc, "Segment", timecode)?;

    // A picture track.
    let video = w.create_timeline_slot(comp, 24, None)?;
    let sequence = w.create_sequence("picture")?;
    w.set(sequence, "Components", Vec::new())?;
    w.set(video, "Segment", sequence)?;
    w.set(video, "SlotName", "V1")?;
    w.set(video, "PhysicalTrackNumber", 1)?;
    let clip =
        w.create_mob_source_clip(comp, slot_id(w, video), Some(10), Some(48), Some("picture"))?;
    w.set_source_mob(clip, master)?;
    w.set_source_slot(clip, master_slot)?;
    w.set(clip, "SourceMobSlotID", slot_id(w, master_slot))?;
    w.set_tagged_value(clip, "ComponentAttributeList", "_COLOR_R", 65535)?;
    w.append(sequence, "Components", clip)?;
    w.set(sequence, "Length", 48)?;

    // A sound track: a mono pan around a sequence, with keyframes.
    let audio = w.create_sound_slot(comp, 24)?;
    let pan = w.create_definition(
        "OperationDef",
        Some(id(MONO_AUDIO_PAN)),
        Some("Audio Pan"),
        None,
    )?;
    w.set_media_kind(pan, "sound")?;
    w.set(pan, "NumberInputs", 1)?;
    w.register_def(pan)?;
    let opgroup = w.create_operation_group(pan, 0, None)?;
    w.set_media_kind(opgroup, "sound")?;
    w.set(opgroup, "Length", 48)?;
    w.set(audio, "Segment", opgroup)?;
    w.set(audio, "SlotName", "A1")?;
    w.set(audio, "PhysicalTrackNumber", 1)?;
    let sound_sequence = w.create_sequence("sound")?;
    w.set(sound_sequence, "Components", Vec::new())?;
    w.set(sound_sequence, "Length", 48)?;
    w.append(opgroup, "InputSegments", sound_sequence)?;

    let rational = w.lookup_typedef("Rational").unwrap();
    let param_def = w.create_parameter_def(id(PAN_PARAMETER), "Pan", "Pan", Some(rational))?;
    w.register_def(param_def)?;
    let interp = w.create_definition(
        "InterpolationDef",
        Some(id(LINEAR_INTERP)),
        Some("LinearInterp"),
        Some("LinearInterp"),
    )?;
    w.register_def(interp)?;
    let varying = w.create("VaryingValue")?;
    w.set_parameter_def(varying, param_def)?;
    w.set(varying, "Interpolation", interp)?;
    w.set(varying, "VVal_Extrapolation", id(EXTRAPOLATION))?;
    w.set(varying, "VVal_FieldCount", 1)?;
    for (time, value) in [("0/48", "1/2"), ("47/48", "1/2")] {
        let point = w.create("ControlPoint")?;
        w.set(point, "Time", Rational::parse(time).unwrap())?;
        w.set(point, "Value", Rational::parse(value).unwrap())?;
        w.set(point, "ControlPointSource", 2)?;
        w.set(point, "EditHint", "Proportional")?;
        w.append(varying, "PointList", point)?;
    }
    w.append(opgroup, "Parameters", varying)?;

    let level = w.create_parameter_def(
        id(LEVEL_PARAMETER),
        "ParameterDef_Level",
        "",
        Some(rational),
    )?;
    w.register_def(level)?;
    w.extend(pan, "ParametersDefined", &[param_def, level])?;
    let constant = w.create_constant_value(level, Some(Rational::new(1, 2).into()))?;
    w.append(opgroup, "Parameters", constant)?;

    let sound_clip =
        w.create_mob_source_clip(comp, slot_id(w, audio), None, Some(48), Some("sound"))?;
    w.set_source_mob(sound_clip, soundmob)?;
    w.set_source_slot(sound_clip, sound_slot)?;
    w.set(sound_clip, "SourceMobSlotID", slot_id(w, sound_slot))?;
    w.append(sound_sequence, "Components", sound_clip)?;

    // A marker, in an event slot of its own.
    let events = w.create("EventMobSlot")?;
    w.set(events, "EditRate", 24)?;
    w.set(events, "SlotID", 1000)?;
    w.set(events, "PhysicalTrackNumber", 1)?;
    let marker_sequence = w.create_sequence("DescriptiveMetadata")?;
    let marker = w.create("DescriptiveMarker")?;
    w.set(marker, "Length", 1)?;
    w.set(
        marker,
        "DescribedSlots",
        WriteValue::array([slot_id(w, video)]),
    )?;
    w.set(marker, "Position", 12)?;
    w.set(marker, "Comment", "Check focus")?;
    w.set(marker, "CommentMarkerUser", "editor")?;
    let colour = WriteValue::record([("red", 41471), ("green", 12134), ("blue", 6564)]);
    w.set(marker, "CommentMarkerColor", colour.clone())?;
    w.set(marker, "CommentMarkerColorExtended", colour)?;
    w.set(marker, "CommentMarkerTime", "07:08")?;
    w.set(marker, "CommentMarkerDate", "05/06/2024")?;
    w.set_tagged_value(
        marker,
        "CommentMarkerAttributeList",
        "_ATN_CRM_COM",
        "Check focus",
    )?;
    w.set_tagged_value(
        marker,
        "CommentMarkerAttributeList",
        "_ATN_CRM_LONG_CREATE_DATE",
        1_714_979_289,
    )?;
    w.set_tagged_value(marker, "UserComments", "Comment", "Check focus")?;
    w.append(marker_sequence, "Components", marker)?;
    w.set(events, "Segment", marker_sequence)?;
    w.append(comp, "Slots", events)?;
    Ok(())
}

#[test]
fn a_source_chain_matches_pyaaf2() {
    let bytes = check("written_mobs", |w| build_mobs(w).unwrap());
    let (_, mut aaf) = read_back(&bytes);
    let mobs = aaf.mobs().unwrap();
    assert_eq!(mobs.len(), 5);
    let top = aaf.top_level_mobs().unwrap();
    assert_eq!(top.len(), 1);
    let slots = aaf.slots(&top[0]).unwrap();
    assert_eq!(slots.len(), 4);
}

// --- the deterministic sources, and the defaults ----------------------------------

fn deterministic(sector_size: u32) -> AafWriter {
    AafWriter::with_options(WriteOptions {
        sector_size,
        platform: "linux".to_owned(),
        extensions: true,
        clock: Box::new(SteppingClock::new(
            Timestamp::parse_iso("2024-01-02T03:04:05").unwrap(),
        )),
        ids: Box::new(SequentialIds::new(0x5eed_0000)),
    })
    .unwrap()
}

#[test]
fn deterministic_sources_write_the_same_bytes_every_time() {
    for sector_size in [512, 4096] {
        let build = || {
            let mut w = deterministic(sector_size);
            build_mobs(&mut w).unwrap();
            w.finish().unwrap()
        };
        let first = build();
        assert_identical("a second deterministic build", &build(), &first);

        let file = CompoundFile::open(Cursor::new(first.clone())).unwrap();
        assert!(file.warnings().is_empty(), "{:?}", file.warnings());
        let (_, mut aaf) = read_back(&first);
        assert_eq!(aaf.mobs().unwrap().len(), 5);
    }
}

#[test]
fn the_default_options_write_a_file_that_reads_back() {
    let mut w = AafWriter::new().unwrap();
    build_sequence(&mut w).unwrap();
    let bytes = w.finish().unwrap();
    let file = CompoundFile::open(Cursor::new(bytes.clone())).unwrap();
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let (_, mut aaf) = read_back(&bytes);
    let mobs = aaf.top_level_mobs().unwrap();
    assert_eq!(
        aaf.name(&mobs[0]).unwrap().as_deref(),
        Some("Sequence Test")
    );
}
