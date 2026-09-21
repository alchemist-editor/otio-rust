//! The bits of path handling the format needs.
//!
//! An EDL carries media paths written on whatever machine produced it, so a
//! file read on Linux routinely holds Windows paths and the other way round.
//! These helpers are deliberately POSIX-only — `/` separates, `\` does not —
//! which is what upstream does when its tests run, and it means this adapter
//! produces the same output on every platform rather than output that depends
//! on where it ran.

/// Rewrites Windows separators as POSIX ones.
#[must_use]
pub fn flip_windows_slashes(path: &str) -> String {
    path.replace('\\', "/")
}

/// Returns the part of a path after the last `/`.
#[must_use]
pub fn basename(path: &str) -> &str {
    match path.rfind('/') {
        Some(at) => &path[at + 1..],
        None => path,
    }
}

/// Returns the part of a path up to the last `/`, without the separator.
///
/// A path with no separator has no directory, so this is empty.
#[must_use]
pub fn dirname(path: &str) -> &str {
    match path.rfind('/') {
        Some(at) => &path[..at],
        None => "",
    }
}

/// Returns a filename without its extension.
///
/// This is Python's `os.path.splitext`, so a leading dot is part of the name
/// rather than an extension: `.profile` keeps its dot.
#[must_use]
pub fn strip_extension(name: &str) -> &str {
    match name.rfind('.') {
        Some(0) | None => name,
        Some(at) => &name[..at],
    }
}

/// Returns a reel name without a trailing alphabetic extension.
///
/// Stricter than [`strip_extension`]: `clip.001` keeps its `.001`, because a
/// numbered part of a name is not an extension. This is what upstream strips
/// from a reel name, and it differs from what it strips from a clip name.
#[must_use]
pub fn strip_alphabetic_extension(name: &str) -> &str {
    let Some(at) = name.rfind('.') else {
        return name;
    };
    let extension = &name[at + 1..];
    if !extension.is_empty() && extension.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        &name[..at]
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::{basename, dirname, strip_alphabetic_extension, strip_extension};

    #[test]
    fn splits_a_posix_path() {
        assert_eq!(basename("/var/tmp/clip.mov"), "clip.mov");
        assert_eq!(dirname("/var/tmp/clip.mov"), "/var/tmp");
        assert_eq!(basename("clip.mov"), "clip.mov");
        assert_eq!(dirname("clip.mov"), "");
    }

    #[test]
    fn a_windows_path_is_one_name_until_it_is_flipped() {
        // Upstream's own tests rely on this: a reel taken from an unflipped
        // Windows path is the whole path.
        assert_eq!(basename(r"S:\path\to\take.exr"), r"S:\path\to\take.exr");
    }

    #[test]
    fn strips_extensions_two_different_ways() {
        assert_eq!(strip_extension("clip.001.exr"), "clip.001");
        assert_eq!(strip_alphabetic_extension("clip.001.exr"), "clip.001");
        // A numbered tail is not an extension to a reel name.
        assert_eq!(strip_extension("clip.001"), "clip");
        assert_eq!(strip_alphabetic_extension("clip.001"), "clip.001");
    }
}
