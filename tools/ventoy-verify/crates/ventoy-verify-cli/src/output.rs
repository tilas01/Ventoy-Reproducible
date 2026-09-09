// SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Printing, and the one rule it follows.
//!
//! # Failures first, and never only a count
//!
//! A verifier that prints 182 lines and puts the one failure at line 94 has
//! technically reported it. Every failing path is printed first, with its name,
//! before any summary, because the reader's next action depends on which file
//! it was.
//!
//! Colour is written with the plainest possible escapes and is switched off
//! whenever the output is not a terminal, so that piping into a file or a CI
//! log produces text rather than a page of control characters.

use std::io::{IsTerminal, Write};

use ventoy_verify_core::{Origin, Report, Verdict};
use ventoy_verify_sig::SignatureInfo;

/// Whether to emit ANSI colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Colour(bool);

impl Colour {
    /// Decide from whether stdout is a terminal, honouring `NO_COLOR`.
    ///
    /// `NO_COLOR` is checked for presence, not value, which is what the
    /// convention at no-color.org actually specifies.
    #[must_use]
    pub(crate) fn detect() -> Self {
        let disabled = std::env::var_os("NO_COLOR").is_some();
        Self(!disabled && std::io::stdout().is_terminal())
    }

    /// Colour off, for JSON output and tests.
    #[must_use]
    pub(crate) const fn never() -> Self {
        Self(false)
    }

