//! Rendering values as the text formats spell them.

/// Renders a number the way an interchange file writes one.
///
/// A whole number keeps a trailing `.0`. That looks like a detail and is not:
/// these formats were defined by what Python and Avid happened to print, and
/// files are diffed against those tools' output. An ALE heading states its
/// rate as `24.0` when a caller passed one, and an EDL writes `*ASC_SAT 1.0`
/// rather than `*ASC_SAT 1`.
///
/// Everything else is the shortest spelling that reads back as the same
/// number, which is what both Rust and Python produce.
#[must_use]
pub fn float(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e16 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::float;

    #[test]
    fn a_whole_number_keeps_its_point() {
        assert_eq!(float(24.0), "24.0");
        assert_eq!(float(0.0), "0.0");
        assert_eq!(float(-1.0), "-1.0");
    }

    #[test]
    fn anything_else_is_written_as_briefly_as_it_reads_back() {
        assert_eq!(float(23.976), "23.976");
        assert_eq!(float(-0.0122), "-0.0122");
    }
}
