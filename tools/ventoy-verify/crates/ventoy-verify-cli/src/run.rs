// SPDX-License-Identifier: GPL-3.0-or-later
//!
//! What each subcommand actually does.
//!
//! # The order of checks is the security property
//!
//! `release` checks the signature over the manifest *before* it trusts a single
//! byte of the manifest, and it checks the manifest's schema and digests before
//! it opens any file the manifest names. Doing it the other way round, hashing
//! first and checking the signature at the end, means a hostile manifest has
//! already directed 182 file reads before anything questioned where it came
//! from.
//!
//! # Exit codes
//!
//! 0 for a pass, 1 for a verification failure, and 2 for anything that stopped
//! the check from happening at all: a missing file, a malformed key, an
//! unreadable manifest. The distinction matters in a script, where "the release
//! is bad" and "you typed the wrong path" deserve different reactions.

use std::io::Write;
use std::path::Path;

use ventoy_verify_core::{manifest::Manifest, verify, Report};
use ventoy_verify_sig::{verify_detached, PublicKey};

use crate::args::{Command, Format};
use crate::output::{self, Colour};

/// The process exit code for a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exit {
    /// Everything checked out.
    Pass = 0,
    /// The check ran and something did not verify.
    Fail = 1,
    /// The check could not be performed.
    Error = 2,
}

