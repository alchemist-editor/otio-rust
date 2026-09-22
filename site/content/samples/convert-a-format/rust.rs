use otio_adapter::Adapter;
use otio_cmx3600::{Cmx3600, ReadOptions};
use otio_fcpx::FcpxXml;

fn main() -> Result<(), otio_adapter::Error> {
    // An EDL never says what rate its timecode is at, so this has to be
    // right: a file read at the wrong rate puts every event in the wrong
    // place rather than failing.
    let options = ReadOptions {
        rate: 24.0,
        ..Default::default()
    };
    let document = Cmx3600::read_from_file("cut.edl", &options)?;

    // Nothing happens in between. The timeline an EDL parses to is the same
    // timeline FCP X writes out, so converting is a read and a write: the
    // object model is the interchange, and the file formats are two ways of
    // spelling it.
    FcpxXml::write_to_file(&document, "cut.fcpxml", &Default::default())?;

    Ok(())
}
