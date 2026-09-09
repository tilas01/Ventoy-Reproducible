// SPDX-License-Identifier: GPL-3.0-or-later
//!
//! `ventoy-verify-gui`, a window around the same checks the command line does.
//!
//! Kept separate from the command line binary on purpose. A verification tool
//! has to build on a headless runner and inside a minimal container, and a
//! display stack is not available in either. `cargo build` gives you the
//! terminal program; asking for this one is a deliberate act.

#![forbid(unsafe_code)]
#![warn(missing_docs, unreachable_pub, clippy::all, clippy::pedantic)]
// The window is the one place a GUI framework's own conventions win over ours.
#![allow(clippy::needless_pass_by_value)]
// On Windows, do not open a console behind the window in a release build. The
// console is genuinely useful while developing, so it stays in a debug build.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([860.0, 640.0])
            .with_min_inner_size([640.0, 480.0])
            .with_title("ventoy-verify"),
        ..Default::default()
    };

    eframe::run_native(
        "ventoy-verify",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
