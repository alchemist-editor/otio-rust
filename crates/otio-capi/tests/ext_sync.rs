//! Timeline sync through the C ABI: refused until enabled, then Furnace's
//! golden corpus and a transaction carried from one document to another.
//!
//! Enabling is process-wide, so this is one test, run in order.

use std::ffi::{CStr, CString};
use std::fs;
use std::path::Path;
use std::ptr;

use otio::{
    OtioBuffer, OtioDocument, OtioNode, OtioStatus, otio_buffer_free, otio_document_free,
    otio_document_from_json, otio_document_root, otio_ext_sync_apply, otio_ext_sync_apply_to,
    otio_ext_sync_begin, otio_ext_sync_commit, otio_ext_sync_commit_journal, otio_ext_sync_diff,
    otio_ext_sync_diff_stats, otio_ext_sync_enable, otio_ext_sync_find, otio_ext_sync_guid,
    otio_ext_sync_new_guid, otio_ext_sync_rollback, otio_ext_sync_stamp,
    otio_ext_sync_start_journal, otio_ext_sync_stop_journal, otio_node_name, otio_node_none,
    otio_node_set_name,
};
use otio_ext_sync::{Snapshot, json_eq};

const EMPTY: OtioBuffer = OtioBuffer {
    data: ptr::null_mut(),
    len: 0,
};

fn take(buffer: OtioBuffer) -> String {
    let text = if buffer.data.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(buffer.data.cast()) }
            .to_string_lossy()
            .into_owned()
    };
    unsafe { otio_buffer_free(buffer) };
    text
}

/// Runs a call that writes a buffer, failing with its message if it fails.
fn text_of(call: impl FnOnce(*mut OtioBuffer, *mut OtioBuffer) -> OtioStatus) -> String {
    let (mut out, mut error) = (EMPTY, EMPTY);
    let status = call(&mut out, &mut error);
    assert_eq!(status, OtioStatus::Ok, "{}", take(error));
    take(out)
}

fn ok(call: impl FnOnce(*mut OtioBuffer) -> OtioStatus) {
    let mut error = EMPTY;
    let status = call(&mut error);
    assert_eq!(status, OtioStatus::Ok, "{}", take(error));
}

fn c(text: &str) -> CString {
    CString::new(text).unwrap()
}

fn open(json: &str) -> (*mut OtioDocument, OtioNode) {
    let json = c(json);
    let mut document = ptr::null_mut();
    ok(|error| unsafe { otio_document_from_json(json.as_ptr(), &mut document, error) });
    let mut root = otio_node_none();
    ok(|error| unsafe { otio_document_root(document, &mut root, error) });
    (document, root)
}

fn name(document: *mut OtioDocument, node: OtioNode) -> String {
    text_of(|out, error| unsafe { otio_node_name(document, node, out, error) })
}

fn rename(document: *mut OtioDocument, node: OtioNode, to: &str) {
    let to = c(to);
    ok(|error| unsafe { otio_node_set_name(document, node, to.as_ptr(), error) });
}

const TIMELINE: &str = r#"{"OTIO_SCHEMA": "Timeline.1", "name": "cut",
    "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
        {"OTIO_SCHEMA": "Track.1", "kind": "Video", "children": [
            {"OTIO_SCHEMA": "Gap.1"}, {"OTIO_SCHEMA": "Gap.1"},
            {"OTIO_SCHEMA": "Gap.1"}, {"OTIO_SCHEMA": "Gap.1"}]}]}}"#;

#[test]
fn sync_through_the_c_abi() {
    refused_until_enabled();
    ok(|error| unsafe { otio_ext_sync_enable(error) });
    golden_corpus();
    a_transaction_carries_an_edit_to_another_document();
    rolling_back_and_the_journal();
}

fn refused_until_enabled() {
    let mut error = EMPTY;
    let status = unsafe { otio_ext_sync_new_guid(&mut OtioBuffer { ..EMPTY }, &mut error) };
    assert_eq!(status, OtioStatus::PluginError);
    assert!(take(error).contains("otio_ext_sync_enable"));
}

