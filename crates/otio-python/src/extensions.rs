//! Turning on otio-rust's extensions from Python: work built in this
//! repository that is not part of OpenTimelineIO (ADR 0010).
//!
//! These are private functions of `_otio`. The public face is the
//! `opentimelineio.ext` package, which nothing in `opentimelineio` imports, so
//! a program that never asks for an extension never sees one.
//!
//! Timeline sync's calls, `_sync_*`, are `opentimelineio.ext.sync`'s. They
//! do what the C interface's `otio_ext_sync_*` do, on `otio_ext_sync`
//! directly, and take OTIO objects where it takes a document and a handle:
//! the document is the one the object lives in, as for every other function
//! here that takes an object.
//!
//! ORI Annotations' calls, `_ori_*`, are `opentimelineio.ext.ori`'s, and do
//! what `otio_ext_ori_*` do in the same way. Commands cross as JSON text.

use otio_core::NodeId;
use otio_ext_sync::document::{self as sync_document, Transaction};
use otio_ext_sync::session::{self, Channel, Delivery, Playhead, Selection, Session};
use otio_ext_sync::{Patch, Snapshot, guid};
use otio_ext_sync_transport::{Connection, Frame, Furnace, json};

use pyo3::exceptions::{PyConnectionError, PyNotImplementedError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::{Py, PyAny};

use crate::arena::Shared;
use crate::objects::{Handle, core_error, handle_of, wrap};
use crate::opentime::PyRationalTime;

/// Installs every extension these bindings are built with.
fn install() {
    otio_ext_sync::install();
    otio_ext_ori_annotations::install();
}

/// Turns on the extension `id`, and every extension it depends on first.
#[pyfunction]
fn _ext_enable(id: &str) -> PyResult<()> {
    install();
    otio_ext::enable(id).map_err(|error| match error {
        otio_ext::Error::Register { .. } => PyRuntimeError::new_err(error.to_string()),
        _ => PyValueError::new_err(error.to_string()),
    })
}

/// Whether the extension `id` is on.
#[pyfunction]
fn _ext_is_enabled(id: &str) -> bool {
    otio_ext::is_enabled(id)
}

/// The ids of the extensions that are on, in the order they were enabled.
#[pyfunction]
fn _ext_enabled() -> Vec<&'static str> {
    otio_ext::enabled()
}

/// The ids of every extension these bindings are built with.
#[pyfunction]
fn _ext_available() -> Vec<&'static str> {
    install();
    otio_ext::available()
}

/// Refuses every sync call but turning it on until sync is enabled, as the
/// C interface does.
fn sync_enabled() -> PyResult<()> {
    if otio_ext::is_enabled(otio_ext_sync::ID) {
        Ok(())
    } else {
        Err(PyRuntimeError::new_err(
            "timeline sync is not enabled: call opentimelineio.ext.sync.enable() first",
        ))
    }
}

/// Turns a sync failure into the exception the C interface's status maps
/// to: text that is not JSON or not patches, and a bad argument, are a
/// `ValueError`; a document that cannot be written or read back is a
/// `RuntimeError`.
fn sync_error(error: &otio_ext_sync::Error) -> PyErr {
    match error {
        otio_ext_sync::Error::Document(_) => PyRuntimeError::new_err(error.to_string()),
        _ => PyValueError::new_err(error.to_string()),
    }
}

fn snapshot_from(json: &str) -> PyResult<Snapshot> {
    Snapshot::from_json(json).map_err(|error| sync_error(&error))
}

fn patches_from(json: &str) -> PyResult<Vec<Patch>> {
    otio_ext_sync::patch::from_json(json).map_err(|error| sync_error(&error))
}

/// The document `object` lives in, and its node there.
fn located(object: &Bound<'_, PyAny>) -> PyResult<(Shared, NodeId)> {
    handle_of(object)?.live()
}

/// A new GUID, a version 4 UUID.
#[pyfunction]
fn _sync_new_guid() -> PyResult<String> {
    sync_enabled()?;
    Ok(guid::new_guid())
}

/// The GUID `node` carries in `metadata["sync"]["guid"]`, or `None`.
#[pyfunction]
fn _sync_guid(node: &Bound<'_, PyAny>) -> PyResult<Option<String>> {
    sync_enabled()?;
    let (shared, id) = located(node)?;
    shared.read(|document| {
        core_error(document.try_get(id))?;
        Ok(guid::guid(document, id).map(str::to_owned))
    })
}

