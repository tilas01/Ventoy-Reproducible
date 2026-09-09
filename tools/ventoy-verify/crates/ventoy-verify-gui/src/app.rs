// SPDX-License-Identifier: GPL-3.0-or-later
//!
//! The window.
//!
//! # Why a window exists at all
//!
//! The person most in need of this check is the one who has just downloaded a
//! bootloader and does not use a terminal. Telling them to open one, learn a
//! subcommand and interpret an exit code is how verification gets skipped, and
//! a verification step that gets skipped protects nobody.
//!
//! # What this window is not allowed to do
//!
//! It cannot read a file except through `ventoy-verify-core`, and it cannot
//! check a signature except through `ventoy-verify-sig`. There is no filesystem
//! code in this crate at all beyond handing a `PathBuf` to those two. That keeps
//! the audit surface in one place: if you want to know everything this program
//! can touch, you read `core`, not the interface.
//!
//! # Verification runs off the interface thread
//!
//! Hashing 800 files takes tens of seconds. Doing that on the thread that draws
//! the window freezes it, and a frozen window is one a person force-quits half
//! way through. The work happens on a worker thread and the result arrives down
//! a channel.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use ventoy_verify_core::{manifest::Manifest, verify, Origin, Outcome, Report, Verdict};
use ventoy_verify_sig::{verify_detached, PublicKey};

/// Tokyo Night, the same palette the website and the reference project use.
mod palette {
    use egui::Color32;

    pub(super) const BG: Color32 = Color32::from_rgb(0x1a, 0x1b, 0x26);
    pub(super) const BG_SOFT: Color32 = Color32::from_rgb(0x1f, 0x23, 0x35);
    pub(super) const BORDER: Color32 = Color32::from_rgb(0x2f, 0x35, 0x49);
    pub(super) const FG: Color32 = Color32::from_rgb(0xc0, 0xca, 0xf5);
    pub(super) const MUTED: Color32 = Color32::from_rgb(0x73, 0x7a, 0xa2);
    pub(super) const ACCENT: Color32 = Color32::from_rgb(0x7a, 0xa2, 0xf7);
    pub(super) const OK: Color32 = Color32::from_rgb(0x9e, 0xce, 0x6a);
    pub(super) const WARN: Color32 = Color32::from_rgb(0xe0, 0xaf, 0x68);
    pub(super) const ERR: Color32 = Color32::from_rgb(0xf7, 0x76, 0x8e);
}

/// What the worker thread sends back.
enum Message {
    /// The signature step finished.
    Signature(Result<String, String>),
    /// The whole verification finished.
    Done(Box<Result<Report, String>>),
}

/// What the window is doing right now.
enum State {
    /// Waiting for the user to choose files.
    Idle,
    /// Working, with a line describing which step.
    Busy(String),
    /// Finished, with what was found.
    Finished {
        signature: Option<Result<String, String>>,
        report: Result<Box<Report>, String>,
    },
}

/// The application.
pub(crate) struct App {
    root: Option<PathBuf>,
    manifest: Option<PathBuf>,
    key: Option<PathBuf>,
    strict: bool,
    state: State,
    rx: Option<Receiver<Message>>,
    signature_result: Option<Result<String, String>>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            root: None,
            manifest: None,
            key: None,
            strict: false,
            state: State::Idle,
            rx: None,
            signature_result: None,
        }
    }
}

