use otio_ext_ori_annotations::{annotations, command, review};
use otio_ext_sync::session::{Config, Event, Session};
use otio_ext_sync_transport::{Connection, Frame, Furnace, now_millis};

// The cut both sides have: one shot, which sync knows by the GUID main/shot.
const CUT: &str = r#"{"OTIO_SCHEMA":"Timeline.1","name":"review","metadata":{"sync":{"guid":"main"}},
 "tracks":{"OTIO_SCHEMA":"Stack.1","children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video",
  "children":[{"OTIO_SCHEMA":"Clip.2","name":"shot","metadata":{"sync":{"guid":"main/shot"}},
   "source_range":{"OTIO_SCHEMA":"TimeRange.1",
    "start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},
    "duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}}]}]}}"#;

// What Furnace tells a reviewer who joins: who they are, and that they may comment.
const WELCOME: &str = r#"{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","selfUserId":"u_bea","role":"viewer","roster":[],"session":{"id":"s_1","createdAt":0}}}"#;

// Where the reviewer drew: frame 10 of the shot, as the program shows it.
const ANCHOR: &str = r#"{"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/shot",
 "sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},
 "capturedAt":1787742000000}"#;

// One stroke: a PaintStart, its points and a PaintEnd sharing a uuid.
const STROKE: &str = r#"[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0.25,0.25,1]},
 {"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1",
  "x":[-0.5,0,0.5],"y":[0.2,0,-0.2],"size":[0.01,0.01,0.01],"alpha":[1,1,1]}},
 {"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}]"#;

// What Furnace tells the host when it opens the session.
const HOST_WELCOME: &str = r#"{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_1","selfUserId":"u_ari","role":"host","roster":[],"session":{"id":"s_1","createdAt":0}}}"#;

// The comment as Furnace relays it once it has stored it, to everyone in
// the session. A real host reads this from its own socket.
fn relay() -> String {
    format!(
        r#"{{"v":2,"type":"review_commit","sessionId":"s_1","senderId":"p_2","seq":1,"ts":0,"order":2,"payload":{{"clientMutationId":"m1","commentId":"c1","authorUserId":"u_bea","authorPresenceId":"p_2","serverOrder":1,"revision":1,"createdAt":0,"anchor":{ANCHOR},"bodyMarkdown":"Too dark here.","annotationCommands":{STROKE}}}}}"#
    )
}

// Stands in for the network: a real program writes every frame the session
// has waiting to its WebSocket to Furnace.
fn send(connection: &mut Connection) -> Result<(), otio_ext_sync_transport::Error> {
    while let Some(frame) = connection.poll_transmit(now_millis())? {
        println!("send {} bytes", frame.data.len());
    }
    Ok(())
}

fn joining(token: &str, capability: &[&str]) -> Connection {
    let mut config = Config {
        token: token.to_string(),
        ..Config::default()
    };
    config.identity.capabilities = capability.iter().map(|name| name.to_string()).collect();
    Connection::new(Session::new(config), Furnace::new())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ORI Annotations is an extension; enabling it enables sync too.
    otio_ext_ori_annotations::install();
    otio_ext::enable(otio_ext_ori_annotations::ID)?;

    // The reviewer joins with the right to comment.
    let mut reviewer = joining(
        "join-code",
        &["review.comment.create", "review.annotation.create"],
    );
    reviewer.connect();
    send(&mut reviewer)?;
    reviewer.receive(&Frame::text(WELCOME))?;
    while reviewer.poll_event().is_some() {}

    // Commit the stroke with a note. The session sends it again after every
    // reconnect until Furnace stores it, so it is not lost.
    let seen = otio_core::from_str(CUT)?;
    let id = review::commit(
        reviewer.session_mut(),
        &seen,
        seen.root().ok_or("the cut has no root")?,
        &review::Anchor::from_json(ANCHOR)?,
        "Too dark here.",
        &command::from_json(STROKE)?,
        "",
    )?;
    println!("committed {id}");
    send(&mut reviewer)?;

    // The host hears the stored comment and folds it into its timeline.
    let mut host = joining("host-token", &["review.commit.receive"]);
    host.connect();
    send(&mut host)?;
    host.receive(&Frame::text(HOST_WELCOME))?;
    host.receive(&Frame::text(relay()))?;
    let mut document = otio_core::from_str(CUT)?;
    let timeline = document.root().ok_or("the cut has no root")?;
    while let Some(event) = host.poll_event() {
        // A stroke still being drawn: draw it over the picture, then drop it.
        if let Some(partial) = review::read_partial(&event)? {
            println!("drawing {} commands", partial.commands.len());
            continue;
        }
        let Event::ReviewRelay(relay) = event else {
            continue;
        };
        let folded = review::fold(&mut document, timeline, &relay)?;
        let about = annotations::describe(&document, folded.clip)?;
        println!(
            "folded {} already there: {}",
            about.to_json(&document),
            folded.already
        );
        let owner = review::owner_of(&document, folded.clip, "s1")?;
        println!("the stroke belongs to {owner:?}");
    }
    Ok(())
}
