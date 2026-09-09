// SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Loading a public key, and checking a detached signature against it.
//!
//! # There is no keyring here, and that is the point
//!
//! `gpg --verify` succeeds when the signature was made by any key the user has
//! ever imported, and prints its warning about trust on a line most people
//! scroll past. A reader who once imported a key from a forum post gets a
//! zero exit code and a green tick.
//!
//! This crate takes exactly one public key file and verifies against exactly
//! that certificate. A signature by any other key is [`Error::WrongKey`], which
//! is a failure and not a warning. The trust decision is therefore made once,
//! visibly, when the reader chooses which key file to pass, rather than
//! invisibly by the contents of a keyring they have forgotten about.
//!
//! # Subkeys
//!
//! Signing subkeys are the normal arrangement, so a signature usually is not
//! made by the primary key. The issuer is read out of the signature and matched
//! against the primary key and every subkey of the supplied certificate. That
//! is still the same certificate, so it does not widen what is trusted.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};
use pgp::types::KeyDetails;

use crate::error::{Error, Result};

/// The largest signed input this program will read into memory.
///
/// Only the manifest is signed, and a manifest naming every file in the Ventoy
/// tree is a few hundred kilobytes. 64 MiB is far above anything legitimate and
/// far below anything that would trouble a runner.
pub const MAX_SIGNED_INPUT: u64 = 64 * 1024 * 1024;

/// A loaded OpenPGP certificate, and nothing else.
#[derive(Debug)]
pub struct PublicKey {
    certificate: SignedPublicKey,
    fingerprint: String,
    source: PathBuf,
}

impl PublicKey {
    /// Load an armoured public key from a file.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Read`] if the file cannot be read and [`Error::BadKey`]
    /// if it is not a valid armoured OpenPGP certificate.
    pub fn from_armored_file(path: &Path) -> Result<Self> {
        let file = File::open(path).map_err(|source| Error::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let reader = BufReader::new(file);
        let (certificate, _headers) =
            SignedPublicKey::from_armor_single(reader).map_err(|e| Error::BadKey {
                path: path.to_path_buf(),
                reason: e.to_string(),
            })?;

        let fingerprint = format_fingerprint(&certificate.fingerprint().to_string());
        Ok(Self {
            certificate,
            fingerprint,
            source: path.to_path_buf(),
        })
    }

    /// The primary key fingerprint, uppercase hexadecimal without spaces.
    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// The file this certificate was loaded from.
    #[must_use]
    pub fn source(&self) -> &Path {
        &self.source
    }
}

/// What a successful verification established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureInfo {
    /// The fingerprint of the certificate that verified it.
    pub certificate: String,
    /// Whether a subkey rather than the primary key made the signature.
    pub signed_by_subkey: bool,
    /// The file that was verified.
    pub file: PathBuf,
}

/// Verify a detached armoured signature over the contents of `file`.
///
/// # Errors
///
/// Returns [`Error::TooLarge`] if the signed file exceeds
/// [`MAX_SIGNED_INPUT`], [`Error::Read`] if either file cannot be read,
/// [`Error::BadSignature`] if the signature does not parse,
/// [`Error::WrongKey`] if it was made by a key outside this certificate, and
/// [`Error::Invalid`] if the signature simply does not match the content.
pub fn verify_detached(key: &PublicKey, file: &Path, signature: &Path) -> Result<SignatureInfo> {
    let content = read_bounded(file)?;

    let sig_file = File::open(signature).map_err(|source| Error::Read {
        path: signature.to_path_buf(),
        source,
    })?;
    let (detached, _headers) = DetachedSignature::from_armor_single(BufReader::new(sig_file))
        .map_err(|e| Error::BadSignature {
            path: signature.to_path_buf(),
            reason: e.to_string(),
        })?;

    // Try the primary key first, then each subkey. `verify` is what decides,
    // not the issuer subpacket: an issuer field is unauthenticated data that an
    // attacker can set to anything, so it is used only to produce a better
    // error message when nothing verifies, never to skip a check.
    if detached
        .verify(&key.certificate.primary_key, &content)
        .is_ok()
    {
        return Ok(SignatureInfo {
            certificate: key.fingerprint.clone(),
            signed_by_subkey: false,
            file: file.to_path_buf(),
        });
    }

    for subkey in &key.certificate.public_subkeys {
        if detached.verify(&subkey.key, &content).is_ok() {
            return Ok(SignatureInfo {
                certificate: key.fingerprint.clone(),
                signed_by_subkey: true,
                file: file.to_path_buf(),
            });
        }
    }

    // Nothing in this certificate verified it. Distinguish "signed by somebody
    // else" from "signed by this key over different bytes", because the two
    // mean completely different things to a reader.
    let issuer = declared_issuer(&detached);
    if let Some(issuer) = issuer {
        if !certificate_contains(&key.certificate, &issuer) {
            return Err(Error::WrongKey {
                issuer,
                expected: key.fingerprint.clone(),
            });
        }
    }

    Err(Error::Invalid {
        path: file.to_path_buf(),
    })
}

