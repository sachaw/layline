//! Every attribute the derives accept must come from the model or be listed with a reason.
//!
//! The attribute lists are read from `layline-derive`'s source, so they never go stale.
#![cfg(feature = "walk")]

use std::path::PathBuf;

use layline_codegen::__derive::{field_rows, layout_parts};
use layline_codegen::Root;
use layline_codegen::{Container, Endian, Field, Kind, LayoutDef, Range, Scalar, Stated};

/// `Layout` attributes the model never produces, and why.
const DERIVE_ONLY: &[(&str, &str)] = &[
    ("overlay", "adds an accessor over existing bits and does not change the bytes"),
    ("count", WALK_ONLY),
    ("len", WALK_ONLY),
    ("stride", WALK_ONLY),
    ("fill", WALK_ONLY),
    ("seek", WALK_ONLY),
    ("text", WALK_ONLY),
    ("until", WALK_ONLY),
    ("var", WALK_ONLY),
    ("message", WALK_ONLY),
    ("switch", WALK_ONLY),
    ("when", WALK_ONLY),
];

const WALK_ONLY: &str = "`Layout` accepts it only to reject it with a clear error. \
                         It belongs on a `Message`, where the model does produce it";

fn published() -> Vec<String> {
    published_by("Layout,")
}

fn derive_src() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../layline-derive/src/lib.rs");
    std::fs::read_to_string(&path).expect("read the derive's source")
}