/// Gives `node` the GUID `value`, replacing any it had.
#[pyfunction]
fn _sync_set_guid(node: &Bound<'_, PyAny>, value: &str) -> PyResult<()> {
    sync_enabled()?;
    let (shared, id) = located(node)?;
    shared.write(|document| core_error(guid::set_guid(document, id, value)))
}

/// Gives every object sync addresses under `timeline` a GUID it lacks, and
/// returns how many it gave one to.
#[pyfunction]
fn _sync_stamp(timeline: &Bound<'_, PyAny>) -> PyResult<usize> {
    sync_enabled()?;
    let (shared, id) = located(timeline)?;
    shared.write(|document| Ok(core_error(guid::stamp(document, id))?.len()))
}

/// The object under `timeline` carrying the GUID `value`, or `None`.
#[pyfunction]
fn _sync_find(
    py: Python<'_>,
    timeline: &Bound<'_, PyAny>,
    value: &str,
) -> PyResult<Option<Py<PyAny>>> {
    sync_enabled()?;
    let (shared, id) = located(timeline)?;
    let found = shared.read(|document| {
        core_error(document.try_get(id))?;
        let index = guid::GuidIndex::build(document, id)
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        Ok(index.get(value))
    })?;
    found
        .map(|id| {
            Ok(wrap(
                py,
                &Handle {
                    shared: shared.clone(),
                    id,
                },
            )?
            .unbind())
        })
        .transpose()
}

/// The patches that take one snapshot to another, as a JSON list.
#[pyfunction]
fn _sync_diff(old_snapshot: &str, new_snapshot: &str, pretty: bool) -> PyResult<String> {
    sync_enabled()?;
    let old = snapshot_from(old_snapshot)?;
    let new = snapshot_from(new_snapshot)?;
    let (patches, _) = otio_ext_sync::diff(&old, &new).map_err(|error| sync_error(&error))?;
    Ok(if pretty {
        otio_ext_sync::patch::to_json_pretty(&patches)
    } else {
        otio_ext_sync::patch::to_json(&patches)
    })
}

/// What a diff of the same snapshots does, as a JSON object.
#[pyfunction]
fn _sync_diff_stats(old_snapshot: &str, new_snapshot: &str) -> PyResult<String> {
    sync_enabled()?;
    let old = snapshot_from(old_snapshot)?;
    let new = snapshot_from(new_snapshot)?;
    let (_, stats) = otio_ext_sync::diff(&old, &new).map_err(|error| sync_error(&error))?;
    Ok(stats.to_json())
}

/// The timeline JSON `timeline` with the patches `patches` applied, as
/// compact JSON.
#[pyfunction]
fn _sync_apply(timeline: &str, patches: &str) -> PyResult<String> {
    sync_enabled()?;
    let patches = patches_from(patches)?;
    otio_ext_sync::apply(timeline, &patches).map_err(|error| sync_error(&error))
}

/// Applies `patches` to `timeline` in place.
///
/// What the patches remove leaves the document, so the wrappers held for it
/// are forgotten; see [`Shared::forget_removed`].
#[pyfunction]
fn _sync_apply_to(py: Python<'_>, timeline: &Bound<'_, PyAny>, patches: &str) -> PyResult<()> {
    sync_enabled()?;
    let patches = patches_from(patches)?;
    let (shared, id) = located(timeline)?;
    shared.write(|document| {
        sync_document::apply_to(document, id, &patches).map_err(|error| sync_error(&error))
    })?;
    shared.forget_removed(py)
}

/// Starts a transaction on `timeline`, returning the snapshot that is the
/// transaction.
#[pyfunction]
fn _sync_begin(
    timeline: &Bound<'_, PyAny>,
    sequence_id: &str,
    generation: i64,
) -> PyResult<String> {
    sync_enabled()?;
    let (shared, id) = located(timeline)?;
    shared.write(|document| {
        let transaction = Transaction::begin(document, id, sequence_id, generation)
            .map_err(|error| sync_error(&error))?;
        Ok(transaction.before().to_json())
    })
}

/// Ends the transaction `before`, returning its patches as a compact JSON
/// list.
#[pyfunction]
fn _sync_commit(timeline: &Bound<'_, PyAny>, before: &str) -> PyResult<String> {
    sync_enabled()?;
    let before = snapshot_from(before)?;
    let (shared, id) = located(timeline)?;
    let patches = shared.write(|document| {
        Transaction::resume(id, before)
            .commit(document)
            .map_err(|error| sync_error(&error))
    })?;
    Ok(otio_ext_sync::patch::to_json(&patches))
}

