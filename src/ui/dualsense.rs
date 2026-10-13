//! DualSense mode: the switch, what the virtual pad is doing, and how to
//! get Steam to hand it to games.

use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use apex5::ds5;
use apex5::lang::t;
use apex5::worker::Cmd;
use eframe::egui::{self, RichText, vec2};

use super::theme::{self, accent};
use super::widgets::{self, card, card_titled, hint, pill};
use crate::App;

/// Steam opens the Apex through its own driver; this keeps it off it, so
/// games see only the virtual DualSense. `start_steam` sets the same; the
/// text is for those who start Steam their own way.
const STEAM: &str = "env SDL_GAMECONTROLLER_IGNORE_DEVICES=0x37d7/0x2501 \
                     SDL_JOYSTICK_IGNORE_DEVICES=0x37d7/0x2501 steam";

/// The "DualSense mode" page.
pub fn page(app: &mut App, ui: &mut egui::Ui) {
    super::header(
        app,
        ui,
        t("DualSense mode"),
        t("Games with adaptive triggers drive the Apex triggers"),
    );
    let st = app.ds5();
    let ready = apex5::udev::uhid_ready();
    card(ui, |ui| {
        let mut on = st.on;
        let changed = ui
            .add_enabled_ui(ready || st.on, |ui| {
                widgets::toggle_row(
                    ui,
                    t("Show games a DualSense"),
                    t("Trigger build-up, kick and vibration come from the game itself (e.g. No Man's Sky)"),
                    &mut on,
                )
            })
            .inner;
        if changed {
            app.send(Cmd::DualSense(on));
        }
        if st.on {
            hint(
                ui,
                t(
                    "While the mode is on, closing the window sends it to the tray: games keep the DualSense",
                ),
            );
        }
        widgets::separator(ui);
        resistance(app, ui);
        if !ready && !st.on {
            ui.add_space(4.0);
            ui.label(
                RichText::new(t(
                    "No access to /dev/uhid: it needs a udev rule and the uhid module. \
                     The button below sets up both.",
                ))
                .color(theme::warn()),
            );
            ui.add_space(4.0);
            super::screen::rule_offer(app, ui);
        }
    });

    if st.on {
        ui.add_space(12.0);
        card_titled(ui, t("Now"), |ui| {
            ui.horizontal_wrapped(|ui| {
                pill(
                    ui,
                    if st.opened {
                        t("a game or Steam is connected")
                    } else {
                        t("nobody has connected yet")
                    },
                    Some(if st.opened { accent() } else { theme::faint() }),
                );
                pill(
                    ui,
                    if st.pad {
                        t("Apex passes input")
                    } else {
                        t("Apex not responding: buttons released")
                    },
                    Some(if st.pad { accent() } else { theme::warn() }),
                );
            });
            ui.add_space(8.0);
            egui::Grid::new("ds5_effects")
                .num_columns(2)
                .spacing([14.0, 6.0])
                .show(ui, |ui| {
                    for (side, name) in [(0, "LT"), (1, "RT")] {
                        ui.label(RichText::new(name).color(theme::dim()));
                        match &st.effects[side] {
                            None => ui.label(
                                RichText::new(t("the game set nothing")).color(theme::faint()),
                            ),
                            Some((e, a)) => ui.add(
                                egui::Label::new(
                                    RichText::new(format!(
                                        "{} → {}",
                                        ds5::describe(e),
                                        a.as_ref().map(ds5::describe_apex).unwrap_or_else(|| {
                                            t("not translated, the trigger stays as it was").into()
                                        })
                                    ))
                                    .color(theme::text()),
                                )
                                .wrap(),
                            ),
                        };
                        ui.end_row();
                    }
                });
            ui.add_space(6.0);
            hint(
                ui,
                t(
                    "The profile's trigger effects are off for now. Grip rumble comes from the game \
                     if it's on in the profile.",
                ),
            );
        });
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }

    ui.add_space(12.0);
    card_titled(ui, t("Starting Steam"), steam_hint);
}

/// How much of the games' resistance reaches the Apex, kept between runs.
fn resistance(app: &mut App, ui: &mut egui::Ui) {
    widgets::row(
        ui,
        t("Game resistance strength"),
        t(
            "If a trigger has to be pushed all the way (the mining beam in No Man's Sky), turn it \
             down; the build-up stays",
        ),
        |ui| {
            // left to right inside the row's right-aligned control
            let r = ui
                .allocate_ui_with_layout(
                    vec2(250.0, 30.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().slider_width = 170.0;
                        ui.add(
                            widgets::slider(&mut app.ds5_resistance, 0..=100)
                                .suffix("%")
                                .step_by(5.0),
                        )
                    },
                )
                .inner;
            // on release, not every frame of a drag
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                app.send(Cmd::DualSenseResistance(app.ds5_resistance));
            }
            r.changed()
        },
    );
}

