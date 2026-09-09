// SPDX-License-Identifier: GPL-3.0-or-later
//!
//! One error type for the crate, and no panics.
//!
//! A verification tool that aborts on a malformed input has failed at its one
//! job: the malformed input is exactly the case it exists to notice. So every
//! fallible path returns `Result` and every message names the file it was
//! reading when it gave up, because "invalid JSON" without a path is a bug
//! report nobody can act on.

use std::path::PathBuf;

/// The result type used throughout this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong while hashing or verifying.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A file named by the manifest could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// A manifest could not be parsed as JSON, or did not match the schema.
    #[error("{path} is not a valid manifest: {source}")]
    Manifest {
        /// The manifest that failed to parse.
        path: PathBuf,
        /// The underlying serde failure.
        #[source]
        source: serde_json::Error,
    },

    /// A digest in the manifest was not valid hexadecimal of the right length.
    #[error("{path} records a malformed {algorithm} digest: {reason}")]
    MalformedDigest {
        /// The entry that carried the bad digest.
        path: PathBuf,
        /// Which digest field was malformed.
        algorithm: &'static str,
        /// What was wrong with it.
        reason: String,
    },

    /// The manifest declared a schema version this build does not understand.
    ///
    /// Refusing an unknown version is deliberate. A newer manifest may add a
    /// field whose absence changes a verdict, and silently ignoring it would
    /// report a pass that was never checked.
    #[error(
        "manifest schema version {found} is newer than this build understands \
         (supported: {supported}); upgrade ventoy-verify"
    )]
    UnsupportedSchema {
        /// The version found in the file.
        found: u32,
        /// The newest version this build supports.
        supported: u32,
    },

    /// The manifest was internally inconsistent.
    #[error("manifest is inconsistent: {0}")]
    Inconsistent(String),

    /// A path in the manifest tried to escape the root it was resolved against.
    ///
    /// Manifests are downloaded from a release page, which makes them untrusted
    /// input. A path of `../../../etc/passwd` must not become a read outside the
    /// tree the user pointed at.
    #[error("{0} escapes the verification root, which is not allowed")]
    PathEscape(PathBuf),

    /// Serialising a report or manifest failed.
    #[error("cannot serialise: {0}")]
    Serialise(#[from] serde_json::Error),
}