/// Ends the transaction `before`, putting the timeline back as it was.
#[pyfunction]
fn _sync_rollback(py: Python<'_>, timeline: &Bound<'_, PyAny>, before: &str) -> PyResult<()> {
    sync_enabled()?;
    let before = snapshot_from(before)?;
    let (shared, id) = located(timeline)?;
    shared.write(|document| {
        Transaction::resume(id, before)
            .rollback(document)
            .map_err(|error| sync_error(&error))
    })?;
    shared.forget_removed(py)
}

/// Starts the journal of the document `timeline` is in.
#[pyfunction]
fn _sync_start_journal(timeline: &Bound<'_, PyAny>) -> PyResult<()> {
    sync_enabled()?;
    let (shared, id) = located(timeline)?;
    shared.write(|document| {
        core_error(document.try_get(id))?;
        if !document.is_journalling() {
            document.start_journal();
        }
        Ok(())
    })
}

/// Stops the journal of the document `timeline` is in.
#[pyfunction]
fn _sync_stop_journal(timeline: &Bound<'_, PyAny>) -> PyResult<()> {
    sync_enabled()?;
    let (shared, id) = located(timeline)?;
    shared.write(|document| {
        core_error(document.try_get(id))?;
        document.stop_journal();
        Ok(())
    })
}

/// Commits the journal of the document `timeline` is in, running the
/// `document_changed` hook, and returns whether anything changed.
#[pyfunction]
fn _sync_commit_journal(timeline: &Bound<'_, PyAny>) -> PyResult<bool> {
    sync_enabled()?;
    let (shared, id) = located(timeline)?;
    shared.write(|document| {
        core_error(document.try_get(id))?;
        let changes = otio_ext_sync::journal::commit(document)
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        Ok(!changes.is_empty())
    })
}

/// Turns a session's refusal into the exception the C interface's status
/// maps to: a hook or script is a `RuntimeError`, a copy of the timeline
/// that fell out of sync a `ConnectionError` (reconnect), anything else a
/// `ValueError`.
fn session_error(error: &session::Error) -> PyErr {
    match error {
        session::Error::Patch(error) => sync_error(error),
        session::Error::Plugin(_) | session::Error::Vetoed(_) => {
            PyRuntimeError::new_err(error.to_string())
        }
        session::Error::OutOfSync(_) => PyConnectionError::new_err(error.to_string()),
        _ => PyValueError::new_err(error.to_string()),
    }
}

/// Turns a wire failure into an exception: JSON that is not what was
/// expected is a `ValueError`, a message the wire cannot carry a
/// `NotImplementedError`, and a broken connection a `ConnectionError`.
fn wire_error(error: &otio_ext_sync_transport::Error) -> PyErr {
    use otio_ext_sync_transport::Error;
    match error {
        Error::Json(_) => PyValueError::new_err(error.to_string()),
        Error::Unsupported(_) => PyNotImplementedError::new_err(error.to_string()),
        Error::Session(error) => session_error(error),
        _ => PyConnectionError::new_err(error.to_string()),
    }
}

/// A sync session on Furnace's wire: `opentimelineio.ext.sync.Session`'s
/// engine. Settings, events and state cross as JSON text, as they do over
/// the C interface; the Python class turns them into dictionaries.
#[pyclass(module = "opentimelineio._otio", name = "_SyncSession")]
pub struct SyncSession {
    connection: Connection<Furnace>,
}

#[pymethods]
impl SyncSession {
    #[new]
    fn new(settings: &str) -> PyResult<Self> {
        sync_enabled()?;
        let read = json::settings_from_json(settings).map_err(|error| wire_error(&error))?;
        Ok(Self {
            connection: Connection::new(
                Session::new(read.config),
                Furnace::with_encodings(read.encodings),
            ),
        })
    }

    fn connect(&mut self) {
        self.connection.connect();
    }

    fn receive(&mut self, data: &[u8], binary: bool) -> PyResult<()> {
        let frame = Frame {
            data: data.to_vec(),
            binary,
        };
        self.connection
            .receive(&frame)
            .map_err(|error| wire_error(&error))
    }

