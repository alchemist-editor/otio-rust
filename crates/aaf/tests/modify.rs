//! Changing existing files, checked byte for byte against upstream pyaaf2.
//!
//! `tests/data/generators/gen_modified.py` copies fixture files, opens each
//! copy with pyaaf2 the way pyaaf2 opens a file to change it, makes a
//! scripted set of edits, and saves it. Beside each result it records, in a
//! `<name>.calls.tsv` sidecar, the file it started from and every time and
//! identifier pyaaf2 asked for. Each test here opens the same starting file
//! through this crate, makes the same edits in the same order, feeds it the
//! same values, and asserts the result is identical to pyaaf2's.
//!
//! pyaaf2 changes a file in place, so its result is mostly the file it
//! started from. The generator keeps only the difference, as
//! `modified/<name>.patch`: every 512-byte block of pyaaf2's result that
//! differs from the starting file, and the result's length. Applying it to
//! the starting file gives back pyaaf2's bytes exactly.

mod common;
mod written;

use aaf::cfb::{CompoundFileWriter, DirId, ROOT_ID};

use written::{Replay, assert_identical, read_sidecar};

// --- the fixtures ---------------------------------------------------------------

/// A scenario: the file it starts from, pyaaf2's result, and the values
/// pyaaf2 asked for on the way.
struct Scenario {
    base: Vec<u8>,
    expected: Vec<u8>,
    replay: Replay,
    extensions: bool,
}

/// Reads scenario `name`: its sidecar, the file it names as its base, and
/// the patch that turns that base into pyaaf2's result.
fn scenario(name: &str) -> Scenario {
    let dir = common::data_dir();
    let sidecar_path = dir.join(format!("modified/{name}.calls.tsv"));
    let text = std::fs::read_to_string(&sidecar_path).expect("the sidecar is readable");
    let base_name = text
        .lines()
        .find_map(|line| line.strip_prefix("base\t"))
        .expect("the sidecar names its base")
        .to_owned();
    // The replay reads everything but the `base` line, which is this test's.
    let rest: String = text
        .lines()
        .filter(|line| !line.starts_with("base\t"))
        .map(|line| format!("{line}\n"))
        .collect();
    let scratch =
        std::env::temp_dir().join(format!("aaf-modify-{name}-{}.tsv", std::process::id()));
    std::fs::write(&scratch, rest).expect("the scratch sidecar is written");
    let sidecar = read_sidecar(name, &scratch);
    let _ = std::fs::remove_file(&scratch);

    // A base of `@name` is what scenario `name` left.
    let base = match base_name.strip_prefix('@') {
        Some(earlier) => scenario(earlier).expected,
        None => std::fs::read(dir.join(&base_name)).expect("the base fixture exists"),
    };
    let patch =
        std::fs::read(dir.join(format!("modified/{name}.patch"))).expect("the patch exists");
    let expected = apply_patch(&base, &patch);
    Scenario {
        base,
        expected,
        replay: sidecar.replay,
        // pyaaf2 registers its extensions unless told not to.
        extensions: !sidecar
            .options
            .iter()
            .any(|(option, on)| option == "extensions" && !on),
    }
}

/// Applies a patch the generator made: `AAFPATCH`, the result's length, the
/// block size and the number of blocks, then each block's index and bytes.
fn apply_patch(base: &[u8], patch: &[u8]) -> Vec<u8> {
    assert_eq!(&patch[..8], b"AAFPATCH", "a patch starts with its magic");
    let u32_at = |at: usize| u32::from_le_bytes(patch[at..at + 4].try_into().unwrap()) as usize;
    let len = u64::from_le_bytes(patch[8..16].try_into().unwrap()) as usize;
    let block = u32_at(16);
    let count = u32_at(20);
    let mut out = base.to_vec();
    out.resize(len.max(base.len()), 0);
    let mut at = 24;
    for _ in 0..count {
        let index = u32_at(at);
        let start = index * block;
        let end = start + block;
        if out.len() < end {
            out.resize(end, 0);
        }
        out[start..end].copy_from_slice(&patch[at + 4..at + 4 + block]);
        at += 4 + block;
    }
    assert_eq!(at, patch.len(), "the patch holds exactly its blocks");
    out.truncate(len);
    out
}

// --- the container alone --------------------------------------------------------

/// pyaaf2's `pattern`: bytes that differ from one position to the next.
fn pattern(n: usize, seed: usize) -> Vec<u8> {
    (0..n).map(|i| ((i * 31 + seed) % 251) as u8).collect()
}

fn find(c: &CompoundFileWriter, path: &str) -> DirId {
    c.find(path)
        .unwrap_or_else(|| panic!("{path} is in the file"))
}

