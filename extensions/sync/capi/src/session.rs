//! Sync sessions over the C interface, `otio_ext_sync_session_*`.
//!
//! A session is named by a number, which `otio_ext_sync_session_new` hands
//! out and `otio_ext_sync_session_close` retires, so every language holds
//! one the same way. The session speaks Furnace's protocol v2 and owns no
//! socket: the program opens one, hands the session each frame that
//! arrives, and sends each frame the session gives it. Settings, events and
//! state cross as JSON, in the shapes `otio_ext_sync_transport::json`
//! documents.

use std::collections::BTreeMap;
use std::ffi::c_char;
use std::sync::Mutex;

use otio_ext_sync::session::{self, Channel, Delivery, Playhead, Selection, Session};
use otio_ext_sync_transport::{Connection, Frame, Furnace, json};

use crate::buffer::OtioBuffer;
use crate::handle::{OtioDocument, OtioNode, bytes, document_mut, text, write_out};
use crate::status::{Fault, OtioStatus, Outcome, guard};

use super::enabled;

struct Registry {
    next: u64,
    open: BTreeMap<u64, Connection<Furnace>>,
    /// Sessions a call has taken out of `open`, and whether
    /// `otio_ext_sync_session_close` asked for one while it was out.
    busy: BTreeMap<u64, bool>,
}

static SESSIONS: Mutex<Registry> = Mutex::new(Registry {
    next: 1,
    open: BTreeMap::new(),
    busy: BTreeMap::new(),
});

fn registry() -> std::sync::MutexGuard<'static, Registry> {
    SESSIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Puts a session taken out of the registry back when the call ends, even
/// one that fails: the registry is not locked while a call runs, so a hook
/// script the call runs may use other sessions. A session closed while the
/// call had it is released instead.
struct Borrowed {
    id: u64,
    connection: Option<Connection<Furnace>>,
}

impl Drop for Borrowed {
    fn drop(&mut self) {
        let Some(connection) = self.connection.take() else {
            return;
        };
        let mut open = registry();
        if open.busy.remove(&self.id) == Some(true) {
            drop(open);
            drop(connection);
        } else {
            open.open.insert(self.id, connection);
        }
    }
}

/// Runs `body` on session `id`.
fn with<T>(id: u64, body: impl FnOnce(&mut Connection<Furnace>) -> Outcome<T>) -> Outcome<T> {
    enabled()?;
    let taken = {
        let mut open = registry();
        let taken = open.open.remove(&id).ok_or_else(|| {
            Fault::invalid(format!(
                "there is no session {id}: it was closed, or is in use by another call"
            ))
        })?;
        open.busy.insert(id, false);
        taken
    };
    let mut borrowed = Borrowed {
        id,
        connection: Some(taken),
    };
    body(borrowed.connection.as_mut().expect("just put there"))
}

fn session_fault(error: &session::Error) -> Fault {
    let status = match error {
        session::Error::Patch(error) => return super::sync_fault(error),
        session::Error::Plugin(_) | session::Error::Vetoed(_) => OtioStatus::PluginError,
        _ => OtioStatus::InvalidArgument,
    };
    Fault::new(status, error.to_string())
}

fn wire_fault(error: &otio_ext_sync_transport::Error) -> Fault {
    use otio_ext_sync_transport::Error;
    let status = match error {
        Error::Json(_) => OtioStatus::ParseError,
        Error::Unsupported(_) => OtioStatus::Unsupported,
        Error::Session(error) => return session_fault(error),
        _ => OtioStatus::IoError,
    };
    Fault::new(status, error.to_string())
}

/// Makes a session from the JSON object `settings`, and writes the number
/// that names it to `out_session`. Release it with
/// `otio_ext_sync_session_close`.
///
/// `settings` holds `token`, the token the server issued; `identity`, who
/// the client is (`displayName`, `app`, `appVersion`, `credential`,
/// `deviceId`, `clientKind`, `formFactor`, `deviceName` and
/// `capabilities`); and `encodings`, the compression it accepts (`br`,
/// `gzip`). Each may be left out: `{}` is a guest desktop editor accepting
/// every encoding.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_new(
    settings: *const c_char,
    out_session: *mut u64,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let read = json::settings_from_json(unsafe { text(settings, "settings") }?)
            .map_err(|error| wire_fault(&error))?;
        let connection = Connection::new(
            Session::new(read.config),
            Furnace::with_encodings(read.encodings),
        );
        let mut open = registry();
        let id = open.next;
        open.next += 1;
        open.open.insert(id, connection);
        drop(open);
        unsafe { write_out(out_session, id, "out_session") }
    })
}

