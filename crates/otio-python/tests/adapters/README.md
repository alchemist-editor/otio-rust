# Upstream adapter suites

Each directory holds one upstream adapter repository's `tests/` files, copied
unmodified. [`run_adapter_tests.py`](../run_adapter_tests.py) runs them with
pytest, each in a scratch directory where the fixtures the Rust crate already
vendors sit beside the tests as `sample_data`, which is where they look.

| Directory | Upstream repository | Revision | Fixtures from |
| --- | --- | --- | --- |
| `ale` | `OpenTimelineIO/otio-ale-adapter` | `70c647d36468a423ff1815b761c92f2ea138638c` | `crates/otio-ale/tests/data` |
| `cmx_3600` | `OpenTimelineIO/otio-cmx3600-adapter` | `4b02a6668b4ddb435bd6b8e7459e1dbd170938df` | `crates/otio-cmx3600/tests/data` |
| `fcp_xml` | `OpenTimelineIO/otio-fcp-adapter` | `63824d5ca04dbda81bcced89c9fde6ab0b7e6e36` | `crates/otio-fcp7/tests/data` |
| `fcpx_xml` | `OpenTimelineIO/otio-fcpx-xml-adapter` | `c61839e105fb035589d3ec704b1266a22089b425` | `crates/otio-fcpx/tests/data` |

As with [`../upstream`](../upstream), do not edit them. A test that fails is a
statement about the port. One that cannot pass because it tests something this
port deliberately does not have is left out in the runner, with the reason
beside it.

Two suites import the upstream adapter package by name; [`shims`](shims)
answers those imports with this package's own modules.
