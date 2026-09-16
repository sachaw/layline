#![cfg(feature = "emit")]

use std::path::{Path, PathBuf};

use layline_codegen::emit::{Change, Committed, Drift};

/// An empty temporary directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("layline-committed-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("a parent directory");
    }
    std::fs::write(path, contents).expect("write a fixture");
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read it back")
}

#[test]
fn check_passes_when_the_tree_agrees() {
    let dir = scratch("agrees");
    let file = dir.join("wire.rs");
    write(&file, "// @generated\n");

    Committed::by("cargo codegen").file(&file, "// @generated\n").check().expect("agrees");
}

#[test]
fn check_reports_a_stale_file_and_names_the_command() {
    let dir = scratch("stale");
    let file = dir.join("wire.rs");
    write(&file, "// @generated\npub struct Old;\n");

    let drifted = Committed::by("cargo codegen")
        .file(&file, "// @generated\npub struct New;\n")
        .check()
        .expect_err("a stale file is drift");

    assert_eq!(drifted.paths(), [(file, Drift::Stale)]);
    assert_eq!(drifted.command(), "cargo codegen");
    let shown = format!("{drifted:?}");
    assert!(shown.contains("Run `cargo codegen`"), "{shown}");
    assert!(shown.contains("stale"), "{shown}");
    assert!(shown.contains("It changes the wire format"), "{shown}");
}

#[test]
fn check_reports_a_missing_file() {
    let dir = scratch("missing");
    let file = dir.join("nested/wire.rs");

    let drifted = Committed::by("cargo codegen")
        .file(&file, "// @generated\n")
        .check()
        .expect_err("an absent file is drift");

    assert_eq!(drifted.paths(), [(file, Drift::Missing)]);
    assert!(format!("{drifted}").contains("missing"), "{drifted}");
}

#[test]
fn check_reports_a_module_the_spec_no_longer_defines() {
    let dir = scratch("orphan");
    write(&dir.join("msg_1.rs"), "// 1\n");
    write(&dir.join("msg_9.rs"), "// 9, dropped from the spec\n");

    let drifted = Committed::by("cargo codegen")
        .dir(&dir, [("msg_1.rs", "// 1\n")])
        .check()
        .expect_err("an orphaned module is drift");

    assert_eq!(drifted.paths(), [(dir.join("msg_9.rs"), Drift::Extra)]);
    assert!(format!("{drifted}").contains("extra"), "{drifted}");
}

#[test]
fn check_reports_every_disagreement_at_once() {
    let dir = scratch("all-three");
    write(&dir.join("a.rs"), "stale\n");
    write(&dir.join("c.rs"), "orphan\n");

    let drifted = Committed::by("cargo codegen")
        .dir(&dir, [("a.rs", "fresh\n"), ("b.rs", "new\n")])
        .check()
        .expect_err("three findings");

    assert_eq!(
        drifted.paths(),
        [
            (dir.join("a.rs"), Drift::Stale),
            (dir.join("b.rs"), Drift::Missing),
            (dir.join("c.rs"), Drift::Extra),
        ]
    );
    assert!(format!("{drifted}").starts_with("3 paths out of date"), "{drifted}");
}

#[test]
fn update_writes_a_missing_file_and_creates_its_parent() {
    let dir = scratch("create");
    let file = dir.join("src/generated/wire.rs");

    let changes =
        Committed::by("cargo codegen").file(&file, "// @generated\n").update().expect("write it");

    assert_eq!(changes, [Change::Wrote(file.clone())]);
    assert_eq!(read(&file), "// @generated\n");
}

#[test]
fn update_heals_a_stale_file() {
    let dir = scratch("heal");
    let file = dir.join("wire.rs");
    write(&file, "hand-edited\n");

    let committed = Committed::by("cargo codegen").file(&file, "// @generated\n");
    assert_eq!(committed.update().expect("heal"), [Change::Wrote(file.clone())]);
    assert_eq!(read(&file), "// @generated\n");
    committed.check().expect("healed");
}

