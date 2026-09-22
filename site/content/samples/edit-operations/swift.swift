import OpenTimelineIO

/// One second of picture, named.
func second(_ name: String) throws -> Clip {
    let clip = try Clip(name: name)
    try clip.setSourceRange(
        TimeRange(
            startTime: RationalTime(value: 0, rate: 24),
            duration: RationalTime(value: 24, rate: 24)))
    return clip
}

func show(_ track: Track) throws {
    let names = try track.children().map { try $0.name() }.joined(separator: " ")
    print(names, "-", try track.duration().value, "frames")
}

let track = try Track(name: "V1", kind: "Video")
for name in ["A", "B", "C"] {
    try track.appendChild(try second(name))
}
try show(track)

// Insert makes room: everything from the insertion point onwards moves later,
// and the track gets longer.
try OTIO.insert(
    try second("D"), composition: track,
    time: RationalTime(value: 24, rate: 24), removeTransitions: false)
try show(track)

// Overwrite does not: it lays an item over a span and whatever was in that
// span gives way. The track is the same length afterwards.
try OTIO.overwrite(
    try second("E"), composition: track,
    range: TimeRange(
        startTime: RationalTime(value: 48, rate: 24),
        duration: RationalTime(value: 24, rate: 24)),
    removeTransitions: false)
try show(track)
