# Sample data

These files are copied verbatim from upstream's `otio-fcp-adapter`
`tests/sample_data/`. They are Apache-2.0 licensed, the same licence as this
repository.

They are vendored rather than fetched so the conformance tests run offline and
against a pinned version. Between them they cover the dialects real files come
in: Premiere Pro's own export with eight tracks, transitions, markers and
nested sequences; Hiero's much sparser one; Premiere's two ways of writing a
generator; a file whose `name` elements are empty; and a single filter with
its keyframe curve, paired with the metadata dictionary it has to read as.
