// SPDX-License-Identifier: GPL-3.0-or-later
//!
//! The engine: walk a manifest, hash what it names, and produce a report.
//!
//! # Why this hashes in parallel and reports in order
//!
//! Verifying 182 binaries plus a disk image is I/O bound on a spinning disk and
//! CPU bound on an SSD, and `rayon` covers both without the caller choosing.
//! What it must not do is reorder the output: a report whose lines shuffle
//! between runs cannot be diffed, and diffing two reports is the first thing
//! anybody does when one machine disagrees with another. So the work is
//! parallel and the collection is indexed, which keeps manifest order exactly.

use std::path::Path;

use rayon::prelude::*;

use crate::digest::{digests_equal, hash_file, resolve_within};
use crate::error::Result;
use crate::manifest::Manifest;
use crate::report::{Line, Outcome, Report};

/// Verify every file a manifest names, under `root`.
///
/// Files are hashed in parallel. The returned report is in manifest order
/// regardless of the order they finished in.
///
/// # Errors
///
/// Returns [`crate::Error::PathEscape`] if the manifest names a path outside
/// `root`. Per-file read failures are reported as [`Outcome::Unreadable`]
/// rather than aborting the run, because a reader wants to know about all 182
/// files, not just the first one that failed.
pub fn verify_manifest(manifest: &Manifest, root: &Path) -> Result<Report> {
    // Path resolution happens first and serially, so that a hostile manifest is
    // rejected before any work starts rather than after 90 files have been
    // read. This loop is the only place a manifest path becomes a real path.
    let mut resolved = Vec::with_capacity(manifest.entries.len());
    for entry in &manifest.entries {
        resolved.push(resolve_within(root, &entry.path)?);
    }

    let lines: Vec<Line> = manifest
        .entries
        .par_iter()
        .zip(resolved.par_iter())
        .map(|(entry, full)| {
            let outcome = if full.is_file() {
                match hash_file(full) {
                    Ok(found) => {
                        // Both digests must agree, not either. Publishing two
                        // and checking one would make the second decorative.
                        let sha_ok = digests_equal(&found.sha256, &entry.sha256);
                        let b3_ok = digests_equal(&found.blake3, &entry.blake3);
                        let size_ok = found.size == entry.size;
                        if sha_ok && b3_ok && size_ok {
                            Outcome::Match
                        } else {
                            Outcome::Mismatch {
                                expected_sha256: entry.sha256.clone(),
                                actual_sha256: found.sha256,
                                expected_size: entry.size,
                                actual_size: found.size,
                            }
                        }
                    }
                    Err(e) => Outcome::Unreadable {
                        reason: e.to_string(),
                    },
                }
            } else {
                Outcome::Missing
            };

            Line {
                path: entry.path.clone(),
                origin: entry.origin,
                verdict: entry.verdict,
                outcome,
            }
        })
        .collect();

    Ok(Report::new(lines))
}

/// Verify a single file against an expected SHA-256, for the one-off case.
///
/// This is what the `hash` and `check` subcommands use when somebody has a file
/// and a digest from a release page and wants an answer without a manifest.
///
/// # Errors
///
/// Returns [`crate::Error::Read`] if the file cannot be read.
pub fn verify_one(path: &Path, expected_sha256: &str) -> Result<bool> {
    let found = hash_file(path)?;
    Ok(digests_equal(&found.sha256, expected_sha256))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Entry, Origin, Provenance, Verdict};
    use std::fs;
    use std::path::PathBuf;

    fn provenance() -> Provenance {
        Provenance {
            ventoy_version: "1.1.07".into(),
            commit: "0".repeat(40),
            upstream_commit: None,
            source_date_epoch: 1_700_000_000,
            builder_image: "test".into(),
            workflow_run: None,
        }
    }

    fn entry(path: &str, size: u64, sha256: &str, blake3: &str) -> Entry {
        Entry {
            path: PathBuf::from(path),
            size,
            sha256: sha256.into(),
            blake3: blake3.into(),
            origin: Origin::Built,
            verdict: Verdict::Reproduced,
            built_by: None,
            toolchain: None,
            note: None,
        }
    }

    const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const ABC_B3: &str = "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85";

    #[test]
    fn a_matching_tree_verifies_clean() {
        let dir = tempfile::tempdir().expect("a temp dir");
        fs::write(dir.path().join("blob.bin"), b"abc").expect("write");

        let manifest = Manifest {
            schema: 1,
            provenance: provenance(),
            entries: vec![entry("blob.bin", 3, ABC_SHA, ABC_B3)],
        };

        let report = verify_manifest(&manifest, dir.path()).expect("verification runs");
        assert!(report.summary.intact());
        assert!(report.summary.fully_reproducible());
        assert_eq!(report.summary.matched, 1);
    }

    #[test]
    fn a_changed_byte_is_caught() {
        let dir = tempfile::tempdir().expect("a temp dir");
        fs::write(dir.path().join("blob.bin"), b"abd").expect("write");

        let manifest = Manifest {
            schema: 1,
            provenance: provenance(),
            entries: vec![entry("blob.bin", 3, ABC_SHA, ABC_B3)],
        };

        let report = verify_manifest(&manifest, dir.path()).expect("verification runs");
        assert!(!report.summary.intact());
        assert_eq!(report.summary.mismatched, 1);
    }

    #[test]
    fn a_missing_file_is_reported_not_skipped() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let manifest = Manifest {
            schema: 1,
            provenance: provenance(),
            entries: vec![entry("absent.bin", 3, ABC_SHA, ABC_B3)],
        };

        let report = verify_manifest(&manifest, dir.path()).expect("verification runs");
        assert_eq!(report.summary.missing, 1);
        assert!(!report.summary.intact());
    }

    #[test]
    fn a_traversing_manifest_is_refused_before_anything_is_read() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let manifest = Manifest {
            schema: 1,
            provenance: provenance(),
            entries: vec![entry("../escape.bin", 3, ABC_SHA, ABC_B3)],
        };
        assert!(verify_manifest(&manifest, dir.path()).is_err());
    }

    #[test]
    fn report_order_follows_manifest_order() {
        let dir = tempfile::tempdir().expect("a temp dir");
        for name in ["c.bin", "a.bin", "b.bin"] {
            fs::write(dir.path().join(name), b"abc").expect("write");
        }
        let manifest = Manifest {
            schema: 1,
            provenance: provenance(),
            entries: vec![
                entry("c.bin", 3, ABC_SHA, ABC_B3),
                entry("a.bin", 3, ABC_SHA, ABC_B3),
                entry("b.bin", 3, ABC_SHA, ABC_B3),
            ],
        };
        let report = verify_manifest(&manifest, dir.path()).expect("verification runs");
        let paths: Vec<_> = report
            .lines
            .iter()
            .map(|l| l.path.to_string_lossy().to_string())
            .collect();
        assert_eq!(paths, vec!["c.bin", "a.bin", "b.bin"]);
    }

    #[test]
    fn verify_one_answers_a_single_file() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let file = dir.path().join("x");
        fs::write(&file, b"abc").expect("write");
        assert!(verify_one(&file, ABC_SHA).expect("hashing works"));
        assert!(!verify_one(&file, &"0".repeat(64)).expect("hashing works"));
    }
}
