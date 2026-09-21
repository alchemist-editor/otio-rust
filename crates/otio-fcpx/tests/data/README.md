# Sample data

These files are copied verbatim from upstream's `otio-fcpx-xml-adapter`
`tests/sample_data/`. They are Apache-2.0 licensed, the same licence as this
repository.

They are vendored rather than fetched so the conformance tests run offline and
against a pinned version. Between them they cover the four shapes an FCP X
file comes in: a whole library, a bare event, a single project, and a file of
loose clips with no edit in it. `fcpx_example.fcpxml` is upstream's copy of
the library file under another name, kept so that a test written against
either name has something to read.