#[test]
fn update_prunes_what_is_no_longer_emitted() {
    let dir = scratch("prune");
    write(&dir.join("msg_1.rs"), "// 1\n");
    write(&dir.join("msg_9.rs"), "// 9, dropped from the spec\n");

    let committed = Committed::by("cargo codegen").dir(&dir, [("msg_1.rs", "// 1\n")]);
    assert_eq!(committed.update().expect("prune"), [Change::Removed(dir.join("msg_9.rs"))]);
    assert!(!dir.join("msg_9.rs").exists());
    committed.check().expect("pruned");
}

#[test]
fn update_is_a_no_op_when_the_tree_agrees() {
    let dir = scratch("noop");
    let file = dir.join("wire.rs");
    write(&file, "// @generated\n");

    let changes = Committed::by("cargo codegen").file(&file, "// @generated\n").update();
    assert_eq!(changes.expect("no-op"), []);
}

#[test]
fn an_owned_directory_keeps_what_is_not_a_rust_file() {
    let dir = scratch("owned");
    write(&dir.join("msg_1.rs"), "// 1\n");
    write(&dir.join("README.md"), "notes\n");
    write(&dir.join("sub/other.rs"), "// somebody else's\n");

    let committed = Committed::by("cargo codegen").dir(&dir, [("msg_1.rs", "// 1\n")]);
    assert_eq!(committed.update().expect("no-op"), []);
    committed.check().expect("agrees");
    assert!(dir.join("README.md").exists());
    assert!(dir.join("sub/other.rs").exists());
}

#[test]
fn a_checkout_that_converted_line_endings_is_not_drift() {
    let dir = scratch("crlf");
    let file = dir.join("wire.rs");
    write(&file, "// @generated\r\npub struct A;\r\n");

    let committed = Committed::by("cargo codegen").file(&file, "// @generated\npub struct A;\n");
    committed.check().expect("line endings are not the wire format");
    assert_eq!(committed.update().expect("no-op"), []);
    assert_eq!(read(&file), "// @generated\r\npub struct A;\r\n");
}

#[test]
fn unrelated_uncommitted_changes_are_not_drift() {
    let dir = scratch("dirty");
    let file = dir.join("src/generated/wire.rs");
    write(&file, "// @generated\n");
    write(&dir.join("src/lib.rs"), "// half-finished refactor\n");
    write(&dir.join("codegen/src/spec.rs"), "// a spec edit in progress\n");

    Committed::by("cargo codegen")
        .file(&file, "// @generated\n")
        .check()
        .expect("only the artifact is checked");
}

#[test]
fn the_audit_artifact_is_guarded_beside_the_source() {
    use layline_codegen::Item;
    use layline_codegen::emit::{Module, generate};

    let dir = scratch("audit");
    let source = dir.join("src/generated/wire.rs");
    let artifact = dir.join("src/generated/wire.audit");
    let word = layline_codegen::LayoutDef::new(
        "Word",
        layline_codegen::Container::Bytes { bytes: 2, endian: layline_codegen::Endian::Be },
        vec![
            layline_codegen::Field::new(
                "hi",
                layline_codegen::Kind::Scalar(layline_codegen::Scalar::U(8)),
            ),
            layline_codegen::Field::new(
                "lo",
                layline_codegen::Kind::Scalar(layline_codegen::Scalar::U(8)),
            ),
        ],
    );
    let out = generate(
        &Module::new(vec![Item::Layout(word)]).with_doc(vec!["A tiny module, generated.".into()]),
    )
    .expect("a module emits");
    let committed = || {
        Committed::by("cargo run -p wire-codegen")
            .file(&source, out.source.clone())
            .file(&artifact, out.audit.clone())
    };

    let changes = committed().update().expect("writes");
    assert_eq!(changes.len(), 2, "{changes:?}");
    committed().check().expect("agrees after writing");
    assert!(read(&artifact).starts_with("L Word 16\n"), "{}", read(&artifact));

    write(&artifact, "L Word 16\nF Word 0 8 hi\n");
    let drifted = committed().check().expect_err("a stale artifact is drift");
    assert_eq!(drifted.paths(), [(artifact.clone(), Drift::Stale)]);
    assert!(drifted.to_string().contains("wire.audit"), "{drifted}");

    let changes = committed().update().expect("repairs");
    assert_eq!(changes, [Change::Wrote(artifact.clone())]);
    committed().check().expect("agrees again");
}
