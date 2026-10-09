import {
  init,
  attachHookScript,
  detachHookScript,
  ExternalReference,
  readFromString,
  registerHookScript,
  registerMediaLinker,
  unregisterHookScript,
  unregisterMediaLinker,
} from "@alchemist-edit/otio";

await init();

// A cut of two clips, as an .otio file would hold it.
const cut = `{
  "OTIO_SCHEMA": "Track.1",
  "name": "V1",
  "kind": "Video",
  "children": [
    {"OTIO_SCHEMA": "Clip.2", "name": "A"},
    {"OTIO_SCHEMA": "Clip.2", "name": "B"}
  ]
}`;

// A media linker is handed each clip as it is read, with the arguments the
// read was given, and answers with the media the clip should use. Answering
// nothing leaves the clip as it was; throwing stops the read.
registerMediaLinker("proxies", (clip, args) =>
  new ExternalReference({ name: clip.name, targetUrl: `${args.root}/${clip.name}.mov` }),
);

// A hook script is handed the whole result, and answers with what the read
// goes on with: here the same object, stamped.
registerHookScript("stamp", (target, args) => {
  target.metadata.set("read_by", String(args.who));
  return target;
});
attachHookScript("post_adapter_read", "stamp");

try {
  // The read names the linker, and carries both sets of arguments as JSON.
  const track = readFromString("otioJson", cut, {
    mediaLinker: "proxies",
    mediaLinkerArguments: JSON.stringify({ root: "/proxies" }),
    hookArguments: JSON.stringify({ who: "the conform" }),
  });

  console.log("read by", track.metadata.get("read_by"));
  for (const clip of track.findClips()) {
    const media = clip.mediaReference();
    if (media instanceof ExternalReference) {
      console.log(clip.name, "->", media.targetUrl);
    }
  }
} finally {
  detachHookScript("post_adapter_read", "stamp");
  unregisterHookScript("stamp");
  unregisterMediaLinker("proxies");
}
