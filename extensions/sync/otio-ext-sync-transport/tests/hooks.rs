//! The session's hooks and the media linker, run around a remote change.
//! One test, since hooks are registered process-wide.

mod common;

use std::sync::Mutex;

use otio_adapter::plugins::{self, HookScript, LinkerChoice, MediaLinker};
use otio_core::Any;
use otio_ext_sync::session::{
    self, ApplyOptions, Config, Event, Handled, SYNC_AFTER_APPLY, SYNC_BEFORE_APPLY,
    SYNC_BEFORE_SEND, SYNC_JOINED, Session,
};
use otio_ext_sync_transport::loopback::{Room, pump};
use otio_ext_sync_transport::{Connection, Furnace};

static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());
static REFUSE: Mutex<Option<&'static str>> = Mutex::new(None);

fn log(entry: String) {
    LOG.lock().expect("unpoisoned").push(entry);
}

fn take_log() -> Vec<String> {
    std::mem::take(&mut *LOG.lock().expect("unpoisoned"))
}

fn recorder(hook: &'static str) -> HookScript {
    HookScript::new(move |_, target, arguments| {
        let patches = match arguments.get("patches") {
            Some(Any::String(patches)) => patches.len(),
            _ => 0,
        };
        log(format!("{hook} patches={}", usize::from(patches > 2)));
        if *REFUSE.lock().expect("unpoisoned") == Some(hook) {
            return Err(format!("{hook} says no"));
        }
        Ok(target)
    })
}

