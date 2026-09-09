// SPDX-License-Identifier: GPL-3.0-or-later
//!
//! The manifest: what CI built, what it got, and what it is honest about.
//!
//! # Why one signed manifest instead of 182 signed blobs
//!
//! Signing every blob separately produces 182 signatures that a reader has to
//! check one at a time, and it leaves the *set* unsigned: nothing stops a file
//! being removed from a release, because its signature goes with it. One
//! manifest that names every path, its digests and its verdict, signed once,
//! makes the collection itself the thing that is attested. A missing file is
//! then a verification failure rather than an absence nobody notices.
//!
//! # Why a verdict field exists at all
//!
//! Because several of these files genuinely cannot be built here, and a format
//! with nowhere to say so invites a build script to pretend. `BLOB_List.md`
//! marks nine of upstream's blobs as third-party binaries: the Rocky Linux shim,
//! the imdisk driver, a `TinyCore` kernel. This project can pin their hashes and
//! cannot compile them. `Origin::UpstreamBinary` is how the manifest says that
//! out loud instead of quietly counting them as passes.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::digest::{validate_hex, BLAKE3_HEX_LEN, SHA256_HEX_LEN};
use crate::error::{Error, Result};

/// The manifest schema version this build writes and understands.
pub const SCHEMA: u32 = 1;

/// Where a file in the manifest came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    /// Compiled from source by this project's CI, in a pinned container.
    Built,
    /// A third-party signed binary that cannot be rebuilt here, only pinned.
    ///
    /// The shim, the imdisk driver and the `TinyCore` kernel are the cases. Their
    /// presence in a release is recorded honestly rather than counted as a
    /// success, because nobody here compiled them.
    UpstreamBinary,
    /// Present in upstream's tree and not yet wired into the rebuild set.
    NotBuilt,
}

/// What happened when a built file was compared against the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// Two independent builds produced identical bytes.
    Reproduced,
    /// Two builds of the same source produced different bytes.
    Differs,
    /// Reproducibility was not established, for the reason in `note`.
    Unknown,
}

/// One file in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Path relative to the repository root, using forward slashes.
    pub path: PathBuf,
    /// Length in bytes.
    pub size: u64,
    /// Lowercase hexadecimal SHA-256.
    pub sha256: String,
    /// Lowercase hexadecimal BLAKE3.
    pub blake3: String,
    /// Where the file came from.
    pub origin: Origin,
    /// Whether a second build agreed.
    pub verdict: Verdict,
    /// The build script that produced it, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub built_by: Option<String>,
    /// The toolchain id from `toolchains.lock` that produced it, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchain: Option<String>,
    /// A human sentence about anything unusual, shown in reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The provenance of a whole build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// The upstream Ventoy version this release corresponds to.
    pub ventoy_version: String,
    /// The commit in this fork that was built.
    pub commit: String,
    /// The upstream commit that was merged in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_commit: Option<String>,
    /// `SOURCE_DATE_EPOCH` used for the build.
    pub source_date_epoch: i64,
    /// The container image, pinned by digest, that ran the build.
    pub builder_image: String,
    /// The GitHub Actions run that produced this manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run: Option<String>,
}

/// A full build manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Schema version. Refused if newer than [`SCHEMA`].
    pub schema: u32,
    /// How this build was produced.
    pub provenance: Provenance,
    /// Every file, in path order.
    pub entries: Vec<Entry>,
}

impl Manifest {
    /// Parse a manifest from JSON, checking the schema and every digest.
    ///
    /// Validation happens here rather than at use, so that a caller cannot get
    /// half way through verifying 182 files and then discover entry 140 was
    /// malformed. Either the whole manifest is well formed or none of it is
    /// used.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Manifest`] if the JSON does not parse,
    /// [`Error::UnsupportedSchema`] if it is from a newer build,
    /// [`Error::MalformedDigest`] if any digest is not lowercase hex of the
    /// right length, and [`Error::Inconsistent`] if a path repeats.
    pub fn from_json(json: &str, path: &Path) -> Result<Self> {
        let manifest: Self = serde_json::from_str(json).map_err(|source| Error::Manifest {
            path: path.to_path_buf(),
            source,
        })?;

        if manifest.schema > SCHEMA {
            return Err(Error::UnsupportedSchema {
                found: manifest.schema,
                supported: SCHEMA,
            });
        }

        let mut seen: BTreeSet<&Path> = BTreeSet::new();
        for entry in &manifest.entries {
            validate_hex(&entry.sha256, SHA256_HEX_LEN, "sha256", &entry.path)?;
            validate_hex(&entry.blake3, BLAKE3_HEX_LEN, "blake3", &entry.path)?;
            if !seen.insert(entry.path.as_path()) {
                // A repeated path is how a manifest smuggles two answers for one
                // file: whichever the verifier happens to read second wins.
                return Err(Error::Inconsistent(format!(
                    "{} appears more than once",
                    entry.path.display()
                )));
            }
        }

        Ok(manifest)
    }

