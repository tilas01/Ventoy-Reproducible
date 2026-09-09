// SPDX-License-Identifier: GPL-3.0-or-later
//!
//! `ventoy-verify`, the command line program.
//!
//! This file is deliberately tiny. Everything it could get wrong is in `run`,
//! which returns an exit code instead of calling `process::exit`, so that the
//! whole program can be driven from a test without spawning a process. The only
//! thing that happens here and nowhere else is turning that code into an exit.

#![forbid(unsafe_code)]
#![warn(missing_docs, unreachable_pub, clippy::all, clippy::pedantic)]

mod args;
mod output;
mod run;

use clap::Parser;
use std::io::Write as _;

fn main() -> std::process::ExitCode {
    let cli = args::Cli::parse();

    // Locking both streams once, rather than per line, keeps the output of a
    // 182-line report from interleaving with itself under a parallel writer.
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut out = stdout.lock();
    let mut err = stderr.lock();

    let exit = run::run(&cli.command, cli.format, cli.quiet, &mut out, &mut err);

    // A broken pipe here is `ventoy-verify hash | head`, which is not an error
    // and must not turn a passing verification into a failing exit code.
    let _ = out.flush();
    let _ = err.flush();

    std::process::ExitCode::from(exit as u8)
}
