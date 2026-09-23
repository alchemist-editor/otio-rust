//! Compiles `tests/abi.c` against the built library and runs it.
//!
//! This is the only check that actually proves the ABI. Everything else in
//! this crate is Rust talking to Rust: the header could describe a different
//! interface, the structs could have a different layout, and a symbol could
//! be missing, and no Rust test would notice. A C compiler, a linker and a
//! running program notice all three.
//!
//! The program is built against the shared library with an rpath, so it finds
//! the library at run time without the caller setting anything. If no C
//! compiler is on the path the test explains itself and stops, unless
//! `OTIO_CAPI_REQUIRE_CC` is set, which CI does so that a runner without a
//! compiler is a failure rather than a silent pass.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Where `cargo` put `libotio`.
///
/// The test binary lives in `target/<profile>/deps/`, and the library it was
/// linked against sits one directory up.
fn artifact_dir() -> PathBuf {
    let mut path = std::env::current_exe().expect("a test knows its own path");
    path.pop(); // the test binary
    if path.ends_with("deps") {
        path.pop();
    }
    path
}

/// Finds a C compiler, preferring whatever `CC` names.
///
/// On Windows nothing is searched for. A runner there has `clang` on the path
/// but not the MSVC environment `clang` needs to link, so guessing produces a
/// failure that says nothing about this library. Set `CC` from inside a
/// developer command prompt to run it there; the CI job that requires a
/// compiler runs on Linux and macOS.
fn compiler() -> Option<String> {
    if let Ok(from_env) = std::env::var("CC") {
        if !from_env.is_empty() {
            return Some(from_env);
        }
    }
    if cfg!(windows) {
        return None;
    }
    ["cc", "clang", "gcc"]
        .iter()
        .find(|candidate| runs(candidate))
        .map(|candidate| (*candidate).to_string())
}

/// Returns whether a command exists and can be started.
fn runs(program: &str) -> bool {
    // `cl` writes its banner and returns 0 with no arguments; the others want
    // a flag. Either way, only "could it be started" is being asked.
    let mut command = Command::new(program);
    if program != "cl" {
        command.arg("--version");
    }
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// Builds the C program, returning where it was written.
fn build(compiler: &str, source: &Path, include: &Path, artifacts: &Path) -> PathBuf {
    let out_dir = artifact_dir().join("c-abi");
    std::fs::create_dir_all(&out_dir).expect("the output directory can be created");

    let mut command = Command::new(compiler);
    let program;

    if compiler.ends_with("cl") {
        // MSVC: the static library links without an rpath, and the import
        // library for the DLL would need the DLL beside the executable.
        program = out_dir.join("abi.exe");
        command
            .arg(source)
            .arg(format!("/I{}", include.display()))
            .arg("/nologo")
            .arg("/W3")
            .arg(format!("/Fe:{}", program.display()))
            .arg(format!("/Fo:{}\\", out_dir.display()))
            .arg("/link")
            .arg(artifacts.join("otio.lib"))
            .arg("ws2_32.lib")
            .arg("userenv.lib")
            .arg("ntdll.lib")
            .arg("bcrypt.lib")
            .arg("advapi32.lib");
    } else {
        program = out_dir.join(if cfg!(windows) { "abi.exe" } else { "abi" });
        command
            .arg("-std=c11")
            .arg("-Wall")
            .arg("-Wextra")
            .arg("-Werror")
            .arg("-I")
            .arg(include)
            .arg(source)
            .arg("-o")
            .arg(&program)
            .arg("-L")
            .arg(artifacts)
            .arg("-lotio");
        if !cfg!(windows) {
            command.arg(format!("-Wl,-rpath,{}", artifacts.display()));
        }
    }

    let output = command.output().expect("the C compiler can be started");
    assert!(
        output.status.success(),
        "compiling tests/abi.c failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    program
}

#[test]
fn a_c_program_links_against_the_library_and_uses_it() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = manifest.join("tests/abi.c");
    let include = manifest.join("include");
    let artifacts = artifact_dir();

    let required = std::env::var_os("OTIO_CAPI_REQUIRE_CC").is_some();
    let Some(compiler) = compiler() else {
        assert!(
            !required,
            "OTIO_CAPI_REQUIRE_CC is set and no C compiler was found; \
             set CC, or install one"
        );
        eprintln!(
            "no C compiler found, so the ABI is not being proven here. \
             Set CC to one, or run the C ABI job in CI, which does."
        );
        return;
    };

    // `cargo test` builds the crate's rlib for the test harness to link
    // against; the shared library it also declares is built by `cargo build`.
    // Saying so beats failing at link time with a bare "cannot find -lotio".
    let built = artifacts.read_dir().is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("libotio.") || name.starts_with("otio.")
        })
    });
    assert!(
        built,
        "no libotio in {}; run `cargo build -p otio-capi` first",
        artifacts.display()
    );

    let program = build(&compiler, &source, &include, &artifacts);

    // The bundle checks write files, so the program is lent an empty
    // directory to write them in, emptied again on every run.
    let scratch = artifact_dir().join("c-abi").join("scratch");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("the scratch directory can be created");

    let output = Command::new(&program)
        .env("OTIO_ABI_SCRATCH", &scratch)
        .output()
        .expect("the C program can be run");
    print!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "tests/abi.c reported failures:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