impl App {
    /// Build the app and apply the palette.
    #[must_use]
    pub(crate) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = palette::BG;
        visuals.window_fill = palette::BG;
        visuals.extreme_bg_color = palette::BG_SOFT;
        visuals.widgets.noninteractive.bg_stroke.color = palette::BORDER;
        visuals.widgets.noninteractive.fg_stroke.color = palette::FG;
        visuals.widgets.inactive.bg_fill = palette::BG_SOFT;
        visuals.widgets.hovered.bg_fill = palette::BORDER;
        visuals.hyperlink_color = palette::ACCENT;
        cc.egui_ctx.set_visuals(visuals);
        Self::default()
    }

    /// Whether everything needed to run is chosen.
    fn ready(&self) -> bool {
        self.manifest.is_some() && self.root.is_some()
    }

    /// Start the work on a background thread.
    fn start(&mut self, ctx: &egui::Context) {
        let Some(manifest_path) = self.manifest.clone() else {
            return;
        };
        let Some(root) = self.root.clone() else {
            return;
        };
        let key_path = self.key.clone();

        let (tx, rx): (Sender<Message>, Receiver<Message>) = mpsc::channel();
        self.rx = Some(rx);
        self.signature_result = None;
        self.state = State::Busy("checking the signature".to_owned());

        let ctx = ctx.clone();
        std::thread::spawn(move || {
            // The signature is checked before the manifest is trusted, exactly
            // as the command line does it. Doing it in the other order here
            // would mean the two programs disagree about what they verified.
            if let Some(key_path) = key_path {
                let outcome = PublicKey::from_armored_file(&key_path)
                    .map_err(|e| e.to_string())
                    .and_then(|key| {
                        let mut sig = manifest_path.clone().into_os_string();
                        sig.push(".asc");
                        verify_detached(&key, &manifest_path, &PathBuf::from(sig))
                            .map_err(|e| e.to_string())
                            .map(|info| info.certificate)
                    });
                let failed = outcome.is_err();
                let _ = tx.send(Message::Signature(outcome));
                ctx.request_repaint();

                if failed {
                    let _ = tx.send(Message::Done(Box::new(Err(
                        "the signature did not verify, so the manifest was not trusted \
                         and no file was checked"
                            .to_owned(),
                    ))));
                    ctx.request_repaint();
                    return;
                }
            }

            let result = std::fs::read_to_string(&manifest_path)
                .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))
                .and_then(|json| {
                    Manifest::from_json(&json, &manifest_path).map_err(|e| e.to_string())
                })
                .and_then(|manifest| {
                    verify::verify_manifest(&manifest, &root).map_err(|e| e.to_string())
                });

            let _ = tx.send(Message::Done(Box::new(result)));
            ctx.request_repaint();
        });
    }

    /// Drain anything the worker sent.
    ///
    /// The receiver is taken out of `self` for the duration, because the loop
    /// needs to write to `self.state` while reading from the channel, and a
    /// borrow of one field held across a write to another is exactly what the
    /// borrow checker is for.
    fn poll(&mut self) {
        let Some(rx) = self.rx.take() else {
            return;
        };
        let mut finished = false;

        while let Ok(message) = rx.try_recv() {
            match message {
                Message::Signature(result) => {
                    self.signature_result = Some(result);
                    self.state = State::Busy("hashing every file the manifest names".to_owned());
                }
                Message::Done(result) => {
                    self.state = State::Finished {
                        signature: self.signature_result.clone(),
                        report: (*result).map(Box::new),
                    };
                    finished = true;
                }
            }
        }

        if !finished {
            self.rx = Some(rx);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        let ctx = ui.ctx().clone();

        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.add_space(8.0);
            ui.heading("ventoy-verify");
            ui.colored_label(
                palette::MUTED,
                "Check a Ventoy-Reproducible release against its signed manifest.",
            );
            ui.add_space(12.0);

            pick_row(ui, "Release folder", &mut self.root, true);
            pick_row(ui, "manifest.json", &mut self.manifest, false);
            pick_row(ui, "Public key (optional)", &mut self.key, false);

            ui.add_space(6.0);
            ui.checkbox(
                &mut self.strict,
                "Require that every file was built by CI and reproduced",
            );
            ui.colored_label(
                palette::MUTED,
                "Off by default: some files in a genuine release are third-party \
                 binaries nobody can rebuild.",
            );

            ui.add_space(12.0);
            let busy = matches!(self.state, State::Busy(_));
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.ready() && !busy, egui::Button::new("  Verify  "))
                    .clicked()
                {
                    self.start(&ctx);
                }
                if self.key.is_none() {
                    ui.colored_label(
                        palette::WARN,
                        "No key chosen: contents will be checked, authenticity will not.",
                    );
                }
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            match &self.state {
                State::Idle => {
                    ui.colored_label(palette::MUTED, "Nothing checked yet.");
                }
                State::Busy(step) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(step);
                    });
                }
                State::Finished { signature, report } => {
                    show_result(ui, signature.as_ref(), report.as_deref(), self.strict);
                }
            }
        });
    }
}