    fn poll_transmit(&mut self, now: i64) -> PyResult<Option<(Vec<u8>, bool)>> {
        let frame = self
            .connection
            .poll_transmit(now)
            .map_err(|error| wire_error(&error))?;
        Ok(frame.map(|frame| (frame.data, frame.binary)))
    }

    fn poll_event(&mut self) -> Option<String> {
        self.connection
            .poll_event()
            .map(|event| json::event_to_json(&event))
    }

    fn disconnected(&mut self) {
        self.connection.disconnected();
    }

    fn leave(&mut self) {
        self.connection.session_mut().leave();
    }

    fn state(&self) -> String {
        json::state_to_json(
            self.connection.session(),
            self.connection.profile().version(),
            self.connection.profile().accepted(),
        )
    }

    fn timeline(&self, sequence_id: &str) -> Option<String> {
        self.connection
            .session()
            .timeline(sequence_id)
            .map(Snapshot::to_json)
    }

    fn may_send(&self, channel: &str) -> bool {
        self.connection
            .session()
            .may_send(&Channel::from_name(channel))
    }

    fn publish(&mut self, timeline: &Bound<'_, PyAny>, sequence_id: &str) -> PyResult<String> {
        let (shared, id) = located(timeline)?;
        let session = self.connection.session_mut();
        let patches = shared.write(|document| {
            core_error(document.try_get(id))?;
            session
                .publish(document, id, sequence_id)
                .map_err(|error| session_error(&error))
        })?;
        Ok(otio_ext_sync::patch::to_json(&patches))
    }

    fn set_playhead(
        &mut self,
        sequence_id: &str,
        frame: f64,
        playing: bool,
        rate: f64,
    ) -> PyResult<()> {
        self.connection
            .session_mut()
            .set_playhead(Playhead {
                sequence_id: sequence_id.to_string(),
                frame,
                playing,
                rate,
            })
            .map_err(|error| session_error(&error))
    }

    fn set_selection(&mut self, sequence_id: &str, guids: Vec<String>) -> PyResult<()> {
        self.connection
            .session_mut()
            .set_selection(Selection {
                sequence_id: sequence_id.to_string(),
                guids,
            })
            .map_err(|error| session_error(&error))
    }

    fn commit_review(&mut self, commit: &str) -> PyResult<String> {
        let commit = json::review_from_json(commit).map_err(|error| wire_error(&error))?;
        self.connection
            .session_mut()
            .commit_review(commit)
            .map_err(|error| session_error(&error))
    }

    fn cancel_review(&mut self, client_mutation_id: &str) -> bool {
        self.connection
            .session_mut()
            .cancel_review(client_mutation_id)
    }

    fn set_leader(&mut self, participant: &str) -> PyResult<()> {
        self.connection
            .session_mut()
            .set_leader(participant)
            .map_err(|error| session_error(&error))
    }

    fn set_muted(&mut self, participant: &str, muted: bool) -> PyResult<()> {
        self.connection
            .session_mut()
            .set_muted(participant, muted)
            .map_err(|error| session_error(&error))
    }

    fn send(&mut self, channel: &str, kind: &str, payload: &str) -> PyResult<()> {
        self.connection
            .session_mut()
            .send(Channel::from_name(channel), kind, payload)
            .map_err(|error| session_error(&error))
    }

    fn register_channel(&mut self, name: &str, delivery: &str) -> PyResult<()> {
        let delivery = match delivery {
            "latest_wins" => Delivery::LatestWins,
            "ordered" => Delivery::Ordered,
            "transient" => Delivery::Transient,
            "ledger" => Delivery::Ledger,
            other => {
                return Err(PyValueError::new_err(format!(
                    "unknown delivery {other:?}: use latest_wins, ordered or transient"
                )));
            }
        };
        self.connection
            .session_mut()
            .register_channel(name, delivery)
            .map_err(|error| session_error(&error))
    }
}

/// Acts on the JSON `event` in `timeline`, returning what it did as JSON,
/// as `otio_ext_sync_handle` does.
#[pyfunction]
fn _sync_handle(py: Python<'_>, timeline: &Bound<'_, PyAny>, event: &str) -> PyResult<String> {
    sync_enabled()?;
    let event = json::event_from_json(event).map_err(|error| wire_error(&error))?;
    let (shared, id) = located(timeline)?;
    let handled = shared.write(|document| {
        core_error(document.try_get(id))?;
        session::handle(document, id, &event, &session::ApplyOptions::default())
            .map_err(|error| session_error(&error))
    })?;
    shared.forget_removed(py)?;
    Ok(match handled {
        session::Handled::Applied => r#"{"handled":"applied"}"#.to_string(),
        session::Handled::Replaced => r#"{"handled":"replaced"}"#.to_string(),
        session::Handled::Joined => r#"{"handled":"joined"}"#.to_string(),
        session::Handled::Ignored => r#"{"handled":"ignored"}"#.to_string(),
        session::Handled::Rejected(message) => format!(
            r#"{{"handled":"rejected","message":{}}}"#,
            json::quote(&message)
        ),
    })
}