fn check_cfb(name: &str, edit: impl FnOnce(&mut CompoundFileWriter)) {
    let s = scenario(name);
    let mut c = CompoundFileWriter::open(s.base).expect("the base opens for writing");
    edit(&mut c);
    let ours = c.finish().expect("the file finishes");
    s.replay.assert_used_up();
    assert_identical(name, &ours, &s.expected);
}

/// The generator's `cfb_edits`.
fn cfb_edits(c: &mut CompoundFileWriter) {
    // Grow a mini stream past the cutoff, so it moves into the FAT.
    let id = find(c, "/Header-2/properties");
    let mut data = c.read_stream(id).unwrap();
    data.extend(pattern(5000, 1));
    c.write_stream(id, &data).unwrap();
    // Shrink another mini stream, freeing mini sectors.
    let id = find(c, "/MetaDictionary-1/properties");
    c.write_stream(id, &pattern(10, 2)).unwrap();
    // A new storage with a stream written a piece at a time.
    let added = c.create_storage(ROOT_ID, "Added", None).unwrap();
    let data = c.touch(added, "data").unwrap();
    for i in 0..9 {
        c.append_stream(data, &pattern(700, i)).unwrap();
    }
    // Move a stream into it, then take a storage out and a stream.
    let index = find(c, "/Header-2/Content-3b03/Mobs-1901 index");
    c.move_entry(index, added, "moved").unwrap();
    let defs = find(c, "/Header-2/Dictionary-3b04/DataDefinitions-2605{0}");
    c.rmtree(defs).unwrap();
    let moved = find(c, "/Added/moved");
    c.remove(moved).unwrap();
    // And allocate again, into what was freed.
    let again = c.touch(added, "again").unwrap();
    c.append_stream(again, &pattern(300, 7)).unwrap();
    let big = c.touch(added, "big").unwrap();
    c.append_stream(big, &pattern(9000, 8)).unwrap();
}

#[test]
fn opening_and_closing_a_container_matches_pyaaf2() {
    check_cfb("cfb_noop_4096", |_| {});
}

#[test]
fn opening_and_closing_a_512_byte_sector_container_matches_pyaaf2() {
    check_cfb("cfb_noop_512", |_| {});
}

#[test]
fn growing_shrinking_moving_and_removing_in_a_512_byte_sector_container_matches_pyaaf2() {
    check_cfb("cfb_edits_512", cfb_edits);
}

#[test]
fn growing_shrinking_moving_and_removing_in_a_container_matches_pyaaf2() {
    check_cfb("cfb_edits_4096", cfb_edits);
}

// --- files ------------------------------------------------------------------------

use aaf::Auid;
use aaf::write::{AafWriter, ObjRef, OpenOptions, Rational};

/// Opens scenario `name`'s base as pyaaf2's `aaf2.open(path, 'r+')` does,
/// makes the edits, saves, and compares with what pyaaf2 saved.
fn check(name: &str, edit: impl FnOnce(&mut AafWriter) -> aaf::Result<()>) -> Vec<u8> {
    let s = scenario(name);
    let mut w = AafWriter::open_bytes(
        s.base,
        OpenOptions {
            extensions: s.extensions,
            clock: Box::new(s.replay.clone()),
            ids: Box::new(s.replay.clone()),
        },
    )
    .expect("the base opens for changing");
    edit(&mut w).expect("the edits succeed");
    let ours = w.finish().expect("the file saves");
    s.replay.assert_used_up();
    assert_identical(name, &ours, &s.expected);
    ours
}

fn noop(_: &mut AafWriter) -> aaf::Result<()> {
    Ok(())
}

fn id(text: &str) -> Auid {
    text.parse().expect("a valid AUID")
}

fn segment(w: &AafWriter, slot: ObjRef) -> ObjRef {
    w.get_object(slot, "Segment")
        .unwrap()
        .expect("the slot has a segment")
}

#[test]
fn opening_and_saving_a_file_pyaaf2_wrote_matches_pyaaf2() {
    check("noop_written", noop);
}

#[test]
fn opening_and_saving_a_file_the_aaf_sdk_wrote_matches_pyaaf2() {
    check("noop_empty", noop);
}

