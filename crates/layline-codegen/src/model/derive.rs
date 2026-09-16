//! Derives written on generated types.

use alloc::string::String;

/// A derive written on a generated struct or enum.
///
/// A bare path derives unconditionally. [`with_cfg`](Self::with_cfg) puts the derive behind a
/// `cfg` predicate, which is how an optional dependency reaches generated code.
///
/// `Module::with_derives` sets them for every item; each item's own `with_derives` adds to that.
///
/// ```
/// use layline_codegen::Derive;
///
/// let always: Derive = "Copy".into();
/// let gated = Derive::new("serde::Serialize").with_cfg("feature = \"serde\"");
/// assert_eq!(always.cfg, None);
/// assert_eq!(gated.path, "serde::Serialize");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Derive {
    /// The derive path, such as `serde::Serialize`.
    pub path: String,
    /// The `cfg` predicate the derive sits behind, such as `feature = "serde"`.
    pub cfg: Option<String>,
}

impl Derive {
    /// An unconditional derive of `path`.
    #[must_use]
    pub fn new(path: &str) -> Self {
        Self { path: String::from(path), cfg: None }
    }

    /// Sets [`cfg`](Self::cfg).
    #[must_use]
    pub fn with_cfg(self, cfg: &str) -> Self {
        Self { cfg: Some(String::from(cfg)), ..self }
    }
}

impl From<&str> for Derive {
    fn from(path: &str) -> Self {
        Self::new(path)
    }
}

impl From<String> for Derive {
    fn from(path: String) -> Self {
        Self { path, cfg: None }
    }
}