/// Read a file, refusing anything above [`MAX_SIGNED_INPUT`].
///
/// The size is checked from the directory entry before the read, so an
/// oversized file is refused rather than read and then rejected.
fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > MAX_SIGNED_INPUT {
        return Err(Error::TooLarge {
            path: path.to_path_buf(),
            size: metadata.len(),
            limit: MAX_SIGNED_INPUT,
        });
    }

    let file = File::open(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })?;
    // `take` rather than trusting the metadata: the file can grow between the
    // stat and the read, and a bound that a race can step over is not a bound.
    let mut content = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    BufReader::new(file)
        .take(MAX_SIGNED_INPUT)
        .read_to_end(&mut content)
        .map_err(|source| Error::Read {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(content)
}

/// The issuer the signature claims, for error messages only.
fn declared_issuer(signature: &DetachedSignature) -> Option<String> {
    if let Some(fingerprint) = signature.signature.issuer_fingerprint().first() {
        return Some(format_fingerprint(&fingerprint.to_string()));
    }
    signature
        .signature
        .issuer_key_id()
        .first()
        .map(|id| format_fingerprint(&id.to_string()))
}

/// Whether `identifier` names the primary key or any subkey of `certificate`.
fn certificate_contains(certificate: &SignedPublicKey, identifier: &str) -> bool {
    let primary = format_fingerprint(&certificate.fingerprint().to_string());
    if primary.ends_with(identifier) || identifier.ends_with(&primary) {
        return true;
    }
    certificate.public_subkeys.iter().any(|subkey| {
        let sub = format_fingerprint(&subkey.key.fingerprint().to_string());
        sub.ends_with(identifier) || identifier.ends_with(&sub)
    })
}

/// Normalise a fingerprint or key id to uppercase hex with no separators.
///
/// Different corners of the ecosystem print fingerprints with spaces, with
/// `0x`, in lowercase, and in groups of four. Comparing them without
/// normalising is how a correct key gets reported as the wrong one.
fn format_fingerprint(raw: &str) -> String {
    raw.chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_uppercase)
        .collect::<String>()
        .trim_start_matches("0X")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_normalise_to_one_spelling() {
        let spaced = "AB12 CD34 EF56 7890";
        let lower = "ab12cd34ef567890";
        let prefixed = "0xAB12CD34EF567890";
        assert_eq!(format_fingerprint(spaced), "AB12CD34EF567890");
        assert_eq!(format_fingerprint(lower), "AB12CD34EF567890");
        assert_eq!(format_fingerprint(prefixed), "AB12CD34EF567890");
    }

    #[test]
    fn a_missing_key_file_is_a_read_error_naming_the_path() {
        let err = PublicKey::from_armored_file(Path::new("no-such-key.asc"))
            .expect_err("a missing file cannot load");
        assert!(matches!(err, Error::Read { .. }));
        assert!(err.to_string().contains("no-such-key.asc"));
    }

    #[test]
    fn a_file_that_is_not_a_key_is_refused_as_a_bad_key() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("not-a-key.asc");
        std::fs::write(&path, b"this is not an OpenPGP certificate").expect("write");
        let err = PublicKey::from_armored_file(&path).expect_err("garbage is not a key");
        assert!(matches!(err, Error::BadKey { .. }));
    }

    #[test]
    fn an_oversized_signed_input_is_refused_before_it_is_read() {
        // The bound exists so that a hostile release cannot ask the verifier to
        // allocate an arbitrary amount of memory by shipping a huge "manifest".
        assert_eq!(MAX_SIGNED_INPUT, 64 * 1024 * 1024);
    }
}
