//! Process-level tests for how a refused configuration file reaches an
//! operator.
//!
//! The refusal's wording is unit-tested in `src/config/mod.rs`, but the text an
//! operator actually reads is chosen by the process at exit: a `main` that
//! returns `Result` lets `Termination` print the error's `Debug` rendering, so
//! a unit test can call `Display` all it likes and never notice. F-177-5 and
//! F-177-6 are about what the *process* prints, so these run the built binary.
//!
//! Every fixture here is refused before the listener binds, so each run exits
//! instead of serving.

use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

/// The binary under test, as Cargo built it for this test run.
const BINARY: &str = env!("CARGO_BIN_EXE_tucano-test");

/// Runs the binary with `TUCANO_CONFIG_FILE` pointed at `path`.
///
/// The data directory is a temporary one too, so a run that somehow did reach
/// the repository would write where the test owns the files. The key-file
/// variable is removed rather than inherited: the key ring is loaded *before*
/// the configuration file, and a stray value in the test runner's environment
/// would replace the refusal under test with a key-ring one.
fn run_with_config(path: &Path, data_dir: &Path) -> Output {
    Command::new(BINARY)
        .env("TUCANO_CONFIG_FILE", path)
        .env("TUCANO_DATA_DIR", data_dir)
        .env("TUCANO_LOG", "error")
        .env_remove("TUCANO_CONFIG_KEY_FILE")
        .output()
        .expect("the built binary should be runnable")
}

/// Writes `body` to `config.json` inside `directory` and returns its path.
fn write_config(directory: &TempDir, body: &str) -> std::path::PathBuf {
    let path = directory.path().join("config.json");
    std::fs::write(&path, body).expect("write the fixture");
    path
}

#[test]
fn a_missing_configuration_file_names_the_setting_without_the_raw_path() {
    let directory = TempDir::new().expect("temp dir");
    let missing = directory.path().join("absent-config.json");

    let output = run_with_config(&missing, directory.path());
    assert!(
        !output.status.success(),
        "a missing file must refuse startup"
    );
    assert!(output.stdout.is_empty(), "the refusal belongs on stderr");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("TUCANO_CONFIG_FILE"), "{stderr}");
    assert!(
        !stderr.contains("absent-config.json"),
        "must not echo the raw path: {stderr}"
    );
    assert!(
        !stderr.contains("UnreadableFile"),
        "must not print the Debug rendering: {stderr}"
    );
}

#[test]
fn a_wrong_typed_setting_names_the_setting_and_never_the_value() {
    // F-177-6's own reproduction: a sentinel string in a field that has to be a
    // boolean. serde's type error quotes that string, which is how the value
    // reached the startup log before the shape check existed.
    const SENTINEL: &str = "SENTINEL-audit-177-wrong-type";
    let directory = TempDir::new().expect("temp dir");
    let path = write_config(
        &directory,
        &format!(r#"{{"version": 1, "auth_required": "{SENTINEL}"}}"#),
    );

    let output = run_with_config(&path, directory.path());
    assert!(!output.status.success(), "a wrong type must refuse startup");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("TUCANO_CONFIG_FILE"), "{stderr}");
    assert!(stderr.contains("auth_required"), "{stderr}");
    assert!(
        !stderr.contains(SENTINEL),
        "must never echo the value it refused: {stderr}"
    );
    assert!(
        !stderr.contains("Malformed {"),
        "must not print the Debug rendering: {stderr}"
    );
}

#[test]
fn a_wrong_typed_jwt_secret_names_the_setting_and_never_the_value() {
    // The same refusal with the sentinel held inside the secret field itself:
    // the message names `jwt_secret` and never the contents of the value it
    // refused.
    const SENTINEL: &str = "SENTINEL-audit-177-wrong-type";
    let directory = TempDir::new().expect("temp dir");
    let path = write_config(
        &directory,
        &format!(r#"{{"version": 1, "jwt_secret": ["{SENTINEL}"]}}"#),
    );

    let output = run_with_config(&path, directory.path());
    assert!(!output.status.success(), "a wrong type must refuse startup");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("TUCANO_CONFIG_FILE"), "{stderr}");
    assert!(stderr.contains("jwt_secret"), "{stderr}");
    assert!(
        !stderr.contains(SENTINEL),
        "must never echo the value inside the refused field: {stderr}"
    );
}