/// One row with a label, the chosen path and a button.
fn pick_row(ui: &mut egui::Ui, label: &str, slot: &mut Option<PathBuf>, folder: bool) {
    ui.horizontal(|ui| {
        ui.add_sized([170.0, 20.0], egui::Label::new(label));
        if ui.button("Choose").clicked() {
            let dialog = rfd::FileDialog::new();
            let chosen = if folder {
                dialog.pick_folder()
            } else {
                dialog.pick_file()
            };
            if let Some(path) = chosen {
                *slot = Some(path);
            }
        }
        match slot {
            Some(path) => {
                ui.colored_label(palette::FG, path.display().to_string());
            }
            None => {
                ui.colored_label(palette::MUTED, "not chosen");
            }
        }
    });
}

/// Render the outcome, failures first.
fn show_result(
    ui: &mut egui::Ui,
    signature: Option<&Result<String, String>>,
    report: Result<&Report, &String>,
    strict: bool,
) {
    if let Some(signature) = signature {
        match signature {
            Ok(fingerprint) => {
                ui.colored_label(palette::OK, "Signature over the manifest is good.");
                ui.colored_label(palette::MUTED, format!("signed by {fingerprint}"));
            }
            Err(message) => {
                ui.colored_label(palette::ERR, "Signature check failed.");
                ui.colored_label(palette::ERR, message);
            }
        }
        ui.add_space(8.0);
    }

    let report = match report {
        Ok(report) => report,
        Err(message) => {
            ui.colored_label(palette::ERR, message);
            return;
        }
    };

    let s = &report.summary;
    let passed = if strict {
        s.fully_reproducible()
    } else {
        s.intact()
    };

    if passed {
        ui.colored_label(palette::OK, "Every file matches the manifest.");
    } else {
        ui.colored_label(
            palette::ERR,
            "This release is not what the manifest says it is. Do not use it.",
        );
    }

    ui.add_space(8.0);
    egui::Grid::new("counts").num_columns(2).show(ui, |ui| {
        count(ui, "matched the manifest", s.matched, palette::FG);
        count(
            ui,
            "compiled by this project's CI",
            s.built_here,
            palette::FG,
        );
        if s.pinned_only > 0 {
            count(ui, "third-party, pinned only", s.pinned_only, palette::WARN);
        }
        if s.not_reproducible > 0 {
            count(ui, "did not reproduce", s.not_reproducible, palette::WARN);
        }
        if s.mismatched > 0 {
            count(ui, "did not match", s.mismatched, palette::ERR);
        }
        if s.missing > 0 {
            count(ui, "missing", s.missing, palette::ERR);
        }
        if s.unreadable > 0 {
            count(ui, "unreadable", s.unreadable, palette::ERR);
        }
    });

    let problems: Vec<_> = report.problems().collect();
    if problems.is_empty() {
        return;
    }

    ui.add_space(10.0);
    ui.colored_label(
        palette::ERR,
        format!("{} files need attention:", problems.len()),
    );
    egui::ScrollArea::vertical()
        .max_height(220.0)
        .show(ui, |ui| {
            for line in problems {
                let what = match &line.outcome {
                    Outcome::Mismatch { .. } => "contents differ",
                    Outcome::Missing => "not present",
                    Outcome::Unreadable { .. } => "could not be read",
                    Outcome::Match => continue,
                };
                ui.colored_label(
                    palette::ERR,
                    format!(
                        "{}  ({what}, {}, {})",
                        line.path.display(),
                        origin_word(line.origin),
                        verdict_word(line.verdict)
                    ),
                );
            }
        });
}

fn count(ui: &mut egui::Ui, label: &str, value: usize, colour: egui::Color32) {
    ui.colored_label(colour, value.to_string());
    ui.colored_label(palette::MUTED, label);
    ui.end_row();
}

const fn origin_word(origin: Origin) -> &'static str {
    match origin {
        Origin::Built => "built here",
        Origin::UpstreamBinary => "third-party binary",
        Origin::NotBuilt => "not yet built here",
    }
}

const fn verdict_word(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Reproduced => "reproduced",
        Verdict::Differs => "did not reproduce",
        Verdict::Unknown => "reproducibility not established",
    }
}