/// The generator's `edit_properties`.
#[test]
fn changing_adding_and_deleting_properties_matches_pyaaf2() {
    check("edit_properties", |w| {
        let mobs = w.mobs()?;
        let (comp, master) = (mobs[0], mobs[4]);
        w.set(comp, "Name", "Mobs Test, renamed")?;
        w.set(comp, "AppCode", 7)?;
        let slots = w.get_objects(comp, "Slots")?;
        w.remove(slots[0], "SlotName")?;
        w.set(slots[1], "PhysicalTrackNumber", 9)?;
        let sequence = segment(w, slots[1]);
        let clip = w.get_objects(sequence, "Components")?[0];
        w.set(clip, "StartTime", 12)?;
        w.set(clip, "Length", 48)?;
        w.set(sequence, "Length", 48)?;
        w.set_tagged_value(master, "UserComments", "Scene", "14B")?;
        w.set_tagged_value(master, "UserComments", "Camera", "B")?;
        let marker = w.get_objects(segment(w, slots[3]), "Components")?[0];
        w.set(marker, "Comment", "Changed")?;
        Ok(())
    });
}

/// The generator's `add_mob`.
#[test]
fn adding_a_mob_with_slots_and_clips_matches_pyaaf2() {
    check("add_mob", |w| {
        let master = w.mobs()?[4];
        let comp = w.create_mob("CompositionMob", Some("Added"))?;
        w.set(comp, "UsageCode", "Usage_TopLevel")?;
        w.add_mob(comp)?;
        let slot = w.create_timeline_slot(comp, 24, None)?;
        w.set(slot, "SlotName", "V1")?;
        let sequence = w.create_sequence("picture")?;
        w.set(sequence, "Components", Vec::new())?;
        w.set(slot, "Segment", sequence)?;
        let clip = w.create_mob_source_clip(master, 1, Some(0), Some(24), None)?;
        w.append(sequence, "Components", clip)?;
        let filler = w.create_filler("picture", 12)?;
        w.append(sequence, "Components", filler)?;
        w.set(sequence, "Length", 36)?;
        let timecode_slot = w.create_timeline_slot(comp, 24, None)?;
        let timecode = w.create_timecode(24, false, Some(36))?;
        w.set(timecode_slot, "Segment", timecode)?;
        w.set_tagged_value(comp, "UserComments", "Added", "yes")?;
        Ok(())
    });
}

/// The generator's `remove_mob`.
#[test]
fn removing_a_mob_a_slot_and_a_component_matches_pyaaf2() {
    check("remove_mob", |w| {
        let mobs = w.mobs()?;
        let (comp, source) = (mobs[0], mobs[2]);
        w.remove_mob(source)?;
        w.pop(comp, "Slots", 0)?;
        let events = segment(w, w.get_objects(comp, "Slots")?[2]);
        w.pop(events, "Components", 0)?;
        w.set(events, "Length", 0)?;
        Ok(())
    });
}

/// The generator's `mob_id_swap`, after pyaaf2's `test_mob_id_swap`.
#[test]
fn giving_a_mob_a_new_mob_id_matches_pyaaf2() {
    check("mob_id_swap", |w| {
        let comp = w.mobs()?[0];
        let mob_id = w.new_mob_id();
        w.set(comp, "MobID", mob_id)
    });
}

/// The generator's `definitions`.
#[test]
fn adding_definitions_and_a_property_to_a_class_in_the_file_matches_pyaaf2() {
    check("definitions", |w| {
        let comp = w.mobs()?[0];
        w.register_propertydef(
            "Mob",
            "ReelTag",
            id("6e2b1e36-0b43-4bd1-9c5a-8f3c2a1d0e03"),
            None,
            id("01100200-0000-0000-060e-2b3401040101"),
            true,
            false,
        )?;
        w.set(comp, "ReelTag", "Reel 7")?;

        let opdef = w.create_definition(
            "OperationDef",
            Some(id("6e2b1e36-0b43-4bd1-9c5a-8f3c2a1d0e01")),
            Some("Blur"),
            Some("A blur"),
        )?;
        w.register_def(opdef)?;
        w.set_media_kind(opdef, "picture")?;
        w.set(opdef, "IsTimeWarp", false)?;
        w.set(opdef, "NumberInputs", 1)?;
        let rational = w.lookup_typedef("Rational").expect("Rational is defined");
        let amount = w.create_parameter_def(
            id("6e2b1e36-0b43-4bd1-9c5a-8f3c2a1d0e02"),
            "Amount",
            "How much",
            Some(rational),
        )?;
        w.register_def(amount)?;
        w.append(opdef, "ParametersDefined", amount)?;
        let interp = w.create_definition(
            "InterpolationDef",
            Some(id("5b6c85a4-0ede-11d3-80a9-006008143e6f")),
            Some("LinearInterp"),
            Some("LinearInterp"),
        )?;
        w.register_def(interp)?;

        let slot = w.create_timeline_slot(comp, 24, None)?;
        let opgroup = w.create_operation_group(opdef, 24, None)?;
        w.set(slot, "Segment", opgroup)?;
        let filler = w.create_filler("picture", 24)?;
        w.append(opgroup, "InputSegments", filler)?;
        let constant = w.create_constant_value(amount, Some(Rational::new(3, 4).into()))?;
        w.append(opgroup, "Parameters", constant)?;
        Ok(())
    });
}