/// Releases `session`, whose number names nothing afterwards. Closing one
/// already closed does nothing and succeeds. A session another call is
/// using is released when that call returns.
///
/// It sends nothing: call `otio_ext_sync_session_leave` and send the
/// goodbye first to leave politely.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_close(
    session: u64,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let mut open = registry();
        let freed = open.open.remove(&session);
        if let Some(closing) = open.busy.get_mut(&session) {
            *closing = true;
        }
        drop(open);
        drop(freed);
        Ok(())
    })
}

/// Starts joining on a new connection: call it once the socket is open,
/// and again after a reconnect. The hello is the next frame to send.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_connect(
    session: u64,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        with(session, |connection| {
            connection.connect();
            Ok(())
        })
    })
}

/// Hands `session` a frame that arrived: `binary` says whether it was a
/// binary WebSocket frame rather than a text one.
///
/// A failure means the connection broke the protocol or lost frames:
/// close the socket, call `otio_ext_sync_session_disconnected`, and
/// connect again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_receive(
    session: u64,
    data: *const u8,
    len: usize,
    binary: bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let data = unsafe { bytes(data, len, "data") }?;
        let frame = Frame {
            data: data.to_vec(),
            binary,
        };
        with(session, |connection| {
            connection
                .receive(&frame)
                .map_err(|error| wire_fault(&error))
        })
    })
}

/// Writes the next frame to send to `out_bytes`, stamped with `now` in
/// Unix milliseconds, and whether to send it as a binary WebSocket frame
/// rather than a text one to `out_binary`.
///
/// Reports `OTIO_STATUS_NO_VALUE` when there is nothing to send. Call it
/// until then after every other session call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_poll_transmit(
    session: u64,
    now: i64,
    out_bytes: *mut OtioBuffer,
    out_binary: *mut bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let frame = with(session, |connection| {
            connection
                .poll_transmit(now)
                .map_err(|error| wire_fault(&error))
        })?
        .ok_or_else(|| Fault::no_value("a frame to send"))?;
        unsafe {
            write_out(out_binary, frame.binary, "out_binary")?;
            write_out(out_bytes, OtioBuffer::from_bytes(&frame.data), "out_bytes")
        }
    })
}

/// Writes the next event to `out_event` as a JSON object, whose `type` is
/// `joined`, `timeline`, `playhead`, `selection`, `roster`, `leader`,
/// `review_relay`, `review_committed`, `review_rejected`, `error`, `ended`
/// or `message`.
///
/// Reports `OTIO_STATUS_NO_VALUE` when there is none. Pass a `timeline` or
/// `joined` event to `otio_ext_sync_handle` to apply it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_poll_event(
    session: u64,
    out_event: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let event = with(session, |connection| Ok(connection.poll_event()))?
            .ok_or_else(|| Fault::no_value("an event"))?;
        let json = json::event_to_json(&event);
        unsafe { write_out(out_event, OtioBuffer::from_str(&json), "out_event") }
    })
}

/// Tells `session` the socket closed. It waits for
/// `otio_ext_sync_session_connect`, keeping its timelines and the review
/// commits not yet acknowledged.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_disconnected(
    session: u64,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        with(session, |connection| {
            connection.disconnected();
            Ok(())
        })
    })
}

/// Leaves the session: the goodbye is the next frame to send, after which
/// the socket can close.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_leave(
    session: u64,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        with(session, |connection| {
            connection.session_mut().leave();
            Ok(())
        })
    })
}

/// Writes where `session` stands to `out_state` as a JSON object:
/// `joined`, `ended`, `sessionId`, `selfId`, `selfUserId`, `role`,
/// `leader`, `peers`, `activeSequence`, `sequences`, `pendingReviews`, and
/// the protocol `version` and `encodings` the server chose.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_state(
    session: u64,
    out_state: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let state = with(session, |connection| {
            Ok(json::state_to_json(
                connection.session(),
                connection.profile().version(),
                connection.profile().accepted(),
            ))
        })?;
        unsafe { write_out(out_state, OtioBuffer::from_str(&state), "out_state") }
    })
}

/// Writes the latest snapshot of `sequence_id` that `session` sent or
/// received to `out_snapshot`.
///
/// Reports `OTIO_STATUS_NO_VALUE` when it has none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_timeline(
    session: u64,
    sequence_id: *const c_char,
    out_snapshot: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let sequence_id = unsafe { text(sequence_id, "sequence_id") }?;
        let snapshot = with(session, |connection| {
            Ok(connection
                .session()
                .timeline(sequence_id)
                .map(otio_ext_sync::Snapshot::to_json))
        })?
        .ok_or_else(|| Fault::no_value("a snapshot of that sequence"))?;
        unsafe {
            write_out(
                out_snapshot,
                OtioBuffer::from_str(&snapshot),
                "out_snapshot",
            )
        }
    })
}