    /// Serialise to pretty JSON with a trailing newline.
    ///
    /// The exact formatting matters. A manifest is compared byte for byte
    /// between two CI runs, so it is written one way and only one way.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Serialise`] if serialisation fails.
    pub fn to_json(&self) -> Result<String> {
        let mut out = serde_json::to_string_pretty(self)?;
        out.push('\n');
        Ok(out)
    }

    /// Every entry that this project actually compiled.
    pub fn built(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| e.origin == Origin::Built)
    }

    /// Every entry that could not be compiled here and is only pinned.
    pub fn pinned_only(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| e.origin != Origin::Built)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZERO_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const ZERO_B3: &str = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";

    fn manifest_json(schema: u32, extra_entry: &str) -> String {
        format!(
            r#"{{
              "schema": {schema},
              "provenance": {{
                "ventoy_version": "1.1.07",
                "commit": "0000000000000000000000000000000000000000",
                "source_date_epoch": 1700000000,
                "builder_image": "docker.io/library/debian@sha256:abc"
              }},
              "entries": [
                {{
                  "path": "INSTALL/ventoy/ventoy_x64.efi",
                  "size": 0,
                  "sha256": "{ZERO_SHA}",
                  "blake3": "{ZERO_B3}",
                  "origin": "built",
                  "verdict": "reproduced"
                }}{extra_entry}
              ]
            }}"#
        )
    }

    #[test]
    fn a_well_formed_manifest_parses() {
        let m = Manifest::from_json(&manifest_json(1, ""), Path::new("m.json"))
            .expect("this manifest is well formed");
        assert_eq!(m.entries.len(), 1);
        assert_eq!(m.built().count(), 1);
        assert_eq!(m.pinned_only().count(), 0);
    }

    #[test]
    fn a_newer_schema_is_refused_rather_than_guessed_at() {
        let err = Manifest::from_json(&manifest_json(SCHEMA + 1, ""), Path::new("m.json"))
            .expect_err("a future schema must not be accepted");
        assert!(matches!(err, Error::UnsupportedSchema { .. }));
    }

    #[test]
    fn a_repeated_path_is_refused() {
        let dup = format!(
            r#",{{
              "path": "INSTALL/ventoy/ventoy_x64.efi",
              "size": 1, "sha256": "{ZERO_SHA}", "blake3": "{ZERO_B3}",
              "origin": "built", "verdict": "differs"
            }}"#
        );
        let err = Manifest::from_json(&manifest_json(1, &dup), Path::new("m.json"))
            .expect_err("a duplicate path must not be accepted");
        assert!(matches!(err, Error::Inconsistent(_)));
    }

    #[test]
    fn a_malformed_digest_is_caught_at_parse_time() {
        let bad = manifest_json(1, "").replace(ZERO_SHA, "nothexatall");
        let err = Manifest::from_json(&bad, Path::new("m.json"))
            .expect_err("a short digest must not be accepted");
        assert!(matches!(err, Error::MalformedDigest { .. }));
    }

    #[test]
    fn round_trips_through_json_unchanged() {
        let m =
            Manifest::from_json(&manifest_json(1, ""), Path::new("m.json")).expect("well formed");
        let json = m.to_json().expect("serialising cannot fail");
        let again = Manifest::from_json(&json, Path::new("m.json")).expect("still well formed");
        assert_eq!(m, again);
        assert!(
            json.ends_with('\n'),
            "manifests end with exactly one newline"
        );
    }
}
