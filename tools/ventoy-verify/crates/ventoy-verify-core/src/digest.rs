// SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Hashing, streamed.
//!
//! # Why this never calls `fs::read`
//!
//! The files this program checks include ISO images and `ventoy.disk.img`, which
//! run to hundreds of megabytes and occasionally to several gigabytes. Reading
//! one into a `Vec<u8>` to hash it works on a developer laptop and gets the job
//! killed by the out-of-memory reaper on a CI runner with 7 GB of RAM. Every
//! hash here is fed from a fixed buffer, so the memory cost is the buffer, not
//! the file.
//!
//! # Why both SHA-256 and BLAKE3
//!
//! SHA-256 is what everybody else publishes, what `sha256sum` prints, and what a
//! reader can check without installing anything. BLAKE3 is several times faster
//! on large files, which matters when the set being verified is 182 binaries
//! plus a disk image. Publishing both costs one extra pass over a buffer already
//! in cache and means neither audience has to take the other's word for it.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq;

use crate::error::{Error, Result};

/// The read buffer, in bytes.
///
/// 1 MiB measured fastest on the blob set: large enough that the syscall cost
/// disappears, small enough to stay in L2 while both hashes consume it.
const BUFFER: usize = 1024 * 1024;

/// The length of a SHA-256 digest in hexadecimal characters.
pub const SHA256_HEX_LEN: usize = 64;

/// The length of a BLAKE3 digest in hexadecimal characters.
pub const BLAKE3_HEX_LEN: usize = 64;

/// Both digests of one file, as lowercase hexadecimal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digests {
    /// The SHA-256 digest.
    pub sha256: String,
    /// The BLAKE3 digest.
    pub blake3: String,
    /// The file's length in bytes.
    pub size: u64,
}

