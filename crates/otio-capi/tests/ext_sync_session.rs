//! Sync sessions through the C ABI: a host and a viewer join an in-memory
//! Furnace room, and the host's edits reach the viewer's document.
//!
//! Enabling is process-wide, so this is one test, run in order.

use std::ffi::{CStr, CString};
use std::ptr;

use otio::{
    OtioBuffer, OtioDocument, OtioNode, OtioStatus, otio_buffer_free, otio_document_free,
    otio_document_from_json, otio_document_root, otio_ext_sync_enable, otio_ext_sync_handle,
    otio_ext_sync_session_cancel_review, otio_ext_sync_session_close,
    otio_ext_sync_session_commit_review, otio_ext_sync_session_connect,
    otio_ext_sync_session_may_send, otio_ext_sync_session_new, otio_ext_sync_session_poll_event,
    otio_ext_sync_session_poll_transmit, otio_ext_sync_session_publish,
    otio_ext_sync_session_receive, otio_ext_sync_session_register_channel,
    otio_ext_sync_session_send, otio_ext_sync_session_set_playhead, otio_ext_sync_session_state,
    otio_ext_sync_session_timeline, otio_node_name, otio_node_none, otio_node_set_name,
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

fn name(document: *mut OtioDocument, node: OtioNode) -> String {
    text_of(|out, error| unsafe { otio_node_name(document, node, out, error) }).unwrap()
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

fn state(session: u64) -> String {
    text_of(|out, error| unsafe { otio_ext_sync_session_state(session, out, error) }).unwrap()
}

const TIMELINE: &str = r#"{"OTIO_SCHEMA": "Timeline.1", "name": "cut",
    "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
        {"OTIO_SCHEMA": "Track.1", "kind": "Video", "children": [
            {"OTIO_SCHEMA": "Gap.1"}, {"OTIO_SCHEMA": "Gap.1"}]}]}}"#;

const EMPTY_TIMELINE: &str = r#"{"OTIO_SCHEMA": "Timeline.1", "name": "waiting"}"#;

#[test]
fn sessions_through_the_c_abi() {
    let mut session = 0;
    let settings = c("{}");
    expect(OtioStatus::PluginError, |error| unsafe {
        otio_ext_sync_session_new(settings.as_ptr(), &mut session, error)
    });
    ok(|error| unsafe { otio_ext_sync_enable(error) });

    settings_are_checked();
    a_host_and_a_viewer_share_a_timeline();
}

fn settings_are_checked() {
    let mut session = 0;
    for bad in [r#"{"tokn":"t"}"#, r#"{"encodings":["zstd"]}"#, "not json"] {
        let bad = c(bad);
        expect(OtioStatus::ParseError, |error| unsafe {
            otio_ext_sync_session_new(bad.as_ptr(), &mut session, error)
        });
    }
    let message = expect(OtioStatus::InvalidArgument, |error| unsafe {
        otio_ext_sync_session_connect(u64::MAX, error)
    });
    assert!(message.contains("no session"), "{message}");
    ok(|error| unsafe { otio_ext_sync_session_close(u64::MAX, error) });
}

