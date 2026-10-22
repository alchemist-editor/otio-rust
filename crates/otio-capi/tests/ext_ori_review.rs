//! Live review through the C ABI: a reviewer commits a stroke to an
//! in-memory Furnace room, the host folds it into its timeline and
//! publishes, and the stroke reaches the reviewer's copy intact; plus the
//! merge rules and reading a stroke in progress.
//!
//! Enabling is process-wide, so this is one test, run in order.

use std::ffi::{CStr, CString};
use std::ptr;

use otio::{
    OtioBuffer, OtioDocument, OtioNode, OtioStatus, otio_buffer_free, otio_document_from_json,
    otio_document_root, otio_ext_ori_commands, otio_ext_ori_commit, otio_ext_ori_enable,
    otio_ext_ori_ensure_clip, otio_ext_ori_ensure_track, otio_ext_ori_fold, otio_ext_ori_list,
    otio_ext_ori_merge_commands, otio_ext_ori_merge_duplicates, otio_ext_ori_owner_of,
    otio_ext_ori_read_partial, otio_ext_ori_replace_by_kind, otio_ext_ori_send_partial,
    otio_ext_ori_set_commands, otio_ext_sync_handle, otio_ext_sync_session_connect,
    otio_ext_sync_session_new, otio_ext_sync_session_poll_event,
    otio_ext_sync_session_poll_transmit, otio_ext_sync_session_publish,
    otio_ext_sync_session_receive, otio_node_none,
};
use otio_ext_sync_transport::Frame;
use otio_ext_sync_transport::loopback::{ClientId, Room};

const EMPTY: OtioBuffer = OtioBuffer {
    data: ptr::null_mut(),
    len: 0,
};

fn bytes_of(buffer: OtioBuffer) -> Vec<u8> {
    let bytes = if buffer.data.is_null() {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(buffer.data.cast::<u8>(), buffer.len) }.to_vec()
    };
    unsafe { otio_buffer_free(buffer) };
    bytes
}

fn take(buffer: OtioBuffer) -> String {
    if buffer.data.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(buffer.data.cast()) }
        .to_string_lossy()
        .into_owned();
    unsafe { otio_buffer_free(buffer) };
    text
}

/// Runs a call, failing with its message unless it reports `want`.
fn expect(want: OtioStatus, call: impl FnOnce(*mut OtioBuffer) -> OtioStatus) -> String {
    let mut error = EMPTY;
    let status = call(&mut error);
    let message = take(error);
    assert_eq!(status, want, "{message}");
    message
}

fn ok(call: impl FnOnce(*mut OtioBuffer) -> OtioStatus) {
    expect(OtioStatus::Ok, call);
}