/// Refuses every ORI Annotations call but turning it on until it is
/// enabled, as the C interface does.
fn ori_enabled() -> PyResult<()> {
    if otio_ext_ori_annotations::is_enabled() {
        Ok(())
    } else {
        Err(PyRuntimeError::new_err(
            "ORI Annotations is not enabled: call opentimelineio.ext.ori.enable() first",
        ))
    }
}

/// Turns an ORI Annotations failure into the exception its C status maps
/// to: commands that are not JSON and objects of the wrong kind are a
/// `ValueError` and a `TypeError`, and what otio-core refuses is its own.
fn ori_error(error: otio_ext_ori_annotations::Error) -> PyErr {
    use otio_ext_ori_annotations::Error;
    match error {
        Error::Core(error) => core_error::<()>(Err(error)).unwrap_err(),
        Error::WrongKind { .. } => pyo3::exceptions::PyTypeError::new_err(error.to_string()),
        Error::NotEnabled => PyRuntimeError::new_err(error.to_string()),
        Error::NotPermitted { .. } => {
            pyo3::exceptions::PyPermissionError::new_err(error.to_string())
        }
        Error::Session(error) => session_error(&error),
        _ => PyValueError::new_err(error.to_string()),
    }
}

fn ori_commands(json: &str) -> PyResult<Vec<otio_ext_ori_annotations::command::Command>> {
    otio_ext_ori_annotations::command::from_json(json).map_err(ori_error)
}

/// Wraps an object of the document `shared`.
fn wrap_in(py: Python<'_>, shared: &Shared, id: NodeId) -> PyResult<Py<PyAny>> {
    Ok(wrap(
        py,
        &Handle {
            shared: shared.clone(),
            id,
        },
    )?
    .unbind())
}

/// The annotation track of `timeline`, or `None`.
#[pyfunction]
fn _ori_find_track(py: Python<'_>, timeline: &Bound<'_, PyAny>) -> PyResult<Option<Py<PyAny>>> {
    use otio_ext_ori_annotations::annotations;
    ori_enabled()?;
    let (shared, id) = located(timeline)?;
    let found = shared.read(|document| annotations::find_track(document, id).map_err(ori_error))?;
    found.map(|track| wrap_in(py, &shared, track)).transpose()
}

/// The annotation track of `timeline`, added if it has none.
#[pyfunction]
fn _ori_ensure_track(py: Python<'_>, timeline: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    use otio_ext_ori_annotations::annotations;
    ori_enabled()?;
    let (shared, id) = located(timeline)?;
    let track =
        shared.write(|document| annotations::ensure_track(document, id).map_err(ori_error))?;
    wrap_in(py, &shared, track)
}

/// The annotation clip on `track` for `clip_guid` at `frame`, or `None`.
#[pyfunction]
fn _ori_find_clip(
    py: Python<'_>,
    track: &Bound<'_, PyAny>,
    clip_guid: &str,
    frame: i64,
) -> PyResult<Option<Py<PyAny>>> {
    use otio_ext_ori_annotations::annotations;
    ori_enabled()?;
    let (shared, id) = located(track)?;
    let found = shared.read(|document| {
        annotations::find_clip(document, id, clip_guid, frame).map_err(ori_error)
    })?;
    found.map(|clip| wrap_in(py, &shared, clip)).transpose()
}

/// The annotation clip on `track` for `clip_guid` at `time`, added if
/// there is none.
#[pyfunction]
fn _ori_ensure_clip(
    py: Python<'_>,
    track: &Bound<'_, PyAny>,
    clip_guid: &str,
    time: PyRationalTime,
) -> PyResult<Py<PyAny>> {
    use otio_ext_ori_annotations::annotations;
    ori_enabled()?;
    let (shared, id) = located(track)?;
    let clip = shared.write(|document| {
        annotations::ensure_clip(document, id, clip_guid, time.0).map_err(ori_error)
    })?;
    wrap_in(py, &shared, clip)
}