    fn wrap(self, code: &str, text: &str) -> String {
        if self.0 {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn good(self, text: &str) -> String {
        self.wrap("32", text)
    }

    fn bad(self, text: &str) -> String {
        self.wrap("31", text)
    }

    fn warn(self, text: &str) -> String {
        self.wrap("33", text)
    }

    fn dim(self, text: &str) -> String {
        self.wrap("2", text)
    }
}

/// Print what a signature established.
///
/// # Errors
///
/// Returns any I/O failure from the writer.
pub(crate) fn signature(
    w: &mut impl Write,
    colour: Colour,
    info: &SignatureInfo,
) -> std::io::Result<()> {
    writeln!(
        w,
        "{} signature over {} is good",
        colour.good("OK"),
        info.file.display()
    )?;
    let by = if info.signed_by_subkey {
        "a signing subkey of"
    } else {
        "the primary key of"
    };
    writeln!(
        w,
        "   made by {by} {}",
        colour.dim(&spaced_fingerprint(&info.certificate))
    )
}

/// Print a full report, failures first.
///
/// # Errors
///
/// Returns any I/O failure from the writer.
pub(crate) fn report(
    w: &mut impl Write,
    colour: Colour,
    report: &Report,
    quiet: bool,
) -> std::io::Result<()> {
    for line in report.problems() {
        let what = match &line.outcome {
            ventoy_verify_core::Outcome::Mismatch {
                expected_sha256,
                actual_sha256,
                expected_size,
                actual_size,
            } => {
                if expected_size == actual_size {
                    format!(
                        "contents differ\n     expected {expected_sha256}\n     found    {actual_sha256}"
                    )
                } else {
                    format!(
                        "contents differ\n     expected {expected_size} bytes, {expected_sha256}\n     found    {actual_size} bytes, {actual_sha256}"
                    )
                }
            }
            ventoy_verify_core::Outcome::Missing => "not present".to_string(),
            ventoy_verify_core::Outcome::Unreadable { reason } => reason.clone(),
            ventoy_verify_core::Outcome::Match => unreachable!("problems() excludes matches"),
        };
        writeln!(
            w,
            "{} {}: {what}
     ({}, {})",
            colour.bad("FAIL"),
            line.path.display(),
            origin_word(line.origin),
            verdict_word(line.verdict)
        )?;
    }

    let s = &report.summary;

    // The counts are printed separately and never added together. A pinned
    // third-party binary is not something this project built, and a line
    // reading "182 verified" would say it was.
    if !quiet {
        writeln!(w)?;
        writeln!(
            w,
            "{} files checked",
            s.matched + s.mismatched + s.missing + s.unreadable
        )?;
        writeln!(w, "  {} matched the manifest", s.matched)?;
        writeln!(w, "    {} compiled by this project's CI", s.built_here)?;
        if s.pinned_only > 0 {
            writeln!(
                w,
                "    {} third-party binaries, pinned by hash and {}",
                s.pinned_only,
                colour.warn("not built here")
            )?;
        }
        if s.not_reproducible > 0 {
            writeln!(
                w,
                "    {} {} across two builds",
                s.not_reproducible,
                colour.warn("did not reproduce")
            )?;
        }
        if s.mismatched > 0 {
            writeln!(w, "  {} {}", s.mismatched, colour.bad("did not match"))?;
        }
        if s.missing > 0 {
            writeln!(w, "  {} {}", s.missing, colour.bad("were missing"))?;
        }
        if s.unreadable > 0 {
            writeln!(w, "  {} {}", s.unreadable, colour.bad("could not be read"))?;
        }
    }

    Ok(())
}

/// Print the closing verdict.
///
/// # Errors
///
/// Returns any I/O failure from the writer.
pub(crate) fn verdict(
    w: &mut impl Write,
    colour: Colour,
    report: &Report,
    strict: bool,
) -> std::io::Result<()> {
    writeln!(w)?;
    if !report.summary.intact() {
        return writeln!(
            w,
            "{} this release is not what the manifest says it is. Do not use it.",
            colour.bad("FAILED.")
        );
    }
    if strict && !report.summary.fully_reproducible() {
        return writeln!(
            w,
            "{} every file matches the manifest, but not every file was built \
             here and reproduced, and --strict was given.",
            colour.warn("FAILED STRICTLY.")
        );
    }
    if report.summary.fully_reproducible() {
        writeln!(
            w,
            "{} every file matches, every file was compiled by CI, and every \
             file reproduced across two builds.",
            colour.good("PASSED.")
        )
    } else {
        writeln!(
            w,
            "{} every file matches the manifest. Some were not built here; see \
             the counts above.",
            colour.good("PASSED.")
        )
    }
}

/// Report that a single file matched a digest given on the command line.
///
/// # Errors
///
/// Returns any I/O failure from the writer.
pub(crate) fn matched(
    w: &mut impl Write,
    colour: Colour,
    file: &std::path::Path,
) -> std::io::Result<()> {
    writeln!(w, "{} {} matches", colour.good("OK"), file.display())
}

/// Report that a single file did not match a digest given on the command line.
///
/// The expected digest is echoed back. A reader who pasted it from a web page
/// needs to see what this program actually received, because a truncated paste
/// looks identical to a tampered file until you compare the two.
///
/// # Errors
///
/// Returns any I/O failure from the writer.
pub(crate) fn did_not_match(
    w: &mut impl Write,
    colour: Colour,
    file: &std::path::Path,
    expected: &str,
) -> std::io::Result<()> {
    writeln!(
        w,
        "{} {} does not match {expected}",
        colour.bad("FAIL"),
        file.display()
    )
}

/// Group a fingerprint into fours, the way every other tool prints it.
fn spaced_fingerprint(raw: &str) -> String {
    raw.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Describe an origin in one word, for the JSON and the window.
#[must_use]
pub(crate) const fn origin_word(origin: Origin) -> &'static str {
    match origin {
        Origin::Built => "built here",
        Origin::UpstreamBinary => "third-party binary",
        Origin::NotBuilt => "not yet built here",
    }
}

/// Describe a verdict in one phrase.
#[must_use]
pub(crate) const fn verdict_word(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Reproduced => "reproduced",
        Verdict::Differs => "did not reproduce",
        Verdict::Unknown => "not established",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use ventoy_verify_core::report::{Line, Outcome};

    fn line(outcome: Outcome) -> Line {
        Line {
            path: PathBuf::from("INSTALL/ventoy/ventoy_x64.efi"),
            origin: Origin::Built,
            verdict: Verdict::Reproduced,
            outcome,
        }
    }

    #[test]
    fn a_failing_file_is_named_in_the_output() {
        let r = Report::new(vec![line(Outcome::Missing)]);
        let mut out = Vec::new();
        report(&mut out, Colour::never(), &r, false).expect("writing to a Vec cannot fail");
        let text = String::from_utf8(out).expect("output is utf-8");
        assert!(text.contains("INSTALL/ventoy/ventoy_x64.efi"));
        assert!(text.contains("not present"));
    }

    #[test]
    fn the_summary_never_merges_built_and_pinned_counts() {
        let mut pinned = line(Outcome::Match);
        pinned.origin = Origin::UpstreamBinary;
        let r = Report::new(vec![line(Outcome::Match), pinned]);
        let mut out = Vec::new();
        report(&mut out, Colour::never(), &r, false).expect("writing cannot fail");
        let text = String::from_utf8(out).expect("output is utf-8");
        assert!(text.contains("1 compiled by this project's CI"));
        assert!(text.contains("not built here"));
        // The thing that must never appear is a single total presented as if
        // everything had been verified to the same standard.
        assert!(!text.contains("2 verified"));
    }

    #[test]
    fn a_broken_release_says_do_not_use_it() {
        let r = Report::new(vec![line(Outcome::Missing)]);
        let mut out = Vec::new();
        verdict(&mut out, Colour::never(), &r, false).expect("writing cannot fail");
        let text = String::from_utf8(out).expect("output is utf-8");
        assert!(text.contains("Do not use it"));
    }

    #[test]
    fn colour_is_off_when_asked_to_be_off() {
        let c = Colour::never();
        assert_eq!(c.good("x"), "x");
        assert_eq!(c.bad("x"), "x");
    }

    #[test]
    fn fingerprints_print_in_groups_of_four() {
        assert_eq!(spaced_fingerprint("ABCD1234EF"), "ABCD 1234 EF");
    }
}
