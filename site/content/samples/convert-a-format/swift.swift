import OpenTimelineIO

// An EDL never says what rate its timecode is at, so this has to be right: a
// file read at the wrong rate puts every event in the wrong place rather
// than failing.
let options = ReadOptions(rate: 24, nameColumn: "", ignoreTimecodeMismatch: false)
let root = try OTIO.readFromFile(.cmx3600, path: "cut.edl", options: options)

// Nothing happens in between. The timeline an EDL parses to is the same
// timeline FCP X writes out, so converting is a read and a write: the object
// model is the interchange, and the file formats are two ways of spelling it.
try OTIO.writeToFile(.fcpxXML, root: root, path: "cut.fcpxml")
