# otio-fcp7

Final Cut Pro 7 interchange XML, read and written as OpenTimelineIO.

FCP 7 itself is long gone, but the XML it defined is still how a great many
tools hand an edit to one another: Premiere Pro, Resolve, Hiero and Media
Composer all read or write some dialect of it. This is a port of upstream
OpenTimelineIO's `otio-fcp-adapter`.

The format carries far more per-element detail than OTIO has fields for.
Everything this adapter does not turn into a real OTIO field is kept under the
`fcp_xml` key in the relevant object's metadata and written back out on the way
past, so a file read and written again keeps its colour settings, its effect
parameters and its host application's bookkeeping.
