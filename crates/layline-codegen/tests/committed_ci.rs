//! This test sets the `CI` environment variable, so it runs alone in its own binary.

#![cfg(feature = "emit")]

use layline_codegen::emit::Committed;

#[test]
fn the_hint_says_commit_the_result_when_ci_is_set() {
    let dir = std::env::temp_dir().join(format!("layline-committed-ci-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let file = dir.join("wire.rs");
    std::fs::write(&file, "stale\n").expect("write a fixture");

    let committed = Committed::by("cargo codegen").file(&file, "fresh\n");

    // SAFETY: this is the only test in the binary, so no other thread reads the environment.
    unsafe { std::env::remove_var("CI") };
    let local = committed.check().expect_err("stale").to_string();
    assert!(local.contains("Run `cargo codegen`"), "{local}");
    assert!(!local.contains("NOTE"), "no CI note outside CI:\n{local}");

    unsafe { std::env::set_var("CI", "1") };
    let in_ci = committed.check().expect_err("stale").to_string();
    assert!(
        in_ci.contains("NOTE: run `cargo codegen` locally and commit the updated files."),
        "{in_ci}"
    );

    unsafe { std::env::remove_var("CI") };
}
