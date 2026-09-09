// SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Detached OpenPGP signature verification, in pure Rust.
//!
//! # Why not just shell out to `gpg`
//!
//! Because the person most in need of this check is the one who has just
//! downloaded a bootloader and has no idea whether to trust it, and telling
//! them to first install GnuPG, import a key, understand the web of trust and
//! interpret `gpg: WARNING: This key is not certified with a trusted
//! signature!` is how verification instructions get skipped. `ventoy-verify`
//! carries its own OpenPGP implementation so that checking a release is one
//! command with no setup.
//!
//! It also removes a real failure mode. `gpg --verify` exits zero for a good
//! signature made by *any* key in the user's keyring, so a reader who has ever
//! imported an attacker's key gets a green tick. This crate has no keyring: the
//! caller names exactly one public key file, and a signature by anything else
//! is a failure.
//!
//! # What is signed
//!
//! One manifest, not 182 blobs. See `ventoy_verify_core::manifest` for why.
//! That keeps the signed input small enough to hold in memory, which is what
//! makes the pure-Rust path practical here.

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    unreachable_pub,
    clippy::all,
    clippy::pedantic
)]

pub mod error;
pub mod openpgp;

pub use error::{Error, Result};
pub use openpgp::{verify_detached, PublicKey, SignatureInfo};