/// Runs a call that writes a buffer: its text, or `None` for no value.
fn text_of(call: impl FnOnce(*mut OtioBuffer, *mut OtioBuffer) -> OtioStatus) -> Option<String> {
    let (mut out, mut error) = (EMPTY, EMPTY);
    match call(&mut out, &mut error) {
        OtioStatus::Ok => Some(take(out)),
        OtioStatus::NoValue => None,
        status => panic!("{status:?}: {}", take(error)),
    }
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

fn new_session(settings: &str) -> u64 {
    let settings = c(settings);
    let mut session = 0;
    ok(|error| unsafe { otio_ext_sync_session_new(settings.as_ptr(), &mut session, error) });
    session
}

/// Carries frames between the sessions and the room until neither side has
/// anything more to say.
fn pump(room: &mut Room, clients: &[(u64, ClientId)]) {
    loop {
        let mut moved = false;
        for &(session, client) in clients {
            loop {
                let (mut out, mut binary, mut error) = (EMPTY, false, EMPTY);
                let status = unsafe {
                    otio_ext_sync_session_poll_transmit(
                        session,
                        1_700_000_000_000,
                        &mut out,
                        &mut binary,
                        &mut error,
                    )
                };
                if status == OtioStatus::NoValue {
                    break;
                }
                assert_eq!(status, OtioStatus::Ok, "{}", take(error));
                let frame = Frame {
                    data: bytes_of(out),
                    binary,
                };
                room.deliver(client, &frame).unwrap();
                moved = true;
            }
            while let Some(frame) = room.take(client) {
                ok(|error| unsafe {
                    otio_ext_sync_session_receive(
                        session,
                        frame.data.as_ptr(),
                        frame.data.len(),
                        frame.binary,
                        error,
                    )
                });
                moved = true;
            }
        }
        if !moved {
            return;
        }
    }
}

fn events(session: u64) -> Vec<String> {
    std::iter::from_fn(|| {
        text_of(|out, error| unsafe { otio_ext_sync_session_poll_event(session, out, error) })
    })
    .collect()
}

fn handle(document: *mut OtioDocument, timeline: OtioNode, event: &str) -> String {
    let event = c(event);
    text_of(|out, error| unsafe {
        otio_ext_sync_handle(document, timeline, event.as_ptr(), out, error)
    })
    .unwrap()
}

fn commands(document: *mut OtioDocument, holder: OtioNode) -> String {
    text_of(|out, error| unsafe { otio_ext_ori_commands(document, holder, false, out, error) })
        .unwrap()
}

fn holders(document: *mut OtioDocument, timeline: OtioNode) -> Vec<OtioNode> {
    let mut nodes = [otio_node_none(); 8];
    let mut count = 0;
    ok(|error| unsafe {
        otio_ext_ori_list(
            document,
            timeline,
            nodes.as_mut_ptr(),
            nodes.len(),
            &mut count,
            error,
        )
    });
    nodes[..count].to_vec()
}

const TIMELINE: &str = r#"{"OTIO_SCHEMA":"Timeline.1","name":"Program","metadata":{"sync":{"guid":"main"}},
  "tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"main/stack"}},"children":[
    {"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"main/v1"}},"children":[
      {"OTIO_SCHEMA":"Clip.2","name":"Beach Run","metadata":{"sync":{"guid":"main/clip-1"}},
       "source_range":{"OTIO_SCHEMA":"TimeRange.1",
         "start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},
         "duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}}]}]}}"#;

const STROKE: &str = r#"[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0,0,1]},
  {"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1","x":[0.1],"y":[0.2],"size":[0.01],"alpha":[1]}},
  {"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}]"#;

const ANCHOR: &str = r#"{"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/clip-1",
  "sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},"capturedAt":1787742000000}"#;

#[test]
fn live_review_through_the_c_abi() {
    ok(|error| unsafe { otio_ext_ori_enable(error) });
    merge_rules();
    partials();
    a_reviewers_stroke_reaches_everyone();
}

fn merge_rules() {
    let (document, timeline) = open(TIMELINE);
    let mut track = otio_node_none();
    ok(|error| unsafe { otio_ext_ori_ensure_track(document, timeline, &mut track, error) });
    let guid = c("main/clip-1");
    let mut clip = otio_node_none();
    ok(|error| unsafe {
        otio_ext_ori_ensure_clip(document, track, guid.as_ptr(), 10.0, 24.0, &mut clip, error)
    });
    let first = c(
        r#"[{"OTIO_SCHEMA":"TextAnnotation.1","uuid":"t","text":"keep"},{"OTIO_SCHEMA":"PaintStart.1","uuid":"a"}]"#,
    );
    ok(|error| unsafe { otio_ext_ori_set_commands(document, clip, first.as_ptr(), error) });

    let incoming = c(
        r#"[{"OTIO_SCHEMA":"PaintStart.1","uuid":"a","brush":"gauss"},{"OTIO_SCHEMA":"PaintStart.1","uuid":"b"}]"#,
    );
    let (mut replaced, mut added) = (0, 0);
    ok(|error| unsafe {
        otio_ext_ori_merge_commands(
            document,
            clip,
            incoming.as_ptr(),
            &mut replaced,
            &mut added,
            error,
        )
    });
    assert_eq!((replaced, added), (1, 1));
    assert!(commands(document, clip).contains(r#""brush":"gauss""#));

    let strokes = c(r#"[{"OTIO_SCHEMA":"PaintStart.1","uuid":"c"}]"#);
    let mut removed = 0;
    ok(|error| unsafe {
        otio_ext_ori_replace_by_kind(document, clip, strokes.as_ptr(), &mut removed, error)
    });
    assert_eq!(removed, 2);
    let left = commands(document, clip);
    assert!(
        left.contains(r#""uuid":"t""#) && left.contains(r#""uuid":"c""#),
        "{left}"
    );

    let mut count = 99;
    ok(|error| unsafe { otio_ext_ori_merge_duplicates(document, timeline, &mut count, error) });
    assert_eq!(count, 0);
    let uuid = c("t");
    assert!(
        text_of(|out, error| unsafe {
            otio_ext_ori_owner_of(document, clip, uuid.as_ptr(), out, error)
        })
        .is_none()
    );
}

fn partials() {
    let event = c(
        r#"{"type":"message","kind":"Annotation.1/PARTIAL","sender":"p",
        "payload":{"clip_guid":"main/clip-1","frame":10.0,"fps":24.0,"events":[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1"}]}}"#,
    );
    let partial =
        text_of(|out, error| unsafe { otio_ext_ori_read_partial(event.as_ptr(), out, error) })
            .unwrap();
    assert!(
        partial.starts_with(r#"{"clip_guid":"main/clip-1","frame":10,"fps":24,"events":[{"OTIO_SCHEMA":"PaintStart.1""#),
        "{partial}"
    );
    assert!(
        partial.contains(r#""brush":"circle""#),
        "ORI's default: {partial}"
    );
    let other = c(r#"{"type":"leader","leader":null}"#);
    assert!(
        text_of(|out, error| unsafe { otio_ext_ori_read_partial(other.as_ptr(), out, error) })
            .is_none()
    );
}

fn a_reviewers_stroke_reaches_everyone() {
    let mut room = Room::new();
    let host = new_session(&format!(
        r#"{{"token":"{}","identity":{{"credential":"ari","capabilities":["review.commit.receive"]}}}}"#,
        Room::HOST_TOKEN
    ));
    let reviewer = new_session(&format!(
        r#"{{"token":"{}","identity":{{"credential":"bea","capabilities":["review.comment.create","review.annotation.create"]}}}}"#,
        Room::JOIN_TOKEN
    ));
    let host_client = room.open(Room::HOST_TOKEN);
    ok(|error| unsafe { otio_ext_sync_session_connect(host, error) });
    pump(&mut room, &[(host, host_client)]);
    let (host_document, host_timeline) = open(TIMELINE);
    let sequence = c("main");
    text_of(|out, error| unsafe {
        otio_ext_sync_session_publish(
            host,
            host_document,
            host_timeline,
            sequence.as_ptr(),
            out,
            error,
        )
    });
    pump(&mut room, &[(host, host_client)]);

    let reviewer_client = room.open(Room::JOIN_TOKEN);
    ok(|error| unsafe { otio_ext_sync_session_connect(reviewer, error) });
    pump(
        &mut room,
        &[(host, host_client), (reviewer, reviewer_client)],
    );
    let (reviewer_document, reviewer_timeline) = open(r#"{"OTIO_SCHEMA":"Timeline.1"}"#);
    for event in events(reviewer) {
        handle(reviewer_document, reviewer_timeline, &event);
    }
    let _ = events(host);

    // Strokes in progress are not Furnace's to relay yet.
    let partial = c(r#"{"clip_guid":"main/clip-1","frame":10,"fps":24,"events":[]}"#);
    ok(|error| unsafe { otio_ext_ori_send_partial(reviewer, partial.as_ptr(), error) });
    let (mut out, mut binary) = (EMPTY, false);
    assert!(
        !expect(OtioStatus::Unsupported, |error| unsafe {
            otio_ext_sync_session_poll_transmit(reviewer, 0, &mut out, &mut binary, error)
        })
        .is_empty()
    );

    let (anchor, body, stroke, mutation) = (c(ANCHOR), c("Too dark."), c(STROKE), c(""));
    let id = text_of(|out, error| unsafe {
        otio_ext_ori_commit(
            reviewer,
            reviewer_document,
            reviewer_timeline,
            anchor.as_ptr(),
            body.as_ptr(),
            stroke.as_ptr(),
            mutation.as_ptr(),
            out,
            error,
        )
    })
    .unwrap();
    assert!(id.starts_with("mutation_"), "{id}");
    pump(
        &mut room,
        &[(reviewer, reviewer_client), (host, host_client)],
    );
    let relay = events(host)
        .into_iter()
        .find(|event| event.starts_with(r#"{"type":"review_relay""#))
        .expect("relayed to the host");
    let relay = c(&relay);
    let (mut clip, mut already) = (otio_node_none(), true);
    ok(|error| unsafe {
        otio_ext_ori_fold(
            host_document,
            host_timeline,
            relay.as_ptr(),
            &mut clip,
            &mut already,
            error,
        )
    });
    assert!(!already);
    ok(|error| unsafe {
        otio_ext_ori_fold(
            host_document,
            host_timeline,
            relay.as_ptr(),
            &mut clip,
            &mut already,
            error,
        )
    });
    assert!(already);
    let leader = c(r#"{"type":"leader","leader":null}"#);
    expect(OtioStatus::InvalidArgument, |error| unsafe {
        otio_ext_ori_fold(
            host_document,
            host_timeline,
            leader.as_ptr(),
            &mut clip,
            &mut already,
            error,
        )
    });
    text_of(|out, error| unsafe {
        otio_ext_sync_session_publish(
            host,
            host_document,
            host_timeline,
            sequence.as_ptr(),
            out,
            error,
        )
    });
    pump(
        &mut room,
        &[(host, host_client), (reviewer, reviewer_client)],
    );
    for event in events(reviewer) {
        handle(reviewer_document, reviewer_timeline, &event);
    }

    let mine = holders(reviewer_document, reviewer_timeline);
    let theirs = holders(host_document, host_timeline);
    assert_eq!(mine.len(), 1);
    assert_eq!(
        commands(reviewer_document, mine[0]),
        commands(host_document, theirs[0])
    );
    assert!(commands(reviewer_document, mine[0]).contains(r#""uuid":"s1""#));
    let uuid = c("s1");
    assert!(
        text_of(|out, error| unsafe {
            otio_ext_ori_owner_of(reviewer_document, mine[0], uuid.as_ptr(), out, error)
        })
        .is_some()
    );
}
