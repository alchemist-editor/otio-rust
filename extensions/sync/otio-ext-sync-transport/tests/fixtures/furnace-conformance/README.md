# Furnace's conformance corpus

Copied unchanged from `conformance/` in Alchemist's Furnace (svc-furnace), at
corpus revision `5`, as `VERSION` says: `v1` to `v4` from commit
`a24afe45ea68ec9dc21bea62c06286472f0879b7`, and `v5` from branch
`claude/project-thread-tmp5sd`, where Furnace learns to relay patches
(revision 5 adds the `patch` message and its errors). Furnace generates these from its real `internal/wire` structs
and checks the committed files byte for byte, so they are what Furnace sends
and accepts.

`tests/conformance.rs` reads every one:

- each `client-*.json` is read as a server would read it, encoded again by
  this crate's Furnace profile, and must match the fixture (the same JSON
  values, ignoring `seq` and `ts`, which a live client numbers itself);
- each `server-*.json` must decode to the message its type names, with the
  values the fixture holds;
- `client-patch-v2.json`'s patches, applied to the welcome's timeline, must
  reach the timeline in `server-snapshot-patched-v2.json`, which Furnace
  worked out with its own `otiodiff.Apply`.

Do not edit them here. When Furnace bumps its corpus, copy `conformance/`
again, update the commit and revision above, and update the test.
