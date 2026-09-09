// SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Signature failures, told apart from one another.
//!
//! "Signature verification failed" is a useless message, because the four
//! things it can mean want four different responses from the reader. A bad
//! signature means do not run this file. An unknown issuer means you have the
//! wrong key, or somebody else signed it. A malformed armour usually means a
//! truncated download. Each gets its own variant and its own sentence.

use std::path::PathBuf;

/// The result type used throughout this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong while checking a signature.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A key or signature file could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// The public key file was not a valid armoured OpenPGP key.
    #[error("{path} is not a valid armoured OpenPGP public key: {reason}")]
    BadKey {
        /// The file that failed to parse.
        path: PathBuf,
        /// What the parser said.
        reason: String,
    },

    /// The signature file was not a valid armoured detached signature.
    ///
    /// In practice this is nearly always a truncated or HTML-wrapped download
    /// rather than an attack, and the message says so.
    #[error(
        "{path} is not a valid armoured OpenPGP signature: {reason}. \
         A truncated download is the usual cause; fetch it again."
    )]
    BadSignature {
        /// The file that failed to parse.
        path: PathBuf,
        /// What the parser said.
        reason: String,
    },

    /// The signature was made by a key that is not the one supplied.
    ///
    /// This is the variant that matters most. It is the difference between "the
    /// file is authentic" and "the file is signed by somebody, and it was not
    /// the maintainer".
    #[error(
        "the signature was made by key {issuer}, which is not {expected}. \
         This file was not signed by the key you supplied."
    )]
    WrongKey {
        /// The issuer named in the signature.
        issuer: String,
        /// The fingerprint of the key that was supplied.
        expected: String,
    },

    /// The signature is cryptographically invalid for this content.
    #[error(
        "the signature does not match the contents of {path}. \
         Do not use this file."
    )]
    Invalid {
        /// The file whose signature failed.
        path: PathBuf,
    },

    /// The signed input was larger than this program will hold in memory.
    ///
    /// Only the manifest is ever signed and it is measured in kilobytes, so a
    /// signed input of tens of megabytes means something is wrong rather than
    /// something is large.
    #[error(
        "{path} is {size} bytes, larger than the {limit} byte limit for signed \
         input; only the manifest is meant to be signed"
    )]
    TooLarge {
        /// The oversized file.
        path: PathBuf,
        /// Its size.
        size: u64,
        /// The limit.
        limit: u64,
    },
}
