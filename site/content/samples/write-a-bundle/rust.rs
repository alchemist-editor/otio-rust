use std::path::Path;

use otio_bundle::{MediaReferencePolicy, ReadOptions, WriteOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let document = otio_core::from_str(&std::fs::read_to_string("cut.otio")?)?;
    let timeline = document.root().ok_or("cut.otio holds nothing")?;

    // Every clip whose media is a file on disk has the file copied into the
    // bundle and its reference pointed at the copy. Media that is not a file,
    // such as a URL on the web, would stop the write, so it is made missing
    // instead. write_otiod writes the same layout as a directory.
    let options = WriteOptions {
        policy: MediaReferencePolicy::MissingIfNotFile,
        ..WriteOptions::default()
    };
    otio_bundle::write_otioz(&document, timeline, Path::new("cut.otioz"), &options)?;

    // Unpacked, with each reference made absolute, the media is ready to use.
    let unpacked = otio_bundle::read_otioz(
        Path::new("cut.otioz"),
        &ReadOptions {
            extract_path: Some("cut".into()),
            absolute_media_reference_paths: true,
        },
    )?;
    println!("{}", otio_core::to_string(&unpacked)?);

    Ok(())
}
