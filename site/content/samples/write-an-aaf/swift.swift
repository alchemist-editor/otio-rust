import OpenTimelineIO

let timeline = try Timeline(name: "Cut")
let stack = try Stack(name: "tracks")
let track = try Track(name: "V1", kind: "Video")

try timeline.setTracks(stack)
try stack.appendChild(track)

let oneSecond = TimeRange(
    startTime: RationalTime(value: 0, rate: 24),
    duration: RationalTime(value: 24, rate: 24))

// An AAF clip is cut from media of a known length, so each clip's media says
// how much of it there is. A new clip has no media at all, so its reference
// goes in under upstream's key and is made the active one.
for name in ["A001C003", "A001C004"] {
    let media = try ExternalReference(targetURL: "file:///media/\(name).mov")
    try media.setAvailableRange(oneSecond)

    let clip = try Clip(name: name)
    try clip.setMediaReference("DEFAULT_MEDIA", reference: media)
    try clip.setActiveMediaReferenceKey("DEFAULT_MEDIA")
    try clip.setSourceRange(oneSecond)
    try track.appendChild(clip)
}

// Every clip needs a MobID, from its metadata, its media's metadata or the
// AAF its media names. A cut built from scratch has none, so let the writer
// make them up rather than refuse the clip.
try OTIO.writeToFile(
    .aaf, root: timeline, path: "cut.aaf", options: WriteOptions(aafUseEmptyMobIds: true))

for case let clip as Clip in try OTIO.readFromFile(.aaf, path: "cut.aaf").findClips() {
    print(try clip.name())
}
