use otio_adapter::Adapter;
use otio_cmx3600::{Cmx3600, ReadOptions};

fn main() -> Result<(), otio_adapter::Error> {
    // An EDL never says what rate its timecode is at, so this has to be
    // right: a file read at the wrong rate puts every event in the wrong
    // place rather than failing.
    let options = ReadOptions {
        rate: 24.0,
        ..Default::default()
    };
    let document = Cmx3600::read_from_file("cut.edl", &options)?;

    let root = document.root().expect("a parsed document has a root");
    for id in document.find_clips(root)? {
        let clip = document.try_get(id)?;
        println!("{}", clip.name());
    }

    Ok(())
}