fn steam_hint(ui: &mut egui::Ui) {
    hint(
        ui,
        t(
            "Steam opens the Apex itself and gives games it, not the DualSense. Start Steam from \
             here: it is told to leave the Apex alone (keep the game's Steam Input at its default).",
        ),
    );
    ui.add_space(8.0);
    let launch = ui.ctx().data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<Mutex<Launch>>>(egui::Id::new("steam_launch"))
            .clone()
    });
    let mut l = launch.lock().unwrap();
    if l.checked
        .is_none_or(|t| t.elapsed() > Duration::from_secs(1))
    {
        l.checked = Some(Instant::now());
        l.running = steam_running();
    }
    let (label, note, color) = match (&l.phase, l.running) {
        (Phase::Closing, _) => (t("Start Steam"), t("Closing Steam…").into(), theme::dim()),
        (Phase::Failed(e), _) => (t("Start Steam"), e.clone(), theme::warn()),
        (_, None) => (t("Start Steam"), String::new(), theme::dim()),
        (_, Some(true)) => (
            t("Start Steam"),
            t("Steam is running and leaves the Apex alone").into(),
            accent(),
        ),
        (_, Some(false)) => (
            t("Restart Steam"),
            t("Steam is running and holds the Apex: it will be closed and started again").into(),
            theme::warn(),
        ),
    };
    let idle = !matches!(l.phase, Phase::Closing) && l.running != Some(true);
    drop(l);
    ui.horizontal_wrapped(|ui| {
        if widgets::primary(ui, label, idle).clicked() {
            let launch = launch.clone();
            let ctx = ui.ctx().clone();
            launch.lock().unwrap().phase = Phase::Closing;
            std::thread::spawn(move || {
                let r = start_steam();
                let mut l = launch.lock().unwrap();
                l.phase = match r {
                    Ok(()) => Phase::Idle,
                    Err(e) => Phase::Failed(e),
                };
                l.checked = None;
                ctx.request_repaint();
            });
        }
        if widgets::secondary(ui, t("Copy command"), true)
            .on_hover_text(STEAM)
            .clicked()
        {
            ui.ctx().copy_text(STEAM.into());
        }
    });
    if !note.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new(note).size(12.5).color(color));
    }
    ui.ctx().request_repaint_after(Duration::from_secs(1));
}

/// The Steam button's progress, shared with the thread that restarts Steam.
#[derive(Default)]
struct Launch {
    phase: Phase,
    /// Last look at the running Steam: `None` = not running, `Some(true)` =
    /// running with the Apex ignored.
    running: Option<bool>,
    checked: Option<Instant>,
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Closing,
    Failed(String),
}

/// Whether Steam runs, and if so whether it was started to ignore the Apex.
fn steam_running() -> Option<bool> {
    let mut found = None;
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        let dir = e.path();
        if std::fs::read_to_string(dir.join("comm")).is_ok_and(|c| c.trim_end() == "steam") {
            let ignores = std::fs::read(dir.join("environ")).is_ok_and(|env| {
                env.split(|&b| b == 0).any(|v| {
                    v.starts_with(b"SDL_GAMECONTROLLER_IGNORE_DEVICES=")
                        && v.ends_with(b"0x37d7/0x2501")
                })
            });
            if ignores {
                return Some(true);
            }
            found = Some(false);
        }
    }
    found
}

/// Closes a running Steam and starts it with the Apex ignored, detached from
/// this app so it outlives it.
fn start_steam() -> Result<(), String> {
    if steam_running().is_some() {
        let _ = Command::new("steam")
            .arg("-shutdown")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let deadline = Instant::now() + Duration::from_secs(60);
        while steam_running().is_some() {
            if Instant::now() > deadline {
                return Err(t("Steam did not close; close it yourself and try again").into());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    let mut cmd = Command::new("steam");
    cmd.env("SDL_GAMECONTROLLER_IGNORE_DEVICES", "0x37d7/0x2501")
        .env("SDL_JOYSTICK_IGNORE_DEVICES", "0x37d7/0x2501")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe; it only leaves this app's session.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| apex5::tr!("could not start Steam: {e}", e))?;
    // reaps the launcher script when it ends; Steam itself lives on
    std::thread::spawn(move || child.wait());
    Ok(())
}