fn golden_corpus() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/sync/otio-ext-sync/tests/fixtures/otiodiff");
    let mut cases = 0;
    for entry in fs::read_dir(fixtures).unwrap() {
        let directory = entry.unwrap().path();
        if !directory.is_dir() {
            continue;
        }
        cases += 1;
        let case = directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let old_text = fs::read_to_string(directory.join("old.json")).unwrap();
        let new_text = fs::read_to_string(directory.join("new.json")).unwrap();
        let (old, new) = (c(&old_text), c(&new_text));

        let patches = text_of(|out, error| unsafe {
            otio_ext_sync_diff(old.as_ptr(), new.as_ptr(), true, out, error)
        });
        let want = fs::read_to_string(directory.join("patches.json")).unwrap();
        assert_eq!(patches + "\n", want, "{case}: patches");

        let stats = text_of(|out, error| unsafe {
            otio_ext_sync_diff_stats(old.as_ptr(), new.as_ptr(), out, error)
        });
        let fell_back = [
            "generation_regression",
            "guidless_transition",
            "metadata_sync_threshold_fallback",
            "sequence_changed",
        ]
        .contains(&case.as_str());
        assert!(
            stats.contains(&format!("\"fellBack\":{fell_back}")),
            "{case}: {stats}"
        );

        let old_timeline = c(&Snapshot::from_json(&old_text).unwrap().timeline);
        let compact = text_of(|out, error| unsafe {
            otio_ext_sync_diff(old.as_ptr(), new.as_ptr(), false, out, error)
        });
        let compact = c(&compact);
        let applied = text_of(|out, error| unsafe {
            otio_ext_sync_apply(old_timeline.as_ptr(), compact.as_ptr(), out, error)
        });
        let new_timeline = Snapshot::from_json(&new_text).unwrap().timeline;
        assert!(json_eq(&applied, &new_timeline).unwrap(), "{case}: apply");
    }
    assert_eq!(cases, 11);
}

fn a_transaction_carries_an_edit_to_another_document() {
    let (editor, timeline) = open(TIMELINE);
    let main = c("main");
    let before = text_of(|out, error| unsafe {
        otio_ext_sync_begin(editor, timeline, main.as_ptr(), 1, out, error)
    });
    let snapshot = Snapshot::from_json(&before).unwrap();
    assert_eq!(
        (snapshot.sequence_id.as_str(), snapshot.generation),
        ("main", 1)
    );
    let (peer, peer_timeline) = open(&snapshot.timeline);

    rename(editor, timeline, "fine cut");
    let before = c(&before);
    let patches = text_of(|out, error| unsafe {
        otio_ext_sync_commit(editor, timeline, before.as_ptr(), out, error)
    });
    assert!(patches.contains(r#""path":"/name""#), "{patches}");
    let patches = c(&patches);
    ok(|error| unsafe { otio_ext_sync_apply_to(peer, peer_timeline, patches.as_ptr(), error) });
    assert_eq!(name(peer, peer_timeline), "fine cut", "the handle survives");

    // Both copies name the timeline by the same GUID, and stamping finds
    // nothing left to do.
    let guid = text_of(|out, error| unsafe { otio_ext_sync_guid(editor, timeline, out, error) });
    let guid = c(&guid);
    let mut found = otio_node_none();
    ok(|error| unsafe {
        otio_ext_sync_find(peer, peer_timeline, guid.as_ptr(), &mut found, error)
    });
    assert_eq!(found, peer_timeline);
    let mut count = usize::MAX;
    ok(|error| unsafe { otio_ext_sync_stamp(peer, peer_timeline, &mut count, error) });
    assert_eq!(count, 0);

    let missing = c("nobody");
    let status = unsafe {
        otio_ext_sync_find(
            peer,
            peer_timeline,
            missing.as_ptr(),
            &mut found,
            ptr::null_mut(),
        )
    };
    assert_eq!(status, OtioStatus::NoValue);
    assert!(!text_of(|out, error| unsafe { otio_ext_sync_new_guid(out, error) }).is_empty());

    unsafe {
        otio_document_free(editor);
        otio_document_free(peer);
    }
}

fn rolling_back_and_the_journal() {
    let (document, timeline) = open(TIMELINE);
    let main = c("main");
    let before = text_of(|out, error| unsafe {
        otio_ext_sync_begin(document, timeline, main.as_ptr(), 1, out, error)
    });
    rename(document, timeline, "scrapped");
    let before = c(&before);
    ok(|error| unsafe { otio_ext_sync_rollback(document, timeline, before.as_ptr(), error) });
    assert_eq!(name(document, timeline), "cut");

    let changed = |document| {
        let mut changed = false;
        ok(|error| unsafe {
            otio_ext_sync_commit_journal(document, timeline, &mut changed, error)
        });
        changed
    };
    ok(|error| unsafe { otio_ext_sync_start_journal(document, timeline, error) });
    assert!(!changed(document));
    rename(document, timeline, "journalled");
    assert!(changed(document));
    assert!(!changed(document), "a commit starts a fresh record");
    ok(|error| unsafe { otio_ext_sync_stop_journal(document, timeline, error) });
    unsafe { otio_document_free(document) };
}