/// Writes whether `session` may send on `channel` now to `out_may`: it has
/// joined, and its role holds the channel's lease.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_may_send(
    session: u64,
    channel: *const c_char,
    out_may: *mut bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let channel = Channel::from_name(unsafe { text(channel, "channel") }?);
        let may = with(session, |connection| {
            Ok(connection.session().may_send(&channel))
        })?;
        unsafe { write_out(out_may, may, "out_may") }
    })
}

/// Sends `timeline`, as the sequence `sequence_id`, to the session, and
/// writes the patches from the version sent before to `out_patches` as a
/// JSON list. Only the host may.
///
/// Stamps missing GUIDs and runs `sync_before_send`, whose scripts may stop
/// it. Sending a timeline that has not changed sends nothing and writes
/// `[]`, unless it switches the active sequence.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_publish(
    session: u64,
    target: *mut OtioDocument,
    timeline: OtioNode,
    sequence_id: *const c_char,
    out_patches: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        let sequence_id = unsafe { text(sequence_id, "sequence_id") }?;
        target.try_get(timeline.to_id())?;
        let patches = with(session, |connection| {
            connection
                .session_mut()
                .publish(target, timeline.to_id(), sequence_id)
                .map_err(|error| session_fault(&error))
        })?;
        let json = otio_ext_sync::patch::to_json(&patches);
        unsafe { write_out(out_patches, OtioBuffer::from_str(&json), "out_patches") }
    })
}

/// Moves this client's playhead in `sequence_id` to `frame`, playing at
/// `rate` or stopped.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_set_playhead(
    session: u64,
    sequence_id: *const c_char,
    frame: f64,
    playing: bool,
    rate: f64,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let playhead = Playhead {
            sequence_id: unsafe { text(sequence_id, "sequence_id") }?.to_string(),
            frame,
            playing,
            rate,
        };
        with(session, |connection| {
            connection
                .session_mut()
                .set_playhead(playhead)
                .map_err(|error| session_fault(&error))
        })
    })
}

/// Selects the objects in `sequence_id` whose GUIDs the JSON list `guids`
/// holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_set_selection(
    session: u64,
    sequence_id: *const c_char,
    guids: *const c_char,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let selection = Selection {
            sequence_id: unsafe { text(sequence_id, "sequence_id") }?.to_string(),
            guids: json::guids_from_json(unsafe { text(guids, "guids") }?)
                .map_err(|error| wire_fault(&error))?,
        };
        with(session, |connection| {
            connection
                .session_mut()
                .set_selection(selection)
                .map_err(|error| session_fault(&error))
        })
    })
}

/// Commits a review comment, the JSON object `commit`, to the session's
/// ledger, and writes its id to `out_id`.
///
/// `commit` holds `anchor`, `bodyMarkdown`, `annotationCommands` and
/// `clientMutationId`, which is made up if left out. The commit is sent
/// again after every reconnect until the server acknowledges or refuses it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_commit_review(
    session: u64,
    commit: *const c_char,
    out_id: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let commit = json::review_from_json(unsafe { text(commit, "commit") }?)
            .map_err(|error| wire_fault(&error))?;
        let id = with(session, |connection| {
            connection
                .session_mut()
                .commit_review(commit)
                .map_err(|error| session_fault(&error))
        })?;
        unsafe { write_out(out_id, OtioBuffer::from_str(&id), "out_id") }
    })
}

/// Stops sending the review commit `client_mutation_id`, and writes whether
/// it was waiting to `out_cancelled`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_cancel_review(
    session: u64,
    client_mutation_id: *const c_char,
    out_cancelled: *mut bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let id = unsafe { text(client_mutation_id, "client_mutation_id") }?;
        let cancelled = with(session, |connection| {
            Ok(connection.session_mut().cancel_review(id))
        })?;
        unsafe { write_out(out_cancelled, cancelled, "out_cancelled") }
    })
}

/// Makes `participant` the playback leader. Only the host may.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_set_leader(
    session: u64,
    participant: *const c_char,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let participant = unsafe { text(participant, "participant") }?.to_string();
        with(session, |connection| {
            connection
                .session_mut()
                .set_leader(participant)
                .map_err(|error| session_fault(&error))
        })
    })
}

/// Mutes or unmutes `participant`. Only the host may.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_set_muted(
    session: u64,
    participant: *const c_char,
    muted: bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let participant = unsafe { text(participant, "participant") }?.to_string();
        with(session, |connection| {
            connection
                .session_mut()
                .set_muted(participant, muted)
                .map_err(|error| session_fault(&error))
        })
    })
}

