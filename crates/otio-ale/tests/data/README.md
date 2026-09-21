# Sample data

These `.ale` files are copied verbatim from upstream's `otio-ale-adapter`
`tests/sample_data/`. They are Apache-2.0 licensed, the same licence as this
repository.

They are vendored rather than fetched so the conformance tests run offline and
against a pinned version. Between them they cover the shapes real files come
in: one heading pair per line and every pair on one line, blank lines
scattered through the heading and none at all between sections, colour
decisions stated three different ways, and a 4K frame that has to be called
`CUSTOM`.