/// Run one subcommand, writing to `out` and `err`.
///
/// Returns the exit code rather than calling `process::exit`, so that the
/// whole program is testable without spawning it.
pub(crate) fn run(
    command: &Command,
    format: Format,
    quiet: bool,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Exit {
    let colour = if format == Format::Json {
        Colour::never()
    } else {
        Colour::detect()
    };

    match command {
        Command::Release {
            root,
            manifest,
            signature,
            key,
            strict,
        } => {
            let sig_path = Command::signature_for(signature.as_ref(), manifest);
            release(
                root, manifest, &sig_path, key, *strict, format, quiet, colour, out, err,
            )
        }
        Command::Manifest {
            root,
            manifest,
            strict,
        } => match check_manifest(root, manifest) {
            Ok(report) => finish(&report, *strict, format, quiet, colour, out, err),
            Err(message) => fail_hard(err, &message),
        },
        Command::Signature {
            file,
            signature,
            key,
        } => {
            let sig_path = Command::signature_for(signature.as_ref(), file);
            signature_only(file, &sig_path, key, colour, out, err)
        }
        Command::Hash { files } => hash(files, format, out, err),
        Command::Check { file, sha256 } => check_one(file, sha256, colour, out, err),
    }
}

/// The whole check: signature, then manifest, then every file.
#[allow(clippy::too_many_arguments)]
fn release(
    root: &Path,
    manifest_path: &Path,
    signature_path: &Path,
    key_path: &Path,
    strict: bool,
    format: Format,
    quiet: bool,
    colour: Colour,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Exit {
    let key = match PublicKey::from_armored_file(key_path) {
        Ok(key) => key,
        Err(e) => return fail_hard(err, &e.to_string()),
    };

    // The signature is checked first, and nothing below runs if it fails.
    let info = match verify_detached(&key, manifest_path, signature_path) {
        Ok(info) => info,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return Exit::Fail;
        }
    };

    if !quiet && format == Format::Text {
        let _ = output::signature(out, colour, &info);
    }

    match check_manifest(root, manifest_path) {
        Ok(report) => finish(&report, strict, format, quiet, colour, out, err),
        Err(message) => fail_hard(err, &message),
    }
}

/// Parse a manifest and verify every file it names.
fn check_manifest(root: &Path, manifest_path: &Path) -> Result<Report, String> {
    let json = std::fs::read_to_string(manifest_path)
        .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;
    let manifest = Manifest::from_json(&json, manifest_path).map_err(|e| e.to_string())?;
    verify::verify_manifest(&manifest, root).map_err(|e| e.to_string())
}

/// Print a report and turn it into an exit code.
fn finish(
    report: &Report,
    strict: bool,
    format: Format,
    quiet: bool,
    colour: Colour,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Exit {
    match format {
        Format::Json => match serde_json::to_string_pretty(report) {
            Ok(json) => {
                let _ = writeln!(out, "{json}");
            }
            Err(e) => return fail_hard(err, &format!("cannot render JSON: {e}")),
        },
        Format::Text => {
            let _ = output::report(out, colour, report, quiet);
            if !quiet {
                let _ = output::verdict(out, colour, report, strict);
            }
        }
    }

    let ok = if strict {
        report.summary.fully_reproducible()
    } else {
        report.summary.intact()
    };
    if ok {
        Exit::Pass
    } else {
        Exit::Fail
    }
}

/// Check one detached signature and nothing else.
fn signature_only(
    file: &Path,
    signature: &Path,
    key_path: &Path,
    colour: Colour,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Exit {
    let key = match PublicKey::from_armored_file(key_path) {
        Ok(key) => key,
        Err(e) => return fail_hard(err, &e.to_string()),
    };
    match verify_detached(&key, file, signature) {
        Ok(info) => {
            let _ = output::signature(out, colour, &info);
            Exit::Pass
        }
        Err(e) => {
            let _ = writeln!(err, "{e}");
            Exit::Fail
        }
    }
}

/// Print digests for one or more files.
fn hash(
    files: &[std::path::PathBuf],
    format: Format,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Exit {
    let mut rows = Vec::with_capacity(files.len());
    for file in files {
        match ventoy_verify_core::digest::hash_file(file) {
            Ok(d) => rows.push((file.clone(), d)),
            Err(e) => return fail_hard(err, &e.to_string()),
        }
    }

    match format {
        Format::Json => {
            let value: Vec<_> = rows
                .iter()
                .map(|(path, d)| {
                    serde_json::json!({
                        "path": path,
                        "size": d.size,
                        "sha256": d.sha256,
                        "blake3": d.blake3,
                    })
                })
                .collect();
            match serde_json::to_string_pretty(&value) {
                Ok(json) => {
                    let _ = writeln!(out, "{json}");
                }
                Err(e) => return fail_hard(err, &format!("cannot render JSON: {e}")),
            }
        }
        Format::Text => {
            for (path, d) in &rows {
                // The same shape `sha256sum` prints, so the line can be pasted
                // into a SHA256SUMS file or compared with one by eye.
                let _ = writeln!(out, "{}  {}", d.sha256, path.display());
                let _ = writeln!(out, "{}  {} (blake3)", d.blake3, path.display());
            }
        }
    }
    Exit::Pass
}

/// Check one file against a digest given on the command line.
fn check_one(
    file: &Path,
    expected: &str,
    colour: Colour,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Exit {
    // Validate the digest the user typed before reading the file. Hashing a
    // two gigabyte image and then discovering the expected value was a
    // truncated paste wastes a minute and teaches nothing.
    if let Err(e) = ventoy_verify_core::digest::validate_hex(
        expected,
        ventoy_verify_core::digest::SHA256_HEX_LEN,
        "sha256",
        file,
    ) {
        return fail_hard(err, &e.to_string());
    }

    match verify::verify_one(file, expected) {
        Ok(true) => {
            let _ = output::matched(out, colour, file);
            Exit::Pass
        }
        Ok(false) => {
            let _ = output::did_not_match(err, colour, file, expected);
            Exit::Fail
        }
        Err(e) => fail_hard(err, &e.to_string()),
    }
}

/// Report something that stopped the check from running.
fn fail_hard(err: &mut impl Write, message: &str) -> Exit {
    let _ = writeln!(err, "{message}");
    Exit::Error
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn run_to_strings(command: &Command, format: Format) -> (Exit, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let exit = run(command, format, false, &mut out, &mut err);
        (
            exit,
            String::from_utf8(out).expect("stdout is utf-8"),
            String::from_utf8(err).expect("stderr is utf-8"),
        )
    }

    #[test]
    fn hashing_a_known_file_prints_the_known_digest() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let file = dir.path().join("abc.bin");
        fs::write(&file, b"abc").expect("write");

        let (exit, out, _) = run_to_strings(
            &Command::Hash {
                files: vec![file.clone()],
            },
            Format::Text,
        );
        assert_eq!(exit, Exit::Pass);
        assert!(out.contains(ABC_SHA));
    }

    #[test]
    fn checking_a_matching_digest_passes_and_a_wrong_one_fails() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let file = dir.path().join("abc.bin");
        fs::write(&file, b"abc").expect("write");

        let (exit, _, _) = run_to_strings(
            &Command::Check {
                file: file.clone(),
                sha256: ABC_SHA.to_string(),
            },
            Format::Text,
        );
        assert_eq!(exit, Exit::Pass);

        let (exit, _, err) = run_to_strings(
            &Command::Check {
                file,
                sha256: "0".repeat(64),
            },
            Format::Text,
        );
        assert_eq!(exit, Exit::Fail);
        assert!(err.contains("does not match"));
    }

    #[test]
    fn a_malformed_expected_digest_is_an_error_not_a_failure() {
        // Exit 2, not exit 1: the user mistyped, the file is not implicated.
        let dir = tempfile::tempdir().expect("a temp dir");
        let file = dir.path().join("abc.bin");
        fs::write(&file, b"abc").expect("write");

        let (exit, _, _) = run_to_strings(
            &Command::Check {
                file,
                sha256: "not-a-digest".to_string(),
            },
            Format::Text,
        );
        assert_eq!(exit, Exit::Error);
    }

    #[test]
    fn a_missing_manifest_is_an_error_not_a_failure() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (exit, _, err) = run_to_strings(
            &Command::Manifest {
                root: dir.path().to_path_buf(),
                manifest: dir.path().join("absent.json"),
                strict: false,
            },
            Format::Text,
        );
        assert_eq!(exit, Exit::Error);
        assert!(err.contains("cannot read"));
    }

    #[test]
    fn a_manifest_whose_files_are_present_passes() {
        let dir = tempfile::tempdir().expect("a temp dir");
        fs::write(dir.path().join("blob.bin"), b"abc").expect("write");
        let manifest = dir.path().join("manifest.json");
        fs::write(
            &manifest,
            format!(
                r#"{{
                  "schema": 1,
                  "provenance": {{
                    "ventoy_version": "1.1.07",
                    "commit": "{}",
                    "source_date_epoch": 1700000000,
                    "builder_image": "test"
                  }},
                  "entries": [
                    {{
                      "path": "blob.bin", "size": 3,
                      "sha256": "{ABC_SHA}",
                      "blake3": "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85",
                      "origin": "built", "verdict": "reproduced"
                    }}
                  ]
                }}"#,
                "0".repeat(40)
            ),
        )
        .expect("write");

        let (exit, out, _) = run_to_strings(
            &Command::Manifest {
                root: dir.path().to_path_buf(),
                manifest: manifest.clone(),
                strict: true,
            },
            Format::Text,
        );
        assert_eq!(exit, Exit::Pass);
        assert!(out.contains("PASSED"));

        // And the same run as JSON must be parseable, because CI consumes it.
        let (exit, out, _) = run_to_strings(
            &Command::Manifest {
                root: dir.path().to_path_buf(),
                manifest,
                strict: true,
            },
            Format::Json,
        );
        assert_eq!(exit, Exit::Pass);
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("JSON output must parse");
        assert_eq!(parsed["summary"]["matched"], 1);
    }

    #[test]
    fn a_tampered_file_fails_with_exit_one() {
        let dir = tempfile::tempdir().expect("a temp dir");
        fs::write(dir.path().join("blob.bin"), b"abd").expect("write");
        let manifest = dir.path().join("manifest.json");
        fs::write(
            &manifest,
            format!(
                r#"{{
                  "schema": 1,
                  "provenance": {{
                    "ventoy_version": "1.1.07",
                    "commit": "{}",
                    "source_date_epoch": 1700000000,
                    "builder_image": "test"
                  }},
                  "entries": [
                    {{
                      "path": "blob.bin", "size": 3,
                      "sha256": "{ABC_SHA}",
                      "blake3": "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85",
                      "origin": "built", "verdict": "reproduced"
                    }}
                  ]
                }}"#,
                "0".repeat(40)
            ),
        )
        .expect("write");

        let (exit, out, _) = run_to_strings(
            &Command::Manifest {
                root: dir.path().to_path_buf(),
                manifest,
                strict: false,
            },
            Format::Text,
        );
        assert_eq!(exit, Exit::Fail);
        assert!(out.contains("Do not use it"));
        assert!(out.contains("blob.bin"));
    }

    #[test]
    fn a_missing_key_file_stops_the_release_check_with_exit_two() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (exit, _, _) = run_to_strings(
            &Command::Release {
                root: dir.path().to_path_buf(),
                manifest: dir.path().join("manifest.json"),
                signature: None,
                key: PathBuf::from("no-such-key.asc"),
                strict: false,
            },
            Format::Text,
        );
        assert_eq!(exit, Exit::Error);
    }
}