/// The objects holding the annotations on `timeline`: all of them, those
/// of `clip_guid`, or those of `clip_guid` at `frame`.
#[pyfunction]
#[pyo3(signature = (timeline, clip_guid = None, frame = None))]
fn _ori_list(
    py: Python<'_>,
    timeline: &Bound<'_, PyAny>,
    clip_guid: Option<&str>,
    frame: Option<i64>,
) -> PyResult<Vec<Py<PyAny>>> {
    use otio_ext_ori_annotations::annotations;
    ori_enabled()?;
    let (shared, id) = located(timeline)?;
    let found = shared.read(|document| {
        match (clip_guid, frame) {
            (Some(guid), Some(frame)) => annotations::at_frame(document, id, guid, frame),
            (Some(guid), None) => annotations::for_clip(document, id, guid),
            (None, None) => annotations::list(document, id),
            (None, Some(_)) => {
                return Err(PyValueError::new_err("a frame needs a clip_guid"));
            }
        }
        .map_err(ori_error)
    })?;
    found
        .iter()
        .map(|annotation| wrap_in(py, &shared, annotation.holder))
        .collect()
}

/// What `holder` says about its annotation, as JSON.
#[pyfunction]
fn _ori_describe(holder: &Bound<'_, PyAny>) -> PyResult<String> {
    use otio_ext_ori_annotations::annotations;
    ori_enabled()?;
    let (shared, id) = located(holder)?;
    shared.read(|document| {
        let annotation = annotations::describe(document, id).map_err(ori_error)?;
        Ok(annotation.to_json(document))
    })
}

/// The commands `holder` holds, as a JSON list.
#[pyfunction]
fn _ori_commands(holder: &Bound<'_, PyAny>, pretty: bool) -> PyResult<String> {
    use otio_ext_ori_annotations::{annotations, command};
    ori_enabled()?;
    let (shared, id) = located(holder)?;
    shared.read(|document| {
        let ids = annotations::commands(document, id).map_err(ori_error)?;
        command::to_json(document, &ids, pretty.then_some(otio_core::DEFAULT_INDENT))
            .map_err(ori_error)
    })
}

/// Adds the commands in `commands` at the end of `clip`'s.
#[pyfunction]
fn _ori_add_commands(clip: &Bound<'_, PyAny>, commands: &str) -> PyResult<()> {
    use otio_ext_ori_annotations::annotations;
    ori_enabled()?;
    let commands = ori_commands(commands)?;
    let (shared, id) = located(clip)?;
    shared.write(|document| {
        annotations::add_commands(document, id, &commands)
            .map(drop)
            .map_err(ori_error)
    })
}

/// Replaces every command of `clip` with the uuid `uuid`, returning whether
/// any had it.
#[pyfunction]
fn _ori_replace_commands(clip: &Bound<'_, PyAny>, uuid: &str, commands: &str) -> PyResult<bool> {
    use otio_ext_ori_annotations::annotations;
    ori_enabled()?;
    let commands = ori_commands(commands)?;
    let (shared, id) = located(clip)?;
    shared.write(|document| {
        annotations::replace_commands(document, id, uuid, &commands).map_err(ori_error)
    })
}

/// Removes every command of `clip` whose uuid is in `uuids`, returning how
/// many it removed.
#[pyfunction]
fn _ori_remove_commands(clip: &Bound<'_, PyAny>, uuids: Vec<String>) -> PyResult<usize> {
    use otio_ext_ori_annotations::annotations;
    ori_enabled()?;
    let uuids: Vec<&str> = uuids.iter().map(String::as_str).collect();
    let (shared, id) = located(clip)?;
    shared.write(|document| annotations::remove_commands(document, id, &uuids).map_err(ori_error))
}

/// Replaces all of `clip`'s commands with `commands`.
#[pyfunction]
fn _ori_set_commands(clip: &Bound<'_, PyAny>, commands: &str) -> PyResult<()> {
    use otio_ext_ori_annotations::annotations;
    ori_enabled()?;
    let commands = ori_commands(commands)?;
    let (shared, id) = located(clip)?;
    shared.write(|document| annotations::set_commands(document, id, &commands).map_err(ori_error))
}

