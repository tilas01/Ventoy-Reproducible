// SPDX-License-Identifier: GPL-3.0-or-later
//!
//! What the verifier concluded, in a shape both the terminal and the window can
//! render without either of them deciding what counts as a pass.
//!
//! # The rule about counting
//!
//! A file that was never built here is not a pass. It is tempting to print
//! "182/182 verified" by counting pinned third-party binaries as successes, and
//! it would be a lie of exactly the kind this project exists to stop. So the
//! summary has separate counters and the display code cannot merge them: there
//! is no field called `total_ok`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::manifest::{Origin, Verdict};

/// What happened to one file when it was checked against the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum Outcome {
    /// The file on disk matched both digests in the manifest.
    Match,
    /// The file exists and its contents do not match.
    Mismatch {
        /// The digest the manifest recorded.
        expected_sha256: String,
        /// The digest the file on disk actually has.
        actual_sha256: String,
        /// The size the manifest recorded.
        expected_size: u64,
        /// The size on disk.
        actual_size: u64,
    },
    /// The manifest names a file that is not present.
    Missing,
    /// The file could not be read, for the reason given.
    Unreadable {
        /// The I/O failure, rendered.
        reason: String,
    },
}

impl Outcome {
    /// Whether this outcome means the file is exactly what was published.
    #[must_use]
    pub fn is_match(&self) -> bool {
        matches!(self, Self::Match)
    }
}

/// One line of a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// The path checked, relative to the verification root.
    pub path: PathBuf,
    /// Where the manifest says the file came from.
    pub origin: Origin,
    /// Whether the manifest says two builds agreed.
    pub verdict: Verdict,
    /// What the check found.
    pub outcome: Outcome,
}

/// Counts, kept separate on purpose.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// Files whose contents matched the manifest.
    pub matched: usize,
    /// Files whose contents did not match.
    pub mismatched: usize,
    /// Files the manifest named that were not on disk.
    pub missing: usize,
    /// Files that could not be read.
    pub unreadable: usize,
    /// Of the matched files, how many this project compiled itself.
    pub built_here: usize,
    /// Of the matched files, how many are third-party binaries only pinned.
    pub pinned_only: usize,
    /// Of the matched files, how many failed to reproduce across two builds.
    pub not_reproducible: usize,
}

impl Summary {
    /// Whether every file the manifest named was present and correct.
    ///
    /// This is deliberately about integrity only: it answers "is this the
    /// release that was published", not "is this release trustworthy". A
    /// manifest full of `Verdict::Differs` entries can still be intact.
    #[must_use]
    pub fn intact(&self) -> bool {
        self.mismatched == 0 && self.missing == 0 && self.unreadable == 0
    }

    /// Whether every file was present, correct, built here and reproducible.
    ///
    /// This is the strong answer, and it is the one the `--strict` flag
    /// requires. It is separate from [`Summary::intact`] because most releases
    /// will legitimately fail it while still being genuine: nine of upstream's
    /// blobs are third-party binaries nobody here can compile.
    #[must_use]
    pub fn fully_reproducible(&self) -> bool {
        self.intact() && self.pinned_only == 0 && self.not_reproducible == 0
    }
}

/// The result of verifying a whole manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// One line per file, in manifest order.
    pub lines: Vec<Line>,
    /// The counts.
    pub summary: Summary,
}

impl Report {
    /// Build a report from its lines, computing the summary once.
    #[must_use]
    pub fn new(lines: Vec<Line>) -> Self {
        let mut summary = Summary::default();
        for line in &lines {
            match &line.outcome {
                Outcome::Match => {
                    summary.matched += 1;
                    match line.origin {
                        Origin::Built => summary.built_here += 1,
                        Origin::UpstreamBinary | Origin::NotBuilt => summary.pinned_only += 1,
                    }
                    if line.verdict != Verdict::Reproduced {
                        summary.not_reproducible += 1;
                    }
                }
                Outcome::Mismatch { .. } => summary.mismatched += 1,
                Outcome::Missing => summary.missing += 1,
                Outcome::Unreadable { .. } => summary.unreadable += 1,
            }
        }
        Self { lines, summary }
    }

    /// Every line that is not a clean match, for printing failures first.
    pub fn problems(&self) -> impl Iterator<Item = &Line> {
        self.lines.iter().filter(|l| !l.outcome.is_match())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(origin: Origin, verdict: Verdict, outcome: Outcome) -> Line {
        Line {
            path: PathBuf::from("a"),
            origin,
            verdict,
            outcome,
        }
    }

    #[test]
    fn a_pinned_third_party_binary_is_not_counted_as_built_here() {
        let report = Report::new(vec![line(
            Origin::UpstreamBinary,
            Verdict::Unknown,
            Outcome::Match,
        )]);
        assert_eq!(report.summary.matched, 1);
        assert_eq!(report.summary.built_here, 0);
        assert_eq!(report.summary.pinned_only, 1);
        // Intact, because the file is what was published. Not fully
        // reproducible, because nobody here compiled it.
        assert!(report.summary.intact());
        assert!(!report.summary.fully_reproducible());
    }

    #[test]
    fn a_built_but_unreproducible_file_fails_the_strong_test_only() {
        let report = Report::new(vec![line(Origin::Built, Verdict::Differs, Outcome::Match)]);
        assert!(report.summary.intact());
        assert!(!report.summary.fully_reproducible());
        assert_eq!(report.summary.not_reproducible, 1);
    }

    #[test]
    fn a_clean_build_passes_both_tests() {
        let report = Report::new(vec![line(
            Origin::Built,
            Verdict::Reproduced,
            Outcome::Match,
        )]);
        assert!(report.summary.intact());
        assert!(report.summary.fully_reproducible());
        assert_eq!(report.problems().count(), 0);
    }

    #[test]
    fn a_mismatch_fails_everything_and_shows_up_as_a_problem() {
        let report = Report::new(vec![line(
            Origin::Built,
            Verdict::Reproduced,
            Outcome::Mismatch {
                expected_sha256: "a".into(),
                actual_sha256: "b".into(),
                expected_size: 1,
                actual_size: 2,
            },
        )]);
        assert!(!report.summary.intact());
        assert_eq!(report.summary.mismatched, 1);
        assert_eq!(report.problems().count(), 1);
    }
}