#[test]
fn hooks_and_the_linker_run_around_changes() {
    common::enable_sync();
    {
        let mut registry = plugins::registry();
        for hook in [
            SYNC_BEFORE_SEND,
            SYNC_BEFORE_APPLY,
            SYNC_AFTER_APPLY,
            SYNC_JOINED,
        ] {
            assert!(registry.has_hook(hook), "sync declares {hook}");
            registry.register_hook_script(hook, recorder(hook));
            registry.attach_hook_script(hook, hook);
        }
        registry.register_media_linker(
            "tag",
            MediaLinker::new(|document, clip, _| {
                let name = document
                    .get(clip)
                    .and_then(otio_core::Node::base)
                    .map(|b| b.name.clone());
                log(format!("linked {}", name.unwrap_or_default()));
                Ok(None)
            }),
        );
    }
    let options = ApplyOptions {
        linker: LinkerChoice::Named("tag".to_string()),
        ..ApplyOptions::default()
    };

    let mut room = Room::new();
    let config = |token: &str| Config {
        token: token.to_string(),
        ..Config::default()
    };
    let mut host = Connection::new(Session::new(config(Room::HOST_TOKEN)), Furnace::new());
    let host_id = room.open(Room::HOST_TOKEN);
    host.connect();
    pump(&mut room, &mut [(host_id, &mut host)], 0).expect("joins");
    let (mut host_document, host_timeline) = common::host_document();

    // A failing sync_before_send stops the change; nothing is sent.
    *REFUSE.lock().expect("unpoisoned") = Some(SYNC_BEFORE_SEND);
    let refused = host
        .session_mut()
        .publish(&mut host_document, host_timeline, "main");
    assert_eq!(
        refused,
        Err(session::Error::Vetoed(
            "sync_before_send says no".to_string()
        ))
    );
    assert!(host.session().timeline("main").is_none());
    *REFUSE.lock().expect("unpoisoned") = None;
    host.session_mut()
        .publish(&mut host_document, host_timeline, "main")
        .expect("publishes");
    pump(&mut room, &mut [(host_id, &mut host)], 0).expect("sent");
    assert_eq!(
        take_log(),
        ["sync_before_send patches=1", "sync_before_send patches=1"]
    );

    // Joining links every clip, then runs the apply hooks and sync_joined.
    let mut viewer = Connection::new(Session::new(config(Room::JOIN_TOKEN)), Furnace::new());
    let viewer_id = room.open(Room::JOIN_TOKEN);
    viewer.connect();
    pump(&mut room, &mut [(viewer_id, &mut viewer)], 0).expect("joins");
    let (mut document, timeline) = common::viewer_document();
    while let Some(event) = viewer.poll_event() {
        session::handle(&mut document, timeline, &event, &options).expect("handles");
    }
    assert_eq!(
        take_log(),
        [
            "sync_before_apply patches=1",
            "linked Beach Run",
            "linked Sunset",
            "sync_after_apply patches=1",
            "sync_joined patches=0",
        ]
    );

    // A failing sync_before_apply rejects a change and leaves the timeline.
    common::rename(&mut host_document, host_timeline, "main/clip-1", "Refused");
    host.session_mut()
        .publish(&mut host_document, host_timeline, "main")
        .expect("publishes");
    pump(
        &mut room,
        &mut [(host_id, &mut host), (viewer_id, &mut viewer)],
        0,
    )
    .expect("relayed");
    let event = viewer.poll_event().expect("the change");
    *REFUSE.lock().expect("unpoisoned") = Some(SYNC_BEFORE_APPLY);
    let handled = session::handle(&mut document, timeline, &event, &options).expect("handles");
    assert_eq!(
        handled,
        Handled::Rejected("sync_before_apply says no".to_string())
    );
    assert_eq!(
        common::name_of(&document, timeline, "main/clip-1"),
        "Beach Run"
    );
    *REFUSE.lock().expect("unpoisoned") = None;
    take_log();

    // The next change still fits the copy that refused one.
    common::rename(
        &mut host_document,
        host_timeline,
        "main/clip-2",
        "Sunset v2",
    );
    host.session_mut()
        .publish(&mut host_document, host_timeline, "main")
        .expect("publishes");
    pump(
        &mut room,
        &mut [(host_id, &mut host), (viewer_id, &mut viewer)],
        0,
    )
    .expect("relayed");
    let Some(Event::Timeline(change)) = viewer.poll_event() else {
        panic!("a change")
    };
    assert_eq!(change.patches.len(), 1, "only the second rename");
    let handled = session::handle(
        &mut document,
        timeline,
        &Event::Timeline(change.clone()),
        &options,
    )
    .expect("handles");
    assert_eq!(handled, Handled::Applied);
    assert_eq!(
        common::name_of(&document, timeline, "main/clip-2"),
        "Sunset v2"
    );
    assert_eq!(
        common::name_of(&document, timeline, "main/clip-1"),
        "Beach Run"
    );
    assert_eq!(
        take_log(),
        [
            "sync_before_send patches=1",
            "sync_before_apply patches=1",
            "sync_after_apply patches=1"
        ]
    );

    // Patches that do not fit this copy put the whole snapshot in place;
    // handles to objects whose GUIDs survive still work, and every clip is
    // linked again.
    let clip = otio_ext_sync::guid::GuidIndex::build(&document, timeline)
        .expect("ok")
        .get("main/clip-2")
        .expect("there");
    let mut misfit = change;
    misfit.patches = otio_ext_sync::patch::from_json(
        r#"[{"op":"remove_child","parentGuid":"main/v1","guid":"no-such-clip"}]"#,
    )
    .expect("reads");
    let handled = session::handle(&mut document, timeline, &Event::Timeline(misfit), &options)
        .expect("handles");
    assert_eq!(handled, Handled::Replaced);
    assert_eq!(
        common::name_of(&document, timeline, "main/clip-1"),
        "Refused"
    );
    assert_eq!(
        document
            .get(clip)
            .and_then(otio_core::Node::base)
            .expect("kept")
            .name,
        "Sunset v2"
    );
    assert_eq!(
        take_log(),
        [
            "sync_before_apply patches=1",
            "linked Refused",
            "linked Sunset v2",
            "sync_after_apply patches=1"
        ]
    );

    for hook in [
        SYNC_BEFORE_SEND,
        SYNC_BEFORE_APPLY,
        SYNC_AFTER_APPLY,
        SYNC_JOINED,
    ] {
        plugins::registry().set_scripts_attached_to(hook, Vec::new());
    }
}
