# Sample data

These `.otio` files are copied verbatim from upstream OpenTimelineIO's
`tests/sample_data/` at version 0.19.0. They are Apache-2.0 licensed, the same
licence as this repository.

They are vendored rather than fetched so the conformance tests run offline and
against a pinned version. They are the closest thing to a specification the
format has: whatever this library does with them, it must not lose.

## `multicam/` (EXPERIMENTAL, ADR 0014)

Written by the C++/Python multicam prototype on the
`claude/project-thread-otfkde` branch of jhodges10/OpenTimelineIO, not by an
upstream release:

- `multicam_synthetic.otio` and `multicam_synthetic_flat.otio`: the fork's
  `tests/test_multicam.py` timeline (with its nested multicam) and what its
  `flatten_multicam` makes of it.
- `avid_mcam_edit_multicam.otio` and `avid_mcam_edit_flat.otio`: Trevor
  Aylward's Avid test edit from
  [otio-multicam-tests](https://github.com/TrevorAyl/otio-multicam-tests)
  (MIT), read from its AAF by the fork's `examples/multicam/aaf_multicam.py`,
  and flattened.
