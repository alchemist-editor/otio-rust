import OpenTimelineIO

// An EDL never says what rate its timecode is at, so this has to be right: a
// file read at the wrong rate puts every event in the wrong place rather
// than failing.
let options = ReadOptions(rate: 24, nameColumn: "", ignoreTimecodeMismatch: false)

// Reading hands back what the file is about, not a container to look inside.
let root = try OTIO.readFromFile(.cmx3600, path: "cut.edl", options: options)

for case let clip as Clip in try root.findClips() {
    print(try clip.name())
}
