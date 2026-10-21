"""Writes what ORI's Python makes of a small review, as the fixture
`ori_built.otio`: upstream OpenTimelineIO 0.18.1 with ORI's schemadef
plugin, building the annotation track the way ORI's SyncManager does
(`get_or_create_clip_timeline`, `broadcast_add_annotation`,
`_make_annotation_clip`) and the stroke and text the way
`annotation_builder.make_stroke` and `make_text` do, with fixed uuids and
timestamps."""
import sys
import opentimelineio as otio

sys.path.insert(0, sys.argv[1] + "/python")
from otio_sync_core import coords

se = otio.schema.schemadef.module_from_name("SyncEvent")
W, H = 1920, 1080
TS = "2026-10-21T09:30:00"


def guid(obj, value):
    obj.metadata["sync"] = {"guid": value}


tl = otio.schema.Timeline(name="review")
guid(tl, "timeline")
guid(tl.tracks, "stack")
v1 = otio.schema.Track(name="V1", kind=otio.schema.TrackKind.Video)
guid(v1, "v1")
clip = otio.schema.Clip(
    name="shot",
    media_reference=otio.schema.ExternalReference(target_url="shot.mov"),
    source_range=otio.opentime.TimeRange(
        otio.opentime.RationalTime(0, 24), otio.opentime.RationalTime(48, 24)
    ),
)
guid(clip, "shot")
v1.append(clip)
ann = otio.schema.Track(name="Annotations")
guid(ann, "annotations")
tl.tracks.append(v1)
tl.tracks.append(ann)


def track_end(track):
    return sum(int(c.source_range.duration.value) for c in track if c.source_range is not None)


def ensure_clip(track, clip_guid, frame, rate, gap_guid, clip_guid_value):
    for child in track:
        if isinstance(child, otio.schema.Clip) and child.metadata.get("clip_guid") == clip_guid \
                and int(child.source_range.start_time.value) == frame:
            return child
    end = track_end(track)
    if frame > end:
        gap = otio.schema.Gap(source_range=otio.opentime.TimeRange(
            otio.opentime.RationalTime(end, rate), otio.opentime.RationalTime(frame - end, rate)))
        guid(gap, gap_guid)
        track.append(gap)
    c = otio.schema.Clip(name=f"Annotation_{frame}")
    c.source_range = otio.opentime.TimeRange(otio.opentime.RationalTime(frame, rate), otio.opentime.RationalTime(1, rate))
    c.metadata["annotation_commands"] = []
    c.metadata["clip_guid"] = clip_guid
    guid(c, clip_guid_value)
    track.append(c)
    return c


def stroke(uuid, points_px, rgba, size):
    start = se.PaintStart()
    start.brush = "circle"
    start.friendly_name = "Jeff"
    start.participant_hash = "p-jeff"
    start.rgba = [float(c) for c in rgba]
    start.source_index = 0
    start.timestamp = TS
    start.type = "color"
    start.uuid = uuid
    start.visible = True
    xs, ys = [], []
    for px, py in points_px:
        nx, ny = coords.px_to_otio(px, py, W, H)
        xs.append(nx)
        ys.append(ny)
    pts = se.PaintPoints()
    pts.source_index = 0
    pts.uuid = uuid
    pts.timestamp = TS
    pts.points = se.PaintVertices(xs, ys, [size] * len(xs))
    end = se.PaintEnd()
    end.uuid = uuid
    end.timestamp = TS
    return [start, pts, end]


def text(uuid, px, py, body):
    nx, ny = coords.px_to_otio(px, py, W, H)
    t = se.TextAnnotation()
    t.uuid = uuid
    t.rgba = [1.0, 1.0, 1.0, 1.0]
    t.friendly_name = "Jeff"
    t.text = body
    t.position = [nx, ny]
    t.font_size = 0.04
    t.scale = 1.0
    t.rotation = 0.0
    t.spacing = 0.8
    t.font = "monospace"
    t.timestamp = TS
    return t


c10 = ensure_clip(ann, "shot", 10, 24, "gap-0", "annotation-10")
c10.metadata["annotation_commands"].extend(
    stroke("stroke-1", [(200, 200), (960, 540), (1720, 880)], [1, 0.25, 0.25, 1], 0.01))
c10.metadata["annotation_commands"].append(text("text-1", 960, 620, "Eyeline is late"))
# A second frame before the track's end: ORI appends it after the first.
c4 = ensure_clip(ann, "shot", 4, 24, "gap-unused", "annotation-4")
c4.metadata["annotation_commands"].extend(
    stroke("stroke-2", [(100, 100), (300, 300)], [0.2, 0.8, 0.3, 1], 0.005))

sys.stdout.write(otio.adapters.write_to_string(tl))
