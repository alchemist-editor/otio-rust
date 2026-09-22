# Import shims

Upstream's adapter suites import the upstream adapter packages by name —
`otio_cmx3600_adapter.cmx_3600`, `otio_fcpx_xml_adapter.fcpx_xml` — to reach
a module-level helper or to check that the adapter OpenTimelineIO found is the
one under test. Here those modules are `opentimelineio.adapters.cmx_3600` and
`opentimelineio.adapters.fcpx_xml`, so each shim registers the real module
under the upstream name. They are aliases, not copies: the conftest's check
that `module_abs_path()` is that module's `__file__` compares the same file.

The runner puts this directory on `sys.path`; nothing installs it.
