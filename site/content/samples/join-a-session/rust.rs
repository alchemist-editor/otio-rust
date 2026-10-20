use otio_ext_sync::session::{self, ApplyOptions, Config, Event, Playhead, Session};
use otio_ext_sync_transport::{Connection, Frame, Furnace, now_millis};

// The frame Furnace sends a viewer joining a session whose host has shared
// one timeline: the welcome, holding the timeline as it is.
const WELCOME: &str = r#"{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","role":"viewer","roster":[{"id":"p_1","displayName":"Host","role":"host","joinedAt":0,"connectionState":"connected","muted":false}],"session":{"id":"s_1","createdAt":0},"snapshot":{"sequenceId":"main","generation":1,"otioJson":{"OTIO_SCHEMA":"Timeline.1","name":"Coastline","metadata":{"sync":{"guid":"t-1"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"s-1"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"v-1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Waves","metadata":{"sync":{"guid":"c-1"}}}]}]}}}}}"#;

// A real program writes each frame to its WebSocket to Furnace, or lets
// `otio_ext_sync_transport::websocket::Client` do it all.
fn send(connection: &mut Connection) -> Result<(), otio_ext_sync_transport::Error> {
    while let Some(frame) = connection.poll_transmit(now_millis())? {
        let kind = if frame.binary { "binary" } else { "text" };
        println!("send a {kind} frame of {} bytes", frame.data.len());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    otio_ext_sync::install();
    otio_ext::enable(otio_ext_sync::ID)?;

    // The session owns no socket. Open one to Furnace, then connect: the
    // hello is the first frame to send.
    let mut config = Config {
        token: "join-code".to_string(),
        ..Config::default()
    };
    config.identity.display_name = "Ada".to_string();
    let mut connection = Connection::new(Session::new(config), Furnace::new());
    connection.connect();
    send(&mut connection)?;

    // Hand the session every frame that arrives. This is the one Furnace
    // sends back.
    connection.receive(&Frame::text(WELCOME))?;

    // The program's copy of the timeline starts empty. Handling the events
    // puts the host's timeline in it, and keeps it current from then on.
    let mut document = otio_core::from_str(r#"{"OTIO_SCHEMA": "Timeline.1"}"#)?;
    let timeline = document.root().ok_or("the document has no root")?;
    while let Some(event) = connection.poll_event() {
        let handled = session::handle(&mut document, timeline, &event, &ApplyOptions::default())?;
        let kind = match event {
            Event::Timeline(_) => "timeline",
            Event::Joined { .. } => "joined",
            _ => "other",
        };
        println!("{kind} {handled:?}");
    }
    println!(
        "the viewer now shows {}",
        document.try_get(timeline)?.name()
    );

    // Everyone in the session sees where this viewer's playhead is.
    connection.session_mut().set_playhead(Playhead {
        sequence_id: "main".to_string(),
        frame: 24.0,
        playing: true,
        rate: 1.0,
    })?;
    send(&mut connection)?;

    connection.session_mut().leave();
    send(&mut connection)?;
    Ok(())
}
