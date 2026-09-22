//! The handful of names the conventions get grammatically wrong.
//!
//! Almost every entry point falls out of the rules in `classify` reading the
//! way a person would have written it by hand. A few do not:
//! `otio_rational_time_duration_from_start_end_time` takes two times and
//! belongs to neither, so making the first one the receiver produces
//! `start.durationFromStartEndTime(end)`, which is nonsense in any language.
//!
//! This table fixes those, and only those. An entry may change what a
//! function is *called* and whether it hangs off a receiver. It may not
//! change what the function takes, what it returns, or what it does — those
//! come from the source and nowhere else, which is what keeps a generated SDK
//! a description of the library rather than an opinion about it.
//!
//! Every entry must name a symbol that exists. One that does not is an error,
//! so an override cannot outlive the function it was written for.

use crate::model::{Group, Role};
use crate::scan::{ScanError, Scanned};

/// One correction.
struct Correction {
    /// The exported symbol it applies to.
    symbol: &'static str,
    /// What to call it instead, in `snake_case`.
    name: &'static str,
    /// Whether to detach it from its receiver and make it a plain function of
    /// its group.
    detach: bool,
    /// Why, for whoever reads this next.
    why: &'static str,
}

/// The corrections, in symbol order.
const CORRECTIONS: &[Correction] = &[
    Correction {
        symbol: "otio_node_kind",
        name: "schema_kind",
        detach: false,
        why: "A track's `kind` is video or audio. This one is which schema \
              the object is, so it says so, beside `schema_name` and \
              `schema_version`.",
    },
    Correction {
        symbol: "otio_generator_reference_kind",
        name: "generator_kind",
        detach: false,
        why: "What upstream calls it, and it does not then collide with the \
              schema kind every object has.",
    },
    Correction {
        symbol: "otio_generator_reference_set_kind",
        name: "set_generator_kind",
        detach: false,
        why: "As above, so the pair still reads as a pair.",
    },
    Correction {
        symbol: "otio_document_read_from_file",
        name: "read_otio_file",
        detach: false,
        why: "It reads a `.otio` file, where the adapter call of nearly the \
              same name reads any format it is told to.",
    },
    Correction {
        symbol: "otio_document_write_to_file",
        name: "write_otio_file",
        detach: false,
        why: "As above.",
    },
    Correction {
        symbol: "otio_document_remove",
        name: "remove_node",
        detach: false,
        why: "It takes an object out of the document, where the edit \
              operation of the same name takes whatever sits at an instant \
              out of a composition.",
    },
    Correction {
        symbol: "otio_document_remove_recursive",
        name: "remove_node_recursive",
        detach: false,
        why: "As above.",
    },
    Correction {
        symbol: "otio_rational_time_duration_from_start_end_time",
        name: "duration_from_start_end_time",
        detach: true,
        why: "It takes a start and an end and belongs to neither of them.",
    },
    Correction {
        symbol: "otio_rational_time_duration_from_start_end_time_inclusive",
        name: "duration_from_start_end_time_inclusive",
        detach: true,
        why: "As above.",
    },
    Correction {
        symbol: "otio_node_equal",
        name: "equals",
        detach: false,
        why: "`a.equal(b)` is not a sentence; `a.equals(b)` is.",
    },
    Correction {
        symbol: "otio_rational_time_equal",
        name: "equals",
        detach: false,
        why: "As above.",
    },
    Correction {
        symbol: "otio_rational_time_strictly_equal",
        name: "strictly_equals",
        detach: false,
        why: "As above.",
    },
    Correction {
        symbol: "otio_rational_time_almost_equal",
        name: "almost_equals",
        detach: false,
        why: "As above.",
    },
];

/// Applies the correction for a symbol, if there is one.
#[must_use]
pub fn apply(symbol: &str, name: String, role: Role) -> (String, Role) {
    let Some(correction) = CORRECTIONS
        .iter()
        .find(|correction| correction.symbol == symbol)
    else {
        return (name, role);
    };
    let role = if correction.detach { Role::Free } else { role };
    (correction.name.to_string(), role)
}

/// Checks that every correction still names a function that exists.
///
/// # Errors
///
/// Fails naming any correction whose symbol the C ABI no longer exports.
pub fn check(groups: &[Group]) -> Scanned<()> {
    let stale: Vec<&str> = CORRECTIONS
        .iter()
        .filter(|correction| {
            !groups.iter().any(|group| {
                group
                    .functions
                    .iter()
                    .any(|function| function.symbol == correction.symbol)
            })
        })
        .map(|correction| correction.symbol)
        .collect();
    if stale.is_empty() {
        return Ok(());
    }
    Err(ScanError {
        location: "crates/otio-sdk-model/src/overrides.rs".to_string(),
        message: format!(
            "these corrections name functions the C ABI no longer exports: {stale:?}. Remove them."
        ),
    })
}

/// Why each correction exists, for the generated documentation of the
/// pipeline itself.
#[must_use]
pub fn reasons() -> Vec<(&'static str, &'static str)> {
    CORRECTIONS
        .iter()
        .map(|correction| (correction.symbol, correction.why))
        .collect()
}

/// Whether a correction takes a function off its receiver.
#[must_use]
pub fn detaches(symbol: &str) -> bool {
    CORRECTIONS
        .iter()
        .any(|correction| correction.symbol == symbol && correction.detach)
}