/// Sends a message of type `kind` with the JSON `payload` on `channel`.
///
/// It is for what sync has no call of its own for: on Furnace, `signal`
/// on the signal channel, and the host's `bin`, `lan_mode_set` and
/// `lan_decision` on the control channel. Furnace closes a connection that
/// sends a type it does not know, so anything else is refused with
/// `OTIO_STATUS_UNSUPPORTED` when it is about to be sent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_send(
    session: u64,
    channel: *const c_char,
    kind: *const c_char,
    payload: *const c_char,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let channel = Channel::from_name(unsafe { text(channel, "channel") }?);
        let kind = unsafe { text(kind, "kind") }?.to_string();
        let payload = unsafe { text(payload, "payload") }?.to_string();
        with(session, |connection| {
            connection
                .session_mut()
                .send(channel, kind, payload)
                .map_err(|error| session_fault(&error))
        })
    })
}

/// Registers the channel `name`, for another extension's messages, with the
/// delivery rule `delivery`: `latest_wins`, `ordered` or `transient`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_session_register_channel(
    session: u64,
    name: *const c_char,
    delivery: *const c_char,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let name = unsafe { text(name, "name") }?.to_string();
        let delivery = match unsafe { text(delivery, "delivery") }? {
            "latest_wins" => Delivery::LatestWins,
            "ordered" => Delivery::Ordered,
            "transient" => Delivery::Transient,
            "ledger" => Delivery::Ledger,
            other => {
                return Err(Fault::invalid(format!(
                    "unknown delivery {other:?}: use latest_wins, ordered or transient"
                )));
            }
        };
        with(session, |connection| {
            connection
                .session_mut()
                .register_channel(name, delivery)
                .map_err(|error| session_fault(&error))
        })
    })
}

/// Acts on the JSON `event` from `otio_ext_sync_session_poll_event` in
/// `timeline`, the program's copy of the timeline the event names, and
/// writes what it did to `out_handled` as a JSON object: `handled`, one of
/// `applied`, `replaced`, `rejected`, `joined` or `ignored`, and for
/// `rejected` the script's `message`.
///
/// A `timeline` event runs `sync_before_apply`, whose scripts may refuse
/// it, applies its patches so the timeline and the objects that survive
/// keep their handles, falling back to the whole snapshot if the patches
/// do not fit (`replaced`), runs the default media linker on what it
/// inserted, then `sync_after_apply`. A `joined` event runs `sync_joined`.
/// Every other event is the program's to act on, and is `ignored`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_handle(
    target: *mut OtioDocument,
    timeline: OtioNode,
    event: *const c_char,
    out_handled: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        let event = json::event_from_json(unsafe { text(event, "event") }?)
            .map_err(|error| wire_fault(&error))?;
        target.try_get(timeline.to_id())?;
        let handled = session::handle(
            target,
            timeline.to_id(),
            &event,
            &session::ApplyOptions::default(),
        )
        .map_err(|error| session_fault(&error))?;
        let json = match handled {
            session::Handled::Applied => r#"{"handled":"applied"}"#.to_string(),
            session::Handled::Replaced => r#"{"handled":"replaced"}"#.to_string(),
            session::Handled::Joined => r#"{"handled":"joined"}"#.to_string(),
            session::Handled::Ignored => r#"{"handled":"ignored"}"#.to_string(),
            session::Handled::Rejected(message) => format!(
                r#"{{"handled":"rejected","message":{}}}"#,
                json::quote(&message)
            ),
        };
        unsafe { write_out(out_handled, OtioBuffer::from_str(&json), "out_handled") }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> u64 {
        assert_eq!(
            unsafe { super::super::otio_ext_sync_enable(std::ptr::null_mut()) },
            OtioStatus::Ok
        );
        let mut id = 0;
        let settings = c"{}";
        let status =
            unsafe { otio_ext_sync_session_new(settings.as_ptr(), &mut id, std::ptr::null_mut()) };
        assert_eq!(status, OtioStatus::Ok);
        id
    }

    #[test]
    fn a_session_closed_mid_call_stays_closed() {
        let id = open();
        let closed = with(id, |_| {
            Ok(unsafe { otio_ext_sync_session_close(id, std::ptr::null_mut()) })
        });
        assert!(matches!(closed, Ok(OtioStatus::Ok)));
        assert!(!registry().open.contains_key(&id));
        assert!(!registry().busy.contains_key(&id));
        assert!(with(id, |_| Ok(())).is_err());
    }

    #[test]
    fn a_session_outlives_a_call() {
        let id = open();
        assert!(with(id, |_| Ok(())).is_ok());
        assert!(registry().open.contains_key(&id));
        assert!(!registry().busy.contains_key(&id));
        unsafe { otio_ext_sync_session_close(id, std::ptr::null_mut()) };
    }
}
