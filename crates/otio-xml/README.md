# otio-xml

A small XML tree, parser and pretty printer, written for the OpenTimelineIO
XML adapters in this workspace.

This is not a general-purpose XML library. It exists because the workspace
carries no third-party dependencies, and because the adapters being ported
are written against Python's `xml.etree.ElementTree` model and emit output
from `xml.dom.minidom.toprettyxml`. Both are reproduced here closely enough
that upstream's own byte-for-byte output comparisons still pass.

What it handles: elements, ordered attributes, character data, the five
predefined entities, numeric character references, `CDATA` sections,
comments, processing instructions and document type declarations.

What it does not: namespace resolution, DTD validation, entity declarations,
and the character data that follows a child element's closing tag, which
`ElementTree` calls a `tail`.