/// Merges `commands` into `clip`'s by uuid: a uuid it has is replaced where
/// it stands, a new one goes at the end. Returns how many uuids were
/// replaced and how many added.
#[pyfunction]
fn _ori_merge_commands(clip: &Bound<'_, PyAny>, commands: &str) -> PyResult<(usize, usize)> {
    use otio_ext_ori_annotations::annotations;
    ori_enabled()?;
    let commands = ori_commands(commands)?;
    let (shared, id) = located(clip)?;
    let merged = shared.write(|document| {
        annotations::merge_commands(document, id, &commands).map_err(ori_error)
    })?;
    Ok((merged.replaced, merged.added))
}

/// Replaces `clip`'s commands of each kind `commands` has (strokes, text)
/// with them, keeping the kinds it lacks; an empty list clears them all.
/// Returns how many commands it removed.
#[pyfunction]
fn _ori_replace_by_kind(clip: &Bound<'_, PyAny>, commands: &str) -> PyResult<usize> {
    use otio_ext_ori_annotations::annotations;
    ori_enabled()?;
    let commands = ori_commands(commands)?;
    let (shared, id) = located(clip)?;
    shared
        .write(|document| annotations::replace_by_kind(document, id, &commands).map_err(ori_error))
}

/// Folds every second annotation clip for one source frame on `timeline`
/// into the first, returning how many it removed.
#[pyfunction]
fn _ori_merge_duplicates(py: Python<'_>, timeline: &Bound<'_, PyAny>) -> PyResult<usize> {
    use otio_ext_ori_annotations::annotations;
    ori_enabled()?;
    let (shared, id) = located(timeline)?;
    let removed =
        shared.write(|document| annotations::merge_duplicates(document, id).map_err(ori_error))?;
    shared.forget_removed(py)?;
    Ok(removed)
}

/// The user whose review comment added the command `uuid` to `clip`, or
/// `None`.
#[pyfunction]
fn _ori_owner_of(clip: &Bound<'_, PyAny>, uuid: &str) -> PyResult<Option<String>> {
    ori_enabled()?;
    let (shared, id) = located(clip)?;
    shared.read(|document| {
        otio_ext_ori_annotations::review::owner_of(document, id, uuid).map_err(ori_error)
    })
}

/// The stroke in progress the session event `event` carries, as JSON, or
/// `None` when it carries none.
#[pyfunction]
fn _ori_read_partial(event: &str) -> PyResult<Option<String>> {
    ori_enabled()?;
    let event = json::event_from_json(event).map_err(|error| wire_error(&error))?;
    otio_ext_ori_annotations::review::read_partial(&event)
        .map_err(ori_error)?
        .map(|partial| partial.to_json().map_err(ori_error))
        .transpose()
}

/// Folds the review comment the session event `event` (a `review_relay`)
/// carries into `timeline`, returning its annotation clip and whether the
/// comment was already there.
#[pyfunction]
fn _ori_fold(
    py: Python<'_>,
    timeline: &Bound<'_, PyAny>,
    event: &str,
) -> PyResult<(Py<PyAny>, bool)> {
    ori_enabled()?;
    let otio_ext_sync::session::Event::ReviewRelay(relay) =
        json::event_from_json(event).map_err(|error| wire_error(&error))?
    else {
        return Err(PyValueError::new_err("the event is not a review_relay"));
    };
    let (shared, id) = located(timeline)?;
    let folded = shared.write(|document| {
        otio_ext_ori_annotations::review::fold(document, id, &relay).map_err(ori_error)
    })?;
    Ok((wrap_in(py, &shared, folded.clip)?, folded.already))
}

/// Sends the stroke in progress `partial` (JSON) to `session`'s peers.
#[pyfunction]
fn _ori_send_partial(mut session: PyRefMut<'_, SyncSession>, partial: &str) -> PyResult<()> {
    use otio_ext_ori_annotations::review;
    ori_enabled()?;
    let partial = review::Partial::from_json(partial).map_err(ori_error)?;
    review::send_partial(session.connection.session_mut(), &partial).map_err(ori_error)
}

