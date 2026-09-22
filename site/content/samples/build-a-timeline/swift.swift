import OpenTimelineIO

// Each object is made on its own and joins a timeline when you put it into
// one. Nothing has to exist before the thing it goes into.
let timeline = try Timeline(name: "Cut")
let stack = try Stack(name: "tracks")
let track = try Track(name: "V1", kind: "Video")

try timeline.setTracks(stack)
try stack.appendChild(track)

for (index, name) in ["A", "B", "C"].enumerated() {
    let clip = try Clip(name: name)
    try clip.setSourceRange(
        TimeRange(
            startTime: RationalTime(value: Double(index * 24), rate: 24),
            duration: RationalTime(value: 24, rate: 24)))
    try track.appendChild(clip)
}

// Three seconds of picture, written as canonical OpenTimelineIO JSON.
// Objects keep their timeline alive between them, so there is nothing to
// close.
print(try track.duration().toSeconds)
try OTIO.save(timeline, to: "cut.otio")
