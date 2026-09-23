import OpenTimelineIO

let timeline = try Timeline(name: "Cut")
let track = try Track(name: "V1", kind: "Video")
try (timeline.tracks() as? Stack)?.appendChild(track)

// A cut of two clips: one whose media is a file beside the program, and one
// whose media is on the web.
for (name, url) in [("A001C003", "shot.mov"), ("A001C004", "https://example.com/remote.mov")] {
    let clip = try Clip(name: name)
    try clip.setMediaReference("DEFAULT_MEDIA", reference: try ExternalReference(targetURL: url))
    try clip.setActiveMediaReferenceKey("DEFAULT_MEDIA")
    try track.appendChild(clip)
}

// Every clip whose media is a file has the file copied into the bundle and
// its reference pointed at the copy. Media that is not a file would stop the
// write, so it is made missing instead. `.otiod` writes the same layout as a
// directory.
try OTIO.writeToFile(
    .otioz, root: timeline, path: "cut.otioz",
    options: WriteOptions(bundleMediaPolicy: .missingIfNotFile))

// Unpacked, with each reference made absolute, the media is ready to use.
let bundled = try OTIO.readFromFile(
    .otioz, path: "cut.otioz",
    options: ReadOptions(bundleExtractPath: "cut", bundleAbsoluteMediaPaths: true))
for case let clip as Clip in try bundled.findClips() {
    if let media = try clip.mediaReference() as? ExternalReference {
        print(try media.targetURL())
    } else {
        print("missing")
    }
}
