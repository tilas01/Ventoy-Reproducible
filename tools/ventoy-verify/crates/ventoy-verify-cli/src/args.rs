// SPDX-License-Identifier: GPL-3.0-or-later
//!
//! The command line surface.
//!
//! # Why the default is the strict-looking one
//!
//! `ventoy-verify release` checks the signature, then the manifest, then every
//! file, and it fails if any of the three fails. That is one command with no
//! flags, because the number of people who will run a three-command sequence
//! correctly is much smaller than the number who will run one, and a
//! verification step that gets skipped protects nobody.
//!
//! The narrower subcommands exist for the cases where somebody genuinely has
//! only one piece: a file and a digest from a release page, or a manifest with
//! no signature because they are checking a fork's build.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// Verify that a Ventoy-Reproducible release is what it says it is.
#[derive(Debug, Parser)]
#[command(
    name = "ventoy-verify",
    version,
    about = "Check a Ventoy-Reproducible release against its signed manifest",
    long_about = "\
Every binary in a Ventoy-Reproducible release is compiled by GitHub Actions and \
recorded in a manifest, and that manifest is signed. This program checks the \
signature over the manifest, then checks every file the manifest names.

It has no keyring. You name one public key file, and a signature made by any \
other key is a failure rather than a warning."
)]
pub(crate) struct Cli {
    /// What to check.
    #[command(subcommand)]
    pub(crate) command: Command,

    /// How to print the result.
    #[arg(long, value_enum, default_value_t = Format::Text, global = true)]
    pub(crate) format: Format,

    /// Print nothing on success, and only failures otherwise.
    #[arg(long, short, global = true)]
    pub(crate) quiet: bool,
}

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Format {
    /// Human readable lines.
    Text,
    /// One JSON document, for scripts and CI.
    Json,
}

/// The subcommands.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Check a signature, a manifest and every file it names.
    ///
    /// This is the whole check and the one to use. It fails if the signature is
    /// bad, if the manifest is malformed, or if any file differs.
    Release {
        /// The directory holding the unpacked release.
        #[arg(long, default_value = ".")]
        root: PathBuf,

        /// The manifest to verify against.
        #[arg(long, default_value = "manifest.json")]
        manifest: PathBuf,

        /// The detached signature over the manifest.
        ///
        /// Defaults to the manifest path with `.asc` appended.
        #[arg(long)]
        signature: Option<PathBuf>,

        /// The public key the signature must have been made by.
        #[arg(long)]
        key: PathBuf,

        /// Also require that every file was built here and reproduced.
        ///
        /// Off by default, because nine of upstream's blobs are third-party
        /// signed binaries that nobody here can compile. A release containing
        /// them is genuine and will not pass this flag.
        #[arg(long)]
        strict: bool,
    },

    /// Check a manifest and its files, without checking any signature.
    ///
    /// For checking your own build, or somebody else's fork, where there is no
    /// signature to check and pretending otherwise would be theatre.
    Manifest {
        /// The directory holding the files.
        #[arg(long, default_value = ".")]
        root: PathBuf,

        /// The manifest to verify against.
        #[arg(long, default_value = "manifest.json")]
        manifest: PathBuf,

        /// Also require that every file was built here and reproduced.
        #[arg(long)]
        strict: bool,
    },

    /// Check one detached signature over one file.
    Signature {
        /// The file that was signed.
        file: PathBuf,

        /// The detached signature. Defaults to the file with `.asc` appended.
        #[arg(long)]
        signature: Option<PathBuf>,

        /// The public key the signature must have been made by.
        #[arg(long)]
        key: PathBuf,
    },

    /// Print the SHA-256 and BLAKE3 of one or more files.
    Hash {
        /// The files to hash.
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },

    /// Check one file against a SHA-256 you have from somewhere else.
    Check {
        /// The file to check.
        file: PathBuf,

        /// The expected SHA-256, as lowercase hexadecimal.
        #[arg(long)]
        sha256: String,
    },
}

impl Command {
    /// The signature path a subcommand should use, applying the `.asc` default.
    ///
    /// Appending rather than replacing the extension is deliberate:
    /// `manifest.json.asc` says what it signs, and `manifest.asc` does not.
    #[must_use]
    pub(crate) fn signature_for(explicit: Option<&PathBuf>, subject: &std::path::Path) -> PathBuf {
        explicit.map_or_else(
            || {
                let mut name = subject.as_os_str().to_os_string();
                name.push(".asc");
                PathBuf::from(name)
            },
            Clone::clone,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_line_definition_is_internally_consistent() {
        // clap panics at runtime on a malformed definition, for example two
        // arguments sharing a short flag. Running its own checker in a test
        // turns that into a build failure rather than a user's first run.
        Cli::command().debug_assert();
    }

    #[test]
    fn the_default_signature_path_appends_rather_than_replaces() {
        let subject = std::path::Path::new("manifest.json");
        let sig = Command::signature_for(None, subject);
        assert_eq!(sig, PathBuf::from("manifest.json.asc"));
    }

    #[test]
    fn an_explicit_signature_path_wins() {
        let subject = std::path::Path::new("manifest.json");
        let explicit = PathBuf::from("elsewhere/sig.asc");
        let sig = Command::signature_for(Some(&explicit), subject);
        assert_eq!(sig, explicit);
    }
}
