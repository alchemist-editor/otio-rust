import OpenTimelineIO

func frames(_ span: TimeRange) -> String {
    "\(span.startTime.value) for \(span.duration.value)"
}

// Ten seconds of rushes on disk. `availableRange` belongs to the media, not
// to the clip: it is what the file offers, whoever uses it.
let media = try ExternalReference(name: "A001", targetURL: "file:///A001.mov")
try media.setAvailableRange(
    TimeRange(
        startTime: RationalTime(value: 0, rate: 24),
        duration: RationalTime(value: 240, rate: 24)))

// Three seconds of it, starting two seconds in. A source range is in the
// media's clock, which is why it starts at 48 rather than at 0.
let clip = try Clip(name: "shot")
try clip.setMediaReference("DEFAULT_MEDIA", reference: media)
try clip.setSourceRange(
    TimeRange(
        startTime: RationalTime(value: 48, rate: 24),
        duration: RationalTime(value: 72, rate: 24)))

// A second of black in front of it, so the clip does not start the track.
let head = try Gap()
try head.setSourceRange(
    TimeRange(
        startTime: RationalTime(value: 0, rate: 24),
        duration: RationalTime(value: 24, rate: 24)))

let track = try Track(name: "V1", kind: "Video")
try track.appendChild(head)
try track.appendChild(clip)

// The same clip, asked four questions. The first three answer in the media's
// clock; the last answers in the track's.
print("available:", frames(try clip.availableRange()))
print("trimmed:  ", frames(try clip.trimmedRange()))
print("visible:  ", frames(try clip.visibleRange()))
print("in parent:", frames(try clip.rangeInParent()))
