import OpenTimelineIO

// A cut of two clips, as an .otio file would hold it.
let cut = """
    {
      "OTIO_SCHEMA": "Track.1",
      "name": "V1",
      "kind": "Video",
      "children": [
        {"OTIO_SCHEMA": "Clip.2", "name": "A"},
        {"OTIO_SCHEMA": "Clip.2", "name": "B"}
      ]
    }
    """

// A media linker is handed each clip as it is read, with the arguments the
// read was given, and answers with the media the clip should use. nil leaves
// the clip as it was.
try OTIO.registerMediaLinker("proxies") { clip, arguments in
    let name = try clip.name()
    let root = try arguments.getString("root")
    return try ExternalReference(name: name, targetURL: "\(root)/\(name).mov")
}

// A hook script is handed the whole result, and answers with what the read
// goes on with: here the same object, stamped.
try OTIO.registerHookScript("stamp") { target, arguments in
    let who = try arguments.getString("who")
    if let named = target as? SerializableObjectWithMetadata {
        try named.metadata.setString("read_by", value: who)
    }
    return target
}
try OTIO.attachHookScript("post_adapter_read", script: "stamp")

// The read names the linker, and carries both sets of arguments as JSON.
let track = try OTIO.readFromBytes(
    .otioJSON, data: Array(cut.utf8),
    options: ReadOptions(
        mediaLinker: "proxies",
        mediaLinkerArguments: #"{"root": "/proxies"}"#,
        hookArguments: #"{"who": "the conform"}"#))

if let named = track as? SerializableObjectWithMetadata {
    let who = try named.metadata.getString("read_by")
    print("read by", who)
}
for case let clip as Clip in try track.findClips() {
    if let media = try clip.mediaReference() as? ExternalReference {
        let name = try clip.name()
        let url = try media.targetURL()
        print(name, "->", url)
    }
}

// The registry is the whole process's, so a program that is done with its
// plugins says so.
_ = OTIO.detachHookScript("post_adapter_read", script: "stamp")
_ = OTIO.unregisterHookScript("stamp")
_ = OTIO.unregisterMediaLinker("proxies")
