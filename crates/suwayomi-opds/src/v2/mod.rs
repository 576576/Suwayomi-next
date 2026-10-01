//! OPDS 2.0 (`application/opds+json`) catalog feeds plus Readium Divina
//! manifests for chapters.
//!
//! Sibling of the 1.2 module, not a replacement: the 1.2 Atom feeds keep
//! rendering byte-for-byte identically (`docs/agent/plans/opds-v2.md` §9.4).
//! Only the serialisation layer differs; `crate::repository` supplies the same
//! rows to both encodings.

pub mod feeds;
pub mod json;
pub mod model;
pub mod router;
