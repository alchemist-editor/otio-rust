//! ORI Annotations through the C ABI: refused until enabled, then the file
//! ORI's own Python writes, rebuilt through these calls byte for byte, and
//! edits by uuid.
//!
//! Enabling is process-wide, so this is one test, run in order.

use std::ffi::{CStr, CString};
use std::fs;
use std::path::Path;
use std::ptr;

use otio::{
    OtioBuffer, OtioDocument, OtioNode, OtioStatus, otio_buffer_free, otio_document_from_json,
    otio_document_root, otio_document_to_json, otio_ext_ori_add_commands, otio_ext_ori_aspect_half,
    otio_ext_ori_bounds_to_ori, otio_ext_ori_commands, otio_ext_ori_describe, otio_ext_ori_enable,
    otio_ext_ori_ensure_clip, otio_ext_ori_ensure_track, otio_ext_ori_find_clip,
    otio_ext_ori_find_track, otio_ext_ori_is_enabled, otio_ext_ori_list,
    otio_ext_ori_list_at_frame, otio_ext_ori_list_for_clip, otio_ext_ori_new_uuid,
    otio_ext_ori_ori_to_bounds, otio_ext_ori_ori_to_pixels, otio_ext_ori_pixels_to_ori,
    otio_ext_ori_remove_commands, otio_ext_ori_replace_commands, otio_ext_ori_set_commands,
    otio_ext_ori_timestamp_now, otio_ext_sync_set_guid, otio_node_children, otio_node_none,
};
use otio_ext_ori_annotations::annotations;

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

fn status(call: impl FnOnce(*mut OtioBuffer) -> OtioStatus) -> OtioStatus {
    let mut error = EMPTY;
    let status = call(&mut error);
    take(error);
    status
}

fn c(text: &str) -> CString {
    CString::new(text).unwrap()
}

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/ori/otio-ext-ori-annotations/tests/fixtures")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn open(json: &str) -> (*mut OtioDocument, OtioNode) {
    let json = c(json);
    let mut document = ptr::null_mut();
    ok(|error| unsafe { otio_document_from_json(json.as_ptr(), &mut document, error) });
    let mut root = otio_node_none();
    ok(|error| unsafe { otio_document_root(document, &mut root, error) });
    (document, root)
}

fn written(document: *mut OtioDocument) -> String {
    text_of(|out, error| unsafe { otio_document_to_json(document, 4, out, error) })
}

fn nodes(
    call: impl Fn(*mut OtioNode, usize, *mut usize, *mut OtioBuffer) -> OtioStatus,
) -> Vec<OtioNode> {
    let mut count = 0;
    ok(|error| call(ptr::null_mut(), 0, &mut count, error));
    let mut found = vec![otio_node_none(); count];
    ok(|error| call(found.as_mut_ptr(), count, &mut count, error));
    found
}

fn commands(document: *mut OtioDocument, holder: OtioNode) -> String {
    text_of(|out, error| unsafe { otio_ext_ori_commands(document, holder, false, out, error) })
}

fn describe(document: *mut OtioDocument, holder: OtioNode) -> String {
    text_of(|out, error| unsafe { otio_ext_ori_describe(document, holder, out, error) })
}

fn find_track(document: *mut OtioDocument, timeline: OtioNode) -> OtioNode {
    let mut track = otio_node_none();
    ok(|error| unsafe { otio_ext_ori_find_track(document, timeline, &mut track, error) });
    track
}

fn find_clip(document: *mut OtioDocument, track: OtioNode, frame: i64) -> OtioNode {
    let mut clip = otio_node_none();
    let guid = c("shot");
    ok(|error| unsafe {
        otio_ext_ori_find_clip(document, track, guid.as_ptr(), frame, &mut clip, error)
    });
    clip
}

/// The media ORI started from: `ori_built.otio` without its annotation
/// track.
fn base() -> String {
    let mut document = otio_core::from_str(&fixture("ori_built.otio")).expect("reads");
    let timeline = document.root().expect("a timeline");
    let track = annotations::find_track(&document, timeline)
        .expect("looks")
        .expect("has one");
    let stack = document.parent_of(track).expect("in the stack");
    document.detach_child(stack, track).expect("detaches");
    document.remove_recursive(track).expect("removes");
    otio_core::to_string_pretty(&document, 4).expect("writes")
}

