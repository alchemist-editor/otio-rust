# Furnace's otiodiff golden cases

These 11 cases are copied unchanged from `internal/otiodiff/testdata/` in
Alchemist's Furnace (svc-furnace), at commit
`a24afe45ea68ec9dc21bea62c06286472f0879b7`. Furnace's `testdata/generate.go`
wrote them with its Go `otiodiff`, which this crate ports.

Each directory holds `old.json` and `new.json`, two snapshots
(`sequenceId`, `generation` and the timeline as `otioJson`), and
`patches.json`, the patches Go's `Diff` made from them, written with
`json.MarshalIndent(patches, "", "  ")` and a trailing newline.
`tests/golden.rs` checks that this crate makes the same bytes and that
applying them to `old.json` gives `new.json`.

Do not edit them here. If Furnace regenerates its goldens, copy them again
and update the commit above.
