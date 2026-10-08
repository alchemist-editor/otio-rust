import json
import os
import tempfile

# Python finds linkers and hook scripts the way upstream does: through a
# plugin manifest naming the modules that hold them. This one is written out
# here so the sample stands alone; a studio would keep it beside its tools and
# point OTIO_PLUGIN_MANIFEST_PATH at it once.
plugins = tempfile.mkdtemp()

with open(os.path.join(plugins, "proxies.py"), "w") as module:
    module.write('''
import opentimelineio as otio

def link_media_reference(in_clip, media_linker_argument_map):
    root = media_linker_argument_map["root"]
    return otio.schema.ExternalReference(
        target_url="{}/{}.mov".format(root, in_clip.name)
    )
''')

with open(os.path.join(plugins, "stamp.py"), "w") as module:
    module.write('''
def hook_function(in_timeline, argument_map=None):
    in_timeline.metadata["read_by"] = argument_map["who"]
    return in_timeline
''')

with open(os.path.join(plugins, "plugin_manifest.json"), "w") as manifest:
    json.dump({
        "OTIO_SCHEMA": "PluginManifest.1",
        "media_linkers": [
            {"OTIO_SCHEMA": "MediaLinker.1", "name": "proxies", "filepath": "proxies.py"},
        ],
        "hook_scripts": [
            {"OTIO_SCHEMA": "HookScript.1", "name": "stamp", "filepath": "stamp.py"},
        ],
        "hooks": {"post_adapter_read": ["stamp"]},
    }, manifest)

os.environ["OTIO_PLUGIN_MANIFEST_PATH"] = os.path.join(plugins, "plugin_manifest.json")

import opentimelineio as otio  # noqa: E402

# A cut of two clips, as an .otio file would hold it.
cut = json.dumps({
    "OTIO_SCHEMA": "Track.1",
    "name": "V1",
    "kind": "Video",
    "children": [
        {"OTIO_SCHEMA": "Clip.2", "name": "A"},
        {"OTIO_SCHEMA": "Clip.2", "name": "B"},
    ],
})

# The read names the linker, and carries both sets of arguments.
track = otio.adapters.read_from_string(
    cut,
    "otio_json",
    media_linker_name="proxies",
    media_linker_argument_map={"root": "/proxies"},
    hook_function_argument_map={"who": "the conform"},
)

print("read by", track.metadata["read_by"])
for clip in track.find_clips():
    print(clip.name, "->", clip.media_reference.target_url)
