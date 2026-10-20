//! Timelines and documents the tests share.

#![allow(dead_code)]

use otio_core::{Document, NodeId};

/// A timeline with one video track holding two clips, with GUIDs.
pub const TIMELINE: &str = r#"{"OTIO_SCHEMA":"Timeline.1","name":"Program","metadata":{"sync":{"guid":"main"}},
  "tracks":{"OTIO_SCHEMA":"Stack.1","name":"tracks","metadata":{"sync":{"guid":"main/stack"}},"children":[
    {"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"main/v1"}},"children":[
      {"OTIO_SCHEMA":"Clip.2","name":"Beach Run","metadata":{"sync":{"guid":"main/clip-1"}},
       "source_range":{"OTIO_SCHEMA":"TimeRange.1",
         "start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},
         "duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}},
      {"OTIO_SCHEMA":"Clip.2","name":"Sunset","metadata":{"sync":{"guid":"main/clip-2"}},
       "source_range":{"OTIO_SCHEMA":"TimeRange.1",
         "start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},
         "duration":{"OTIO_SCHEMA":"RationalTime.1","value":24,"rate":24}}}]}]}}"#;

/// A document holding [`TIMELINE`], and its timeline.
pub fn host_document() -> (Document, NodeId) {
    let document = otio_core::from_str(TIMELINE).expect("the test timeline reads");
    let root = document.root().expect("a root");
    (document, root)
}

/// A document holding an empty timeline, for a viewer to fill.
pub fn viewer_document() -> (Document, NodeId) {
    let document = otio_core::from_str(r#"{"OTIO_SCHEMA":"Timeline.1"}"#).expect("reads");
    let root = document.root().expect("a root");
    (document, root)
}

/// Renames the object with `guid` under `timeline`.
pub fn rename(document: &mut Document, timeline: NodeId, guid: &str, name: &str) {
    let index = otio_ext_sync::guid::GuidIndex::build(document, timeline).expect("unique GUIDs");
    let id = index.get(guid).expect("the object is there");
    document
        .get_mut(id)
        .and_then(otio_core::Node::base_mut)
        .expect("a named object")
        .name = name.to_string();
}

/// The name of the object with `guid` under `timeline`.
pub fn name_of(document: &Document, timeline: NodeId, guid: &str) -> String {
    let index = otio_ext_sync::guid::GuidIndex::build(document, timeline).expect("unique GUIDs");
    let id = index.get(guid).expect("the object is there");
    document
        .get(id)
        .and_then(otio_core::Node::base)
        .expect("named")
        .name
        .clone()
}

/// Turns sync on, once.
pub fn enable_sync() {
    otio_ext_sync::install();
    otio_ext::enable(otio_ext_sync::ID).expect("sync enables");
}
