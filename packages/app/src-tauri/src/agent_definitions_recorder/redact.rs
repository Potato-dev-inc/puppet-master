//! The recorder's redaction is now the app-wide one in `crate::redact` (REC-3 still holds: raw text
//! never reaches a snapshot). This file keeps the old path working for the recorder and its tests.

pub use crate::redact::rules::{redact, secret_env_values, RedactContext, MASK};

#[cfg(test)]
pub(super) use crate::redact::rules::test_ctx;
