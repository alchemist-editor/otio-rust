use otio_adapter::plugins::{self, HookScript, LinkerChoice, MediaLinker, PluginArguments};
use otio_core::schema::{ExternalReference, MediaReferenceData};
use otio_core::{Any, AnyDictionary, Node};

// A cut of two clips, as an .otio file would hold it.
const CUT: &str = r#"{
  "OTIO_SCHEMA": "Track.1",
  "name": "V1",
  "kind": "Video",
  "children": [
    {"OTIO_SCHEMA": "Clip.2", "name": "A"},
    {"OTIO_SCHEMA": "Clip.2", "name": "B"}
  ]
}"#;

/// One string out of an argument map.
fn text(arguments: &AnyDictionary, key: &str) -> Result<String, String> {
    match arguments.get(key) {
        Some(Any::String(value)) => Ok(value.clone()),
        _ => Err(format!("no {key} in the arguments")),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A media linker is handed each clip as it is read, with the arguments
    // the read was given, and answers with media it put in the document for
    // the clip to use. `None` leaves the clip as it was.
    plugins::registry().register_media_linker(
        "proxies",
        MediaLinker::new(|document, clip, arguments| {
            let name = document
                .try_get(clip)
                .map_err(|error| error.to_string())?
                .base()
                .map(|base| base.name.clone())
                .unwrap_or_default();
            let root = text(arguments, "root")?;
            Ok(Some(document.insert(Node::ExternalReference(
                ExternalReference {
                    media: MediaReferenceData::default(),
                    target_url: format!("{root}/{name}.mov"),
                },
            ))))
        }),
    );

    // A hook script is handed the whole result, and answers with what the
    // read goes on with: here the same object, stamped.
    plugins::registry().register_hook_script(
        "stamp",
        HookScript::new(|document, target, arguments| {
            let who = text(arguments, "who")?;
            if let Some(base) = document
                .try_get_mut(target)
                .map_err(|error| error.to_string())?
                .base_mut()
            {
                base.metadata.insert("read_by".into(), Any::String(who));
            }
            Ok(target)
        }),
    );
    plugins::registry().attach_hook_script(plugins::POST_ADAPTER_READ, "stamp");

    // In Rust the hooks run where you say: an adapter only parses, and
    // `after_read` is what upstream runs once it has, in upstream's order.
    let mut document = otio_core::from_str(CUT)?;
    let mut arguments = PluginArguments {
        media_linker: LinkerChoice::Named("proxies".into()),
        ..PluginArguments::default()
    };
    arguments
        .media_linker_arguments
        .insert("root".into(), Any::String("/proxies".into()));
    arguments
        .hook_arguments
        .insert("who".into(), Any::String("the conform".into()));
    plugins::after_read(&mut document, &arguments, AnyDictionary::new())?;

    let track = document.root().ok_or("the cut has no root")?;
    if let Some(Any::String(who)) = document
        .try_get(track)?
        .base()
        .and_then(|base| base.metadata.get("read_by"))
    {
        println!("read by {who}");
    }
    for clip in document.find_clips(track)? {
        let Node::Clip(found) = document.try_get(clip)? else {
            continue;
        };
        let media = found.media_references[&found.active_media_reference_key];
        if let Node::ExternalReference(external) = document.try_get(media)? {
            println!("{} -> {}", found.item.base.name, external.target_url);
        }
    }

    plugins::registry().remove_hook_script("stamp");
    plugins::registry().remove_media_linker("proxies");
    Ok(())
}