/// The generator's `new_class`, after pyaaf2's `test_register`.
#[test]
fn defining_a_class_and_making_an_object_of_it_matches_pyaaf2() {
    check("new_class", |w| {
        w.register_classdef(
            "ShotNotes",
            id("6e2b1e36-0b43-4bd1-9c5a-8f3c2a1d0e04"),
            "DescriptiveFramework",
            true,
        )?;
        w.register_propertydef(
            "ShotNotes",
            "Note",
            id("6e2b1e36-0b43-4bd1-9c5a-8f3c2a1d0e05"),
            None,
            id("01100200-0000-0000-060e-2b3401040101"),
            true,
            false,
        )?;
        let comp = w.mobs()?[0];
        let events = segment(w, w.get_objects(comp, "Slots")?[3]);
        let marker = w.get_objects(events, "Components")?[0];
        let notes = w.create("ShotNotes")?;
        w.set(notes, "Note", "Soft focus")?;
        w.set(marker, "Description", notes)
    });
}

/// The generator's `rewrite_all`, after pyaaf2's `test_rewrite`.
#[test]
fn writing_every_object_again_matches_pyaaf2() {
    check("rewrite_all", |w| {
        for obj in w.walk_references(w.root())? {
            w.add_modified(obj)?;
        }
        Ok(())
    });
}

/// The generator's `grow` and `shrink`: a stream moves out of the mini
/// stream into sectors of its own, and back.
#[test]
fn growing_a_stream_out_of_the_mini_stream_matches_pyaaf2() {
    check("grow", |w| {
        let comp = w.mobs()?[0];
        w.set(comp, "Name", "Grown ".repeat(500))
    });
}

#[test]
fn shrinking_a_stream_back_into_the_mini_stream_matches_pyaaf2() {
    check("shrink", |w| {
        let comp = w.mobs()?[0];
        w.set(comp, "Name", "Shrunk")
    });
}

/// The generator's `essence_parking`.
#[test]
fn taking_out_and_putting_back_objects_that_own_streams_matches_pyaaf2() {
    check("essence_parking", |w| {
        let mobs = w.mobs()?;
        let content = w.content()?;
        let mut essence = Vec::new();
        for (mob, size, seed) in [(mobs[1], 6000, 3), (mobs[2], 900, 4)] {
            let data = w.create("EssenceData")?;
            let mob_id = w.get_mob_id(mob, "MobID")?.expect("the mob has a MobID");
            w.set(data, "MobID", mob_id)?;
            w.append(content, "EssenceData", data)?;
            w.write_stream(data, "Data", &pattern(size, seed))?;
            essence.push(data);
        }
        w.pop_member(content, "EssenceData", essence[0])?;
        w.pop_member(content, "EssenceData", essence[1])?;
        w.append(content, "EssenceData", essence[0])?;
        Ok(())
    });
}

/// The generator's `rewrite_essence`, on what `essence_parking` left.
#[test]
fn writing_a_stream_already_in_the_file_again_matches_pyaaf2() {
    check("rewrite_essence", |w| {
        let content = w.content()?;
        let data = w.get_objects(content, "EssenceData")?[0];
        w.write_stream(data, "Data", &pattern(700, 5))
    });
}

/// The generator's `drop_essence`, on what `essence_parking` left.
#[test]
fn taking_out_an_object_whose_stream_is_already_in_the_file_matches_pyaaf2() {
    check("drop_essence", |w| {
        let content = w.content()?;
        let data = w.get_objects(content, "EssenceData")?[0];
        w.pop_member(content, "EssenceData", data)
    });
}

/// The generator's `reattach_and_grow`, after pyaaf2's `test_reattach512`.
#[test]
fn taking_every_mob_out_and_back_in_a_512_byte_sector_file_matches_pyaaf2() {
    check("reattach_512", |w| {
        let content = w.content()?;
        let mobs = w.get_objects(content, "Mobs")?;
        w.set(content, "Mobs", Vec::<ObjRef>::new())?;
        w.set(content, "Mobs", mobs)?;
        let first = w.mobs()?[0];
        w.set(first, "Name", "Grown ".repeat(800))
    });
}

/// The generator's `without_extensions`: `aaf2.open(path, 'r+',
/// extensions=False)`.
#[test]
fn changing_a_file_without_registering_extensions_matches_pyaaf2() {
    check("without_extensions", |w| {
        let comp = w.mobs()?[0];
        w.set(comp, "Name", "Without extensions")
    });
}