fn published_by(derive: &str) -> Vec<String> {
    let src = derive_src();
    // Find the derive by name, allowing for rustfmt splitting the attribute across lines.
    let at = src
        .match_indices("proc_macro_derive(")
        .find(|(i, _)| src[*i..(*i + 64).min(src.len())].contains(derive))
        .map(|(i, _)| i)
        .expect("the derive");
    let open =
        src[at..].find("attributes(").expect("its attribute list") + at + "attributes(".len();
    let close = src[open..].find(')').expect("a closing bracket") + open;
    src[open..close]
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Matches `name` as rendered by `TokenStream`, e.g. `# [at (byte = 0)]` or `# [fill]`.
/// The `# [` prefix keeps a field named `count` from matching.
fn renders(rendered: &str, name: &str) -> bool {
    rendered.contains(&format!("# [{name} "))
        || rendered.contains(&format!("# [{name}("))
        || rendered.contains(&format!("# [{name}]"))
}

/// Layouts that use every model field that renders as an attribute.
fn every_spelling() -> Vec<LayoutDef> {
    vec![
        LayoutDef::new(
            "Bits",
            Container::word(32, Endian::Be, layline_codegen::BitOrder::Lsb),
            vec![
                Field::new("tag", Kind::Scalar(Scalar::U(8))).with_stated(Stated::bit(0)),
                Field::new("rest", Kind::Scalar(Scalar::U(24))),
            ],
        ),
        LayoutDef::new(
            "Bytes",
            Container::Bytes { bytes: 11, endian: Endian::Le },
            vec![
                Field::new("level", Kind::Codec { ty: "Level".into(), bits: 8 }),
                Field::new("sig", Kind::array(Scalar::U(8), 2))
                    .with_stated(Stated::byte(1))
                    .with_magic(&[0xA1, 0xB2]),
                Field::new("head", Kind::Nested { ty: "Head".into(), bytes: 4 }),
                Field::new("count", Kind::Scalar(Scalar::U(16)))
                    .with_range(Range::new(None, Some(255))),
                Field::new(
                    "crc",
                    Kind::Checksum {
                        repr: Scalar::U(16),
                        algorithm: "::wire::Crc16Ccitt".into(),
                        over: layline_codegen::Coverage::whole(),
                    },
                ),
            ],
        ),
    ]
}

#[test]
fn every_published_attribute_has_a_model_fact_or_a_reason() {
    let rendered: String = every_spelling()
        .iter()
        .map(|l| {
            let parts = layout_parts(l, &[], &Root::default()).expect("renders");
            format!("{} {}", parts.attrs, parts.fields)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut unaccounted = Vec::new();
    for name in published() {
        if DERIVE_ONLY.iter().any(|(n, _)| *n == name) {
            continue;
        }
        if !renders(&rendered, &name) {
            unaccounted.push(name);
        }
    }

    assert!(
        unaccounted.is_empty(),
        "the `Layout` derive accepts {unaccounted:?}, but the model never renders them.\n\
         Add them to the model, or list them in `DERIVE_ONLY` with a reason.\n\
         Rendered:\n{rendered}"
    );
}

#[test]
fn every_derive_only_attribute_is_still_published() {
    let published = published();
    for (name, _) in DERIVE_ONLY {
        assert!(
            published.contains(&(*name).to_string()),
            "the derive no longer accepts `{name}`. Remove it from DERIVE_ONLY"
        );
    }
}

#[test]
fn no_derive_only_attribute_is_produced_by_a_model_fact() {
    let rendered: String = every_spelling()
        .iter()
        .map(|l| {
            let parts = layout_parts(l, &[], &Root::default()).expect("renders");
            format!("{} {}", parts.attrs, parts.fields)
        })
        .collect::<Vec<_>>()
        .join("\n");

    for (name, why) in DERIVE_ONLY {
        assert!(
            !renders(&rendered, name),
            "`{name}` is in DERIVE_ONLY, but the model now renders it.\n\
             Listed reason: {why}\n\
             Remove it from DERIVE_ONLY, or stop the renderer emitting it."
        );
    }
}

#[test]
fn a_nested_array_renders_the_type_and_the_rows() {
    let matrix = LayoutDef::new(
        "Cov",
        Container::Bytes { bytes: 72, endian: Endian::Be },
        vec![Field::new("m", Kind::Array(Scalar::F64, vec![3, 3]))],
    );
    let parts = layout_parts(&matrix, &[], &Root::default()).expect("renders");
    let rendered = parts.fields.to_string();
    assert!(rendered.contains("[[f64 ; 3] ; 3]"), "unexpected type in:\n{rendered}");

    let rows = field_rows(&matrix.fields, &matrix.container);
    assert_eq!(rows.len(), 9);
    assert_eq!(rows[0].name, "m[0][0]");
    assert_eq!(rows[5].name, "m[1][2]");
    assert_eq!(rows[8].name, "m[2][2]");
    assert_eq!(rows[5].extent.start(), 5 * 64, "element offsets are flat");
}

/// `Message` attributes the derive accepts but never reads, and why.
///
/// An accepted attribute that is never read compiles and silently does nothing.
const MESSAGE_DERIVE_ONLY: &[(&str, &str)] = &[];

#[test]
fn every_attribute_the_message_derive_publishes_is_read() {
    // Scan `message/` and `attr.rs` only. Including `layout/` would hide attributes `Message` ignores.
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../layline-derive/src");
    let mut files: Vec<PathBuf> = std::fs::read_dir(src.join("message"))
        .expect("read the message derive source")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "rs"))
        .collect();
    files.push(src.join("attr.rs"));
    let front_end: String = files
        .iter()
        .map(|f| std::fs::read_to_string(f).expect("read the message derive source"))
        .collect();

    let mut inert = Vec::new();
    for name in published_by("Message,") {
        if MESSAGE_DERIVE_ONLY.iter().any(|(n, _)| *n == name) {
            continue;
        }
        // The message derive reads attributes through `is_ident` or `single`.
        let read = [format!("is_ident(\"{name}\")"), format!("single(&v.attrs, \"{name}\"")];
        if !read.iter().any(|r| front_end.contains(r)) {
            inert.push(name);
        }
    }

    assert!(
        inert.is_empty(),
        "the `Message` derive accepts {inert:?} but never reads them, so they are silently ignored.\n\
         Read them in `parse_attrs`, or list them in `MESSAGE_DERIVE_ONLY` with a reason."
    );
}
