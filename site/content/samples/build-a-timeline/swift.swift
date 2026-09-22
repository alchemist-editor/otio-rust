import OpenTimelineIO

let document = Document.new()
defer { document.close() }

let timeline = try document.newTimeline("Cut")
let stack = try document.newStack("tracks")
try timeline.setTracks(stack)
let track = try document.newTrack("V1", kind: "Video")
try stack.appendChild(track)

for (index, name) in ["A", "B", "C"].enumerated() {
    let clip = try document.newClip(name)
    try clip.setSourceRange(
        TimeRange(
            startTime: RationalTime(value: Double(index * 24), rate: 24),
            duration: RationalTime(value: 24, rate: 24)))
    try track.appendChild(clip)
}

try document.setRoot(timeline)

// Three seconds of picture, written as canonical OpenTimelineIO JSON.
print(try track.duration().toSeconds)
try document.save("cut.otio")