/// Hash one file with SHA-256 and BLAKE3 in a single pass.
///
/// # Errors
///
/// Returns [`Error::Read`] if the file cannot be opened or read to the end.
pub fn hash_file(path: &Path) -> Result<Digests> {
    let file = File::open(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = BufReader::with_capacity(BUFFER, file);
    hash_reader(&mut reader, path)
}

/// Hash anything readable, naming `path` in any error.
///
/// Split out from [`hash_file`] so that the tests can drive it from a cursor
/// without touching the filesystem.
///
/// # Errors
///
/// Returns [`Error::Read`] if the reader fails part way through.
pub fn hash_reader<R: Read>(reader: &mut R, path: &Path) -> Result<Digests> {
    let mut sha = Sha256::new();
    let mut b3 = blake3::Hasher::new();
    let mut buf = vec![0u8; BUFFER];
    let mut size: u64 = 0;

    loop {
        let read = reader.read(&mut buf).map_err(|source| Error::Read {
            path: path.to_path_buf(),
            source,
        })?;
        if read == 0 {
            break;
        }
        let chunk = &buf[..read];
        sha.update(chunk);
        b3.update(chunk);
        // `read` came from a slice length, so it fits a u64 on every target
        // this builds for. Saturating rather than wrapping keeps a corrupt
        // count from ever reading as a small one.
        size = size.saturating_add(read as u64);
    }

    Ok(Digests {
        sha256: hex::encode(sha.finalize()),
        blake3: b3.finalize().to_hex().to_string(),
        size,
    })
}

/// Compare two hexadecimal digests in constant time.
///
/// # Why constant time for a public hash
///
/// A file digest is not a secret, so a timing leak here reveals nothing on its
/// own. It is written this way because the same helper is the obvious one to
/// reach for when the next comparison *is* secret-adjacent, for example a
/// signature body or a key fingerprint the user typed. Making the safe call the
/// only call available is cheaper than remembering which is which.
///
/// Length is compared first and in the clear: digest lengths are fixed and
/// public, and a length mismatch is a malformed manifest rather than a
/// mismatched file.
#[must_use]
pub fn digests_equal(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.as_bytes().ct_eq(right.as_bytes()).into()
}

/// Check that a string is a plausible lowercase hex digest of `expected_len`.
///
/// # Errors
///
/// Returns [`Error::MalformedDigest`] naming `path` and `algorithm` if the
/// string is the wrong length or holds a character that is not lowercase hex.
pub fn validate_hex(
    value: &str,
    expected_len: usize,
    algorithm: &'static str,
    path: &Path,
) -> Result<()> {
    if value.len() != expected_len {
        return Err(Error::MalformedDigest {
            path: path.to_path_buf(),
            algorithm,
            reason: format!(
                "expected {expected_len} hex characters, found {}",
                value.len()
            ),
        });
    }
    // Uppercase is rejected rather than normalised. A manifest is generated by
    // this program and compared byte for byte elsewhere; accepting two spellings
    // of the same digest would mean two manifests that verify identically and
    // hash differently, which is the exact confusion this project exists to
    // remove.
    if let Some(bad) = value
        .chars()
        .find(|c| !c.is_ascii_hexdigit() || c.is_ascii_uppercase())
    {
        return Err(Error::MalformedDigest {
            path: path.to_path_buf(),
            algorithm,
            reason: format!("{bad:?} is not a lowercase hexadecimal character"),
        });
    }
    Ok(())
}

/// Resolve `relative` against `root`, refusing anything that escapes it.
///
/// Manifests come off a release page and are therefore untrusted input. A
/// manifest entry of `../../../../etc/shadow` must not turn into a read outside
/// the tree the user pointed at, and on Windows neither must `C:\Windows\...`
/// or a UNC path.
///
/// The check is lexical and deliberately strict: any absolute path, any Windows
/// prefix, and any `..` component is refused outright rather than normalised.
/// Refusing is safe here because every path this program legitimately handles
/// is a plain relative path produced by its own manifest generator.
///
/// # Errors
///
/// Returns [`Error::PathEscape`] if `relative` is absolute or contains a parent
/// or prefix component.
pub fn resolve_within(root: &Path, relative: &Path) -> Result<PathBuf> {
    use std::path::Component;

    if relative.is_absolute() {
        return Err(Error::PathEscape(relative.to_path_buf()));
    }
    for component in relative.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(Error::PathEscape(relative.to_path_buf()));
            }
        }
    }
    Ok(root.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn p() -> PathBuf {
        PathBuf::from("test")
    }

    #[test]
    fn empty_input_has_the_published_empty_digests() {
        let mut cursor = Cursor::new(Vec::new());
        let d = hash_reader(&mut cursor, &p()).expect("hashing an empty reader cannot fail");
        assert_eq!(
            d.sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            d.blake3,
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
        assert_eq!(d.size, 0);
    }

    #[test]
    fn abc_matches_the_published_vectors() {
        let mut cursor = Cursor::new(b"abc".to_vec());
        let d = hash_reader(&mut cursor, &p()).expect("hashing three bytes cannot fail");
        assert_eq!(
            d.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            d.blake3,
            "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
        );
        assert_eq!(d.size, 3);
    }

    #[test]
    fn input_larger_than_the_buffer_is_hashed_whole() {
        // The bug this guards against is a loop that hashes only the first
        // buffer and reports success, which passes every small test.
        let data = vec![0x5au8; BUFFER + 12_345];
        let mut cursor = Cursor::new(data.clone());
        let streamed = hash_reader(&mut cursor, &p()).expect("hashing cannot fail");
        let at_once = hex::encode(Sha256::digest(&data));
        assert_eq!(streamed.sha256, at_once);
        assert_eq!(streamed.size, data.len() as u64);
    }

    #[test]
    fn digests_equal_is_exact() {
        assert!(digests_equal("abcd", "abcd"));
        assert!(!digests_equal("abcd", "abce"));
        assert!(!digests_equal("abcd", "abcde"));
        assert!(!digests_equal("", "a"));
    }

    #[test]
    fn uppercase_hex_is_refused() {
        let upper = "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855";
        assert!(validate_hex(upper, SHA256_HEX_LEN, "sha256", &p()).is_err());
    }

    #[test]
    fn wrong_length_hex_is_refused() {
        assert!(validate_hex("abcd", SHA256_HEX_LEN, "sha256", &p()).is_err());
    }

    #[test]
    fn good_hex_is_accepted() {
        let good = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert!(validate_hex(good, SHA256_HEX_LEN, "sha256", &p()).is_ok());
    }

    #[test]
    fn traversal_is_refused_in_every_spelling() {
        let root = Path::new("/tmp/root");
        assert!(resolve_within(root, Path::new("../etc/passwd")).is_err());
        assert!(resolve_within(root, Path::new("a/../../b")).is_err());
        assert!(resolve_within(root, Path::new("/etc/passwd")).is_err());
        assert!(resolve_within(root, Path::new("INSTALL/ventoy/ventoy_x64.efi")).is_ok());
        assert!(resolve_within(root, Path::new("./INSTALL/vtoyjump64.exe")).is_ok());
    }
}