/// Commits `commands` with the note `body_markdown`, drawn at `anchor`
/// (JSON) on `timeline`, as a review comment on `session`. Returns the
/// commit's client mutation id, made up when `client_mutation_id` is empty.
#[pyfunction]
fn _ori_commit(
    mut session: PyRefMut<'_, SyncSession>,
    timeline: &Bound<'_, PyAny>,
    anchor: &str,
    body_markdown: &str,
    commands: &str,
    client_mutation_id: &str,
) -> PyResult<String> {
    use otio_ext_ori_annotations::review;
    ori_enabled()?;
    let anchor = review::Anchor::from_json(anchor).map_err(ori_error)?;
    let commands = ori_commands(commands)?;
    let (shared, id) = located(timeline)?;
    let session = session.connection.session_mut();
    shared.read(|document| {
        review::commit(
            session,
            document,
            id,
            &anchor,
            body_markdown,
            &commands,
            client_mutation_id,
        )
        .map_err(ori_error)
    })
}

/// The pixel position `px`, `py` of an image `width` by `height`, in ORI's
/// coordinates.
#[pyfunction]
fn _ori_pixels_to_ori(width: f64, height: f64, px: f64, py: f64) -> PyResult<(f64, f64)> {
    ori_enabled()?;
    Ok(otio_ext_ori_annotations::coords::ImageSize::new(width, height).pixels_to_ori(px, py))
}

/// The point `x`, `y` in ORI's coordinates, as a pixel position of an
/// image `width` by `height`.
#[pyfunction]
fn _ori_ori_to_pixels(width: f64, height: f64, x: f64, y: f64) -> PyResult<(f64, f64)> {
    ori_enabled()?;
    Ok(otio_ext_ori_annotations::coords::ImageSize::new(width, height).ori_to_pixels(x, y))
}

/// Half the width of an image `width` by `height`, in ORI's units.
#[pyfunction]
fn _ori_aspect_half(width: f64, height: f64) -> PyResult<f64> {
    ori_enabled()?;
    Ok(otio_ext_ori_annotations::coords::aspect_half(width, height))
}

fn ori_bounds(bounds: (f64, f64, f64, f64)) -> otio_core::Box2d {
    let (min_x, min_y, max_x, max_y) = bounds;
    otio_core::Box2d::new(
        otio_core::V2d::new(min_x, min_y),
        otio_core::V2d::new(max_x, max_y),
    )
}

/// The point `x`, `y` in ORI's coordinates, in OpenTimelineIO image space
/// within the bounds `(min_x, min_y, max_x, max_y)`.
#[pyfunction]
fn _ori_ori_to_bounds(x: f64, y: f64, bounds: (f64, f64, f64, f64)) -> PyResult<(f64, f64)> {
    ori_enabled()?;
    let point = otio_ext_ori_annotations::coords::ori_to_bounds(x, y, ori_bounds(bounds));
    Ok((point.x, point.y))
}

/// The point `x`, `y` in OpenTimelineIO image space within the bounds
/// `(min_x, min_y, max_x, max_y)`, in ORI's coordinates.
#[pyfunction]
fn _ori_bounds_to_ori(x: f64, y: f64, bounds: (f64, f64, f64, f64)) -> PyResult<(f64, f64)> {
    ori_enabled()?;
    Ok(otio_ext_ori_annotations::coords::bounds_to_ori(
        otio_core::V2d::new(x, y),
        ori_bounds(bounds),
    ))
}

/// The current time in ISO 8601, for a command's `timestamp`.
#[pyfunction]
fn _ori_timestamp_now() -> PyResult<String> {
    ori_enabled()?;
    Ok(otio_ext_ori_annotations::command::timestamp_now())
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(_ext_enable, module)?)?;
    module.add_function(wrap_pyfunction!(_ext_is_enabled, module)?)?;
    module.add_function(wrap_pyfunction!(_ext_enabled, module)?)?;
    module.add_function(wrap_pyfunction!(_ext_available, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_find_track, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_ensure_track, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_find_clip, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_ensure_clip, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_list, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_describe, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_commands, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_add_commands, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_replace_commands, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_remove_commands, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_set_commands, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_timestamp_now, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_merge_commands, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_replace_by_kind, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_merge_duplicates, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_owner_of, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_read_partial, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_fold, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_send_partial, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_commit, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_pixels_to_ori, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_ori_to_pixels, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_aspect_half, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_ori_to_bounds, module)?)?;
    module.add_function(wrap_pyfunction!(_ori_bounds_to_ori, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_new_guid, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_guid, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_set_guid, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_stamp, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_find, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_diff, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_diff_stats, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_apply, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_apply_to, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_begin, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_commit, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_rollback, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_start_journal, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_stop_journal, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_commit_journal, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_handle, module)?)?;
    module.add_class::<SyncSession>()?;
    Ok(())
}
