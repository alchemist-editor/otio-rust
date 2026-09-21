# otio-adapter

The trait that every OpenTimelineIO file-format adapter in this workspace
implements, together with the error type they share and the pieces of
metadata more than one of them writes.

Upstream OpenTimelineIO makes an adapter a Python plugin with four loosely
specified entry points and a bag of keyword arguments. Here it is one trait,
`Adapter`, with each format's read and write options named and typed, so that
an option meant for one format cannot be quietly handed to another.

`Adapter` is stated over bytes, since AAF is a binary container. Formats that
really are text — ALE, EDL, and the two FCP XML flavours — also implement
`TextAdapter`, which works in `&str` and `String`.

See `crates/otio-ale` for a worked example.
