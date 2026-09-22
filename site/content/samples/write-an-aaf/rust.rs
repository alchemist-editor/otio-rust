use otio_aaf::{Aaf, WriteOptions};
use otio_adapter::Adapter;

fn main() -> Result<(), otio_adapter::Error> {
    // AAF is written from a timeline. A cut that was read from an AAF keeps
    // what the reader found under metadata["AAF"], and that is written back,
    // so its clips point at the same master mobs and the same media.
    let document = otio_core::from_str(&std::fs::read_to_string("cut.otio")?)?;

    // Every clip needs a MobID, from its metadata, its media's metadata or
    // the AAF its media names. A cut built from scratch has none, so let
    // the writer make them up rather than refuse the clip.
    let options = WriteOptions::new().with_use_empty_mob_ids(true);
    Aaf::write_to_file(&document, "cut.aaf", &options)?;

    Ok(())
}