fn a_host_and_a_viewer_share_a_timeline() {
    let mut room = Room::new();
    let host = new_session(&format!(r#"{{"token":"{}"}}"#, Room::HOST_TOKEN));
    let viewer = new_session(&format!(
        r#"{{"token":"{}","identity":{{"displayName":"Viewer","capabilities":["review.comment.create"]}},"encodings":["gzip"]}}"#,
        Room::JOIN_TOKEN
    ));
    let host_client = room.open(Room::HOST_TOKEN);
    ok(|error| unsafe { otio_ext_sync_session_connect(host, error) });
    pump(&mut room, &[(host, host_client)]);
    assert!(state(host).contains(r#""role":"host""#), "{}", state(host));

    // The host publishes its timeline before the viewer arrives.
    let (host_document, host_timeline) = open(TIMELINE);
    let sequence = c("seq-1");
    let patches = text_of(|out, error| unsafe {
        otio_ext_sync_session_publish(
            host,
            host_document,
            host_timeline,
            sequence.as_ptr(),
            out,
            error,
        )
    })
    .unwrap();
    assert!(patches.contains("replace_timeline"), "{patches}");
    pump(&mut room, &[(host, host_client)]);

    // The viewer joins, gets the timeline, and applies it to its copy.
    let viewer_client = room.open(Room::JOIN_TOKEN);
    ok(|error| unsafe { otio_ext_sync_session_connect(viewer, error) });
    pump(&mut room, &[(host, host_client), (viewer, viewer_client)]);
    let (viewer_document, viewer_timeline) = open(EMPTY_TIMELINE);
    let arrived = events(viewer);
    assert!(
        arrived[0].starts_with(r#"{"type":"timeline""#),
        "{arrived:?}"
    );
    assert!(arrived[0].contains(r#""joining":true"#), "{arrived:?}");
    assert_eq!(
        handle(viewer_document, viewer_timeline, &arrived[0]),
        r#"{"handled":"applied"}"#
    );
    assert_eq!(name(viewer_document, viewer_timeline), "cut");
    let joined = arrived
        .iter()
        .find(|event| event.starts_with(r#"{"type":"joined""#))
        .expect("a joined event");
    assert!(joined.contains(r#""role":"viewer""#), "{joined}");
    assert_eq!(
        handle(viewer_document, viewer_timeline, joined),
        r#"{"handled":"joined"}"#
    );
    let viewer_state = state(viewer);
    assert!(viewer_state.contains(r#""joined":true"#), "{viewer_state}");
    assert!(
        viewer_state.contains(r#""activeSequence":"seq-1""#),
        "{viewer_state}"
    );
    assert!(
        text_of(|out, error| unsafe {
            otio_ext_sync_session_timeline(viewer, sequence.as_ptr(), out, error)
        })
        .unwrap()
        .contains(r#""sequenceId":"seq-1""#)
    );
    let unknown = c("seq-2");
    assert_eq!(
        text_of(|out, error| unsafe {
            otio_ext_sync_session_timeline(viewer, unknown.as_ptr(), out, error)
        }),
        None
    );
    assert!(
        events(host)
            .iter()
            .any(|event| event.contains(r#""type":"roster""#))
    );

    // The host renames its timeline; the viewer gets just that change.
    let renamed = c("cut 2");
    ok(|error| unsafe {
        otio_node_set_name(host_document, host_timeline, renamed.as_ptr(), error)
    });
    let patches = text_of(|out, error| unsafe {
        otio_ext_sync_session_publish(
            host,
            host_document,
            host_timeline,
            sequence.as_ptr(),
            out,
            error,
        )
    })
    .unwrap();
    assert!(patches.contains("set_property"), "{patches}");
    pump(&mut room, &[(host, host_client), (viewer, viewer_client)]);
    let arrived = events(viewer);
    assert_eq!(arrived.len(), 1, "{arrived:?}");
    assert_eq!(
        handle(viewer_document, viewer_timeline, &arrived[0]),
        r#"{"handled":"applied"}"#
    );
    assert_eq!(name(viewer_document, viewer_timeline), "cut 2");

    // Only the host holds the timeline's lease.
    let message = expect(OtioStatus::InvalidArgument, |error| unsafe {
        otio_ext_sync_session_publish(
            viewer,
            viewer_document,
            viewer_timeline,
            sequence.as_ptr(),
            &mut OtioBuffer { ..EMPTY },
            error,
        )
    });
    assert!(message.contains("timeline"), "{message}");
    let timeline_channel = c("timeline");
    let mut may = true;
    ok(|error| unsafe {
        otio_ext_sync_session_may_send(viewer, timeline_channel.as_ptr(), &mut may, error)
    });
    assert!(!may);

    // The viewer's playhead reaches the host.
    ok(|error| unsafe {
        otio_ext_sync_session_set_playhead(viewer, sequence.as_ptr(), 24.0, true, 1.0, error)
    });
    pump(&mut room, &[(host, host_client), (viewer, viewer_client)]);
    let heard = events(host);
    assert!(
        heard.iter().any(
            |event| event.contains(r#""type":"playhead""#) && event.contains(r#""frame":24.0"#)
        ),
        "{heard:?}"
    );
    assert_eq!(
        handle(host_document, host_timeline, &heard[0]),
        r#"{"handled":"ignored"}"#
    );

    // A review commit from a client without the capability is refused; one
    // cancelled before it goes is never sent.
    let commit = c(r#"{"anchor":{"frame":3},"bodyMarkdown":"Too long"}"#);
    expect(OtioStatus::InvalidArgument, |error| unsafe {
        otio_ext_sync_session_commit_review(
            host,
            commit.as_ptr(),
            &mut OtioBuffer { ..EMPTY },
            error,
        )
    });
    let id = text_of(|out, error| unsafe {
        otio_ext_sync_session_commit_review(viewer, commit.as_ptr(), out, error)
    })
    .unwrap();
    assert!(id.starts_with("mutation_"), "{id}");
    let id = c(&id);
    let mut cancelled = false;
    ok(|error| unsafe {
        otio_ext_sync_session_cancel_review(viewer, id.as_ptr(), &mut cancelled, error)
    });
    assert!(cancelled);

    // Named channels, and messages Furnace would close the socket over.
    let (name_text, ledger) = (c("ext.notes"), c("ledger"));
    expect(OtioStatus::InvalidArgument, |error| unsafe {
        otio_ext_sync_session_register_channel(host, name_text.as_ptr(), ledger.as_ptr(), error)
    });
    let (signal, kind, payload) = (c("signal"), c("signal"), c(r#"{"to":"nobody"}"#));
    ok(|error| unsafe {
        otio_ext_sync_session_send(
            viewer,
            signal.as_ptr(),
            kind.as_ptr(),
            payload.as_ptr(),
            error,
        )
    });
    let bad_payload = c("not json");
    ok(|error| unsafe {
        otio_ext_sync_session_send(
            viewer,
            signal.as_ptr(),
            kind.as_ptr(),
            bad_payload.as_ptr(),
            error,
        )
    });
    pump(&mut room, &[(host, host_client)]);
    let (mut out, mut binary) = (EMPTY, false);
    let message = expect(OtioStatus::Ok, |error| unsafe {
        otio_ext_sync_session_poll_transmit(viewer, 0, &mut out, &mut binary, error)
    });
    assert!(message.is_empty());
    bytes_of(out);
    expect(OtioStatus::ParseError, |error| unsafe {
        otio_ext_sync_session_poll_transmit(
            viewer,
            0,
            &mut OtioBuffer { ..EMPTY },
            &mut binary,
            error,
        )
    });

    // A malformed event is refused; freeing twice is harmless.
    let event = c(r#"{"type":"timeline"}"#);
    expect(OtioStatus::ParseError, |error| unsafe {
        otio_ext_sync_handle(
            viewer_document,
            viewer_timeline,
            event.as_ptr(),
            &mut OtioBuffer { ..EMPTY },
            error,
        )
    });
    for session in [host, viewer, viewer] {
        ok(|error| unsafe { otio_ext_sync_session_close(session, error) });
    }
    expect(OtioStatus::InvalidArgument, |error| unsafe {
        otio_ext_sync_session_connect(host, error)
    });
    unsafe {
        otio_document_free(host_document);
        otio_document_free(viewer_document);
    }
}
