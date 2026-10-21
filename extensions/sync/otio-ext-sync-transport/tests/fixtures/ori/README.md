# ORI Sync fixtures

`recordings/` are the `sync_test/recordings` of
[ORIAnnotations](https://github.com/richardssam/ORIAnnotations) at commit
`904b2efe9881cfa3fdfbfed7ccaf137a30e6fec3`, under its Apache License 2.0:
sessions captured by ORI's `sync_recorder` as JSON lines, one envelope per
line. `add_media_notc.jsonl` is left out, since at that commit it is not
valid JSON lines.

`expected/` is what ORI's own headless `SyncManager` ends with after
replaying each recording the way ORI's `sync_recorder` player replays it,
written by `reference.py`:

    python3 reference.py <ORIAnnotations>/python recordings expected

`tests/ori_replay.rs` replays the same recordings into the ORI profile and
compares. The CI job `ORI Sync interop` regenerates `expected/` from
ORIAnnotations at the commit above, with `opentimelineio==0.18.1`, and fails
if the files change; bump the commit there and here together.

`live_peer.py` runs ORI's peer over RabbitMQ for `tests/ori_live.rs`.
