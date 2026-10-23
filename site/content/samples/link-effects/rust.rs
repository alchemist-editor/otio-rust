use otio_adapter::plugins::{self, LinkerChoice, PluginArguments};
use otio_core::{AnyDictionary, Document, Node, NodeId};

// A clip with a blur and a dissolve into the next one, as an editor that
// knows nothing of the registry writes them: named, and nothing more.
const CUT: &str = r#"{
  "OTIO_SCHEMA": "Track.1",
  "name": "V1",
  "kind": "Video",
  "children": [
    {"OTIO_SCHEMA": "Clip.2", "name": "A", "effects": [
      {"OTIO_SCHEMA": "Effect.1", "name": "blur", "effect_name": "Gaussian Blur"}
    ]},
    {"OTIO_SCHEMA": "Transition.2", "name": "dissolve",
     "transition_type": "SMPTE_Dissolve",
     "effect": {"OTIO_SCHEMA": "Effect.2", "effect_name": "Cross Dissolve"}},
    {"OTIO_SCHEMA": "Clip.2", "name": "B"}
  ]
}"#;

/// Prints what an effect was called and what the linker made of it.
fn show(document: &Document, holder: &str, effect: NodeId) -> Result<(), String> {
    let node = document.try_get(effect).map_err(|error| error.to_string())?;
    let effect = node.effect().ok_or("not an effect")?;
    println!("{holder}: {} -> {}", effect.effect_name, effect.effect_id);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `otio_fx_registry` is built in, but runs only when a read names it.
    let mut document = otio_core::from_str(CUT)?;
    let arguments = PluginArguments {
        effects_linker: LinkerChoice::Named(plugins::OTIO_FX_REGISTRY.into()),
        ..PluginArguments::default()
    };
    plugins::after_read(&mut document, &arguments, AnyDictionary::new())?;

    let track = document.root().ok_or("the cut has no root")?;
    let children = document.try_get(track)?.children().unwrap_or_default().to_vec();
    for child in children {
        match document.try_get(child)? {
            Node::Clip(clip) => {
                for &effect in &clip.item.effects {
                    show(&document, &clip.item.base.name, effect)?;
                }
            }
            Node::Transition(transition) => {
                if let Some(effect) = transition.effect {
                    show(&document, &transition.base.name, effect)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
