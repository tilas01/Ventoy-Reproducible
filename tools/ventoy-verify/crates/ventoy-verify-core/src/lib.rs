// SPDX-License-Identifier: GPL-3.0-or-later
//
// The core of `ventoy-verify`: hash a file, read a manifest, say whether the
// two agree.
//
// # What this crate refuses to do
//
// It does not print, it does not exit, it does not open a network connection
// and it does not know what a signature is. Every one of those lives in another
// crate, so that the answer to "what can this code touch" is short enough to
// read in one sitting.
//
// `forbid(unsafe_code)` rather than `deny`, so that no module further down can
// quietly opt back in with an inner attribute.

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    unreachable_pub,
    clippy::all,
    clippy::pedantic
)]
// Verification code reads better with explicit early returns than with the
// combinator chains clippy suggests, and the manifest types are plain data.
#![allow(clippy::module_name_repetitions)]

//! Hashing, manifest handling and verdicts for `ventoy-verify`.
//!
//! The entry points most callers want are [`Manifest::from_json`],
//! [`digest::hash_file`] and [`verify::verify_manifest`].

pub mod digest;
pub mod error;
pub mod manifest;
pub mod report;
pub mod verify;

pub use error::{Error, Result};
pub use manifest::{Entry, Manifest, Origin, Verdict};
pub use report::{Outcome, Report, Summary};
