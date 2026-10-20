# Furnace's conformance corpus

Copied unchanged from `conformance/` in Alchemist's Furnace (svc-furnace), at
commit `a24afe45ea68ec9dc21bea62c06286472f0879b7`: corpus revision `4`, as
`VERSION` says. Furnace generates these from its real `internal/wire` structs
and checks the committed files byte for byte, so they are what Furnace sends
and accepts.

`tests/conformance.rs` reads every one:

- each `client-*.json` is read as a server would read it, encoded again by
  this crate's Furnace profile, and must match the fixture (the same JSON
  values, ignoring `seq` and `ts`, which a live client numbers itself);
- each `server-*.json` must decode to the message its type names, with the
  values the fixture holds.

Do not edit them here. When Furnace bumps its corpus, copy `conformance/`
again, update the commit and revision above, and update the test.