#[test]
fn ori_annotations_through_the_c_abi() {
    // Refused until enabled.
    let (built, timeline) = open(&fixture("ori_built.otio"));
    let mut on = true;
    ok(|error| unsafe { otio_ext_ori_is_enabled(&mut on, error) });
    assert!(!on);
    let mut track = otio_node_none();
    assert_eq!(
        status(|error| unsafe { otio_ext_ori_find_track(built, timeline, &mut track, error) }),
        OtioStatus::PluginError
    );

    ok(|error| unsafe { otio_ext_ori_enable(error) });
    ok(|error| unsafe { otio_ext_ori_is_enabled(&mut on, error) });
    assert!(on);

    // Read what ORI wrote.
    let holders = nodes(|out, capacity, count, error| unsafe {
        otio_ext_ori_list(built, timeline, out, capacity, count, error)
    });
    assert_eq!(holders.len(), 2);
    assert_eq!(
        describe(built, holders[0]),
        r#"{"form":"track","clip_guid":"shot","clip_name":null,"frame":{"value":10.0,"rate":24.0},"note":null,"status":null,"command_count":4}"#
    );
    let track = find_track(built, timeline);
    let at_10 = find_clip(built, track, 10);
    let at_4 = find_clip(built, track, 4);
    assert_eq!(holders, [at_10, at_4]);
    let shot = c("shot");
    assert_eq!(
        nodes(|out, capacity, count, error| unsafe {
            otio_ext_ori_list_at_frame(
                built,
                timeline,
                shot.as_ptr(),
                4,
                out,
                capacity,
                count,
                error,
            )
        }),
        [at_4]
    );
    assert_eq!(
        nodes(|out, capacity, count, error| unsafe {
            otio_ext_ori_list_for_clip(built, timeline, shot.as_ptr(), out, capacity, count, error)
        })
        .len(),
        2
    );
    let mut missing = otio_node_none();
    assert_eq!(
        status(|error| unsafe {
            otio_ext_ori_find_clip(built, track, shot.as_ptr(), 99, &mut missing, error)
        }),
        OtioStatus::NoValue
    );
    let (commands_10, commands_4) = (commands(built, at_10), commands(built, at_4));

    // Rebuild it from the media alone, through the C ABI, as ORI built it.
    let (document, timeline) = open(&base());
    let mut none = otio_node_none();
    assert_eq!(
        status(|error| unsafe { otio_ext_ori_find_track(document, timeline, &mut none, error) }),
        OtioStatus::NoValue
    );
    let mut track = otio_node_none();
    ok(|error| unsafe { otio_ext_ori_ensure_track(document, timeline, &mut track, error) });
    let ensure = |frame: f64| {
        let mut clip = otio_node_none();
        ok(|error| unsafe {
            otio_ext_ori_ensure_clip(
                document,
                track,
                shot.as_ptr(),
                frame,
                24.0,
                &mut clip,
                error,
            )
        });
        clip
    };
    let new_10 = ensure(10.0);
    let json = c(&commands_10);
    ok(|error| unsafe { otio_ext_ori_add_commands(document, new_10, json.as_ptr(), error) });
    let new_4 = ensure(4.0);
    let json = c(&commands_4);
    ok(|error| unsafe { otio_ext_ori_add_commands(document, new_4, json.as_ptr(), error) });
    assert_eq!(ensure(10.0), new_10, "asking again finds the same clip");

    let children = nodes(|out, capacity, count, error| unsafe {
        otio_node_children(document, track, out, capacity, count, error)
    });
    assert_eq!(children.len(), 3, "a gap and two clips");
    for (node, guid) in [
        (track, "annotations"),
        (children[0], "gap-0"),
        (new_10, "annotation-10"),
        (new_4, "annotation-4"),
    ] {
        let guid = c(guid);
        ok(|error| unsafe { otio_ext_sync_set_guid(document, node, guid.as_ptr(), error) });
    }
    assert_eq!(
        written(document).trim_end(),
        fixture("ori_built.otio").trim_end()
    );

    // Edits by uuid leave everyone else's commands alone.
    let mut replaced = false;
    let uuid = c("stroke-1");
    let text = c(r#"[{"OTIO_SCHEMA":"TextAnnotation.1","uuid":"stroke-1","text":"was a stroke"}]"#);
    ok(|error| unsafe {
        otio_ext_ori_replace_commands(
            document,
            new_10,
            uuid.as_ptr(),
            text.as_ptr(),
            &mut replaced,
            error,
        )
    });
    assert!(replaced);
    let after = commands(document, new_10);
    // A command missing ORI's fields gets the defaults ORI's class gives it.
    assert!(
        after.starts_with(r#"[{"OTIO_SCHEMA":"TextAnnotation.1","font":null,"#),
        "{after}"
    );
    assert!(
        after.contains(r#""text":"was a stroke","timestamp":null,"uuid":"stroke-1"}"#),
        "{after}"
    );
    assert!(after.contains(r#""uuid":"text-1""#), "{after}");

    let mut removed = 0;
    let uuids = c(r#"["text-1", "nobody"]"#);
    ok(|error| unsafe {
        otio_ext_ori_remove_commands(document, new_10, uuids.as_ptr(), &mut removed, error)
    });
    assert_eq!(removed, 1);
    let empty = c("[]");
    ok(|error| unsafe { otio_ext_ori_set_commands(document, new_4, empty.as_ptr(), error) });
    assert_eq!(commands(document, new_4), "[]");

    // Bad JSON and objects that are not ORI's are refused.
    for bad in [
        "not json",
        r#"{"OTIO_SCHEMA":"PaintEnd.1"}"#,
        r#"[{"OTIO_SCHEMA":"PaintEnd.1","points":{"OTIO_SCHEMA":"Mystery.1"}}]"#,
    ] {
        let bad = c(bad);
        assert_eq!(
            status(|error| unsafe {
                otio_ext_ori_add_commands(document, new_4, bad.as_ptr(), error)
            }),
            OtioStatus::ParseError,
            "{bad:?}"
        );
    }
    assert_eq!(
        status(|error| unsafe {
            otio_ext_ori_add_commands(document, timeline, empty.as_ptr(), error)
        }),
        OtioStatus::WrongKind
    );

    // New uuids and timestamps.
    let uuid = text_of(|out, error| unsafe { otio_ext_ori_new_uuid(out, error) });
    assert_eq!(uuid.len(), 36, "{uuid}");
    let now = text_of(|out, error| unsafe { otio_ext_ori_timestamp_now(out, error) });
    assert_eq!(&now[10..11], "T", "{now}");

    // Coordinates, against ORI's own test numbers.
    let (mut x, mut y) = (0.0, 0.0);
    ok(|error| unsafe {
        otio_ext_ori_pixels_to_ori(1920.0, 1080.0, 0.0, 0.0, &mut x, &mut y, error)
    });
    assert!((x + 8.0 / 9.0).abs() < 1e-12 && (y - 0.5).abs() < 1e-12);
    let (mut px, mut py) = (1.0, 1.0);
    ok(|error| unsafe {
        otio_ext_ori_ori_to_pixels(1920.0, 1080.0, x, y, &mut px, &mut py, error)
    });
    assert!(px.abs() < 1e-9 && py.abs() < 1e-9);
    let mut half = 0.0;
    ok(|error| unsafe { otio_ext_ori_aspect_half(1920.0, 0.0, &mut half, error) });
    assert_eq!(half, 8.0 / 9.0);
    ok(|error| unsafe {
        otio_ext_ori_ori_to_bounds(0.5, 0.5, -16.0, -9.0, 16.0, 9.0, &mut x, &mut y, error)
    });
    assert_eq!((x, y), (9.0, 9.0));
    ok(|error| unsafe {
        otio_ext_ori_bounds_to_ori(x, y, -16.0, -9.0, 16.0, 9.0, &mut x, &mut y, error)
    });
    assert_eq!((x, y), (0.5, 0.5));
    assert_eq!(
        status(|error| unsafe {
            otio_ext_ori_pixels_to_ori(1920.0, 1080.0, 0.0, 0.0, &mut x, ptr::null_mut(), error)
        }),
        OtioStatus::InvalidArgument
    );
}
