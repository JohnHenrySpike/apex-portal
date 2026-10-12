//! Running without a window: while DualSense mode is on, closing the window
//! leaves the app in the tray, so games keep their controller. A second
//! launch knocks on a socket and gets the window back instead of fighting
//! over the pad.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use ksni::blocking::TrayMethods;

/// What the tray or a second launch asks of the app.
pub enum Wake {
    Open,
    /// Turn DualSense mode off; nothing keeps the app around after that.
    DualSenseOff,
    Quit,
}

/// The open window's context; `None` while there is no window.
pub type Window = Arc<Mutex<Option<egui::Context>>>;

const KNOCK: &[u8] = b"open\n";

/// The socket that makes this the only copy running.
pub struct Instance {
    path: Option<PathBuf>,
}

impl Instance {
    /// `None` when another copy answered: it shows its window, this one
    /// should end. Without a runtime dir there is no guard at all.
    pub fn claim(window: Window, wake: Sender<Wake>) -> Option<Instance> {
        let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") else {
            return Some(Instance { path: None });
        };
        let path = PathBuf::from(dir).join("apex5-control.sock");
        if let Ok(mut s) = UnixStream::connect(&path) {
            let _ = s.write_all(KNOCK);
            return None;
        }
        // left by a copy that did not get to clean up
        let _ = std::fs::remove_file(&path);
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("apex5: {}: {e}", path.display());
                return Some(Instance { path: None });
            }
        };
        std::thread::spawn(move || {
            for s in listener.incoming() {
                let Ok(mut s) = s else { continue };
                let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
                let mut buf = [0; 16];
                let n = s.read(&mut buf).unwrap_or(0);
                if !buf[..n].starts_with(b"open") {
                    continue;
                }
                match &*window.lock().unwrap() {
                    Some(ctx) => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    None => {
                        let _ = wake.send(Wake::Open);
                    }
                }
            }
        });
        Some(Instance { path: Some(path) })
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        if let Some(p) = &self.path {
            let _ = std::fs::remove_file(p);
        }
    }
}

struct Icon {
    wake: Sender<Wake>,
}

impl ksni::Tray for Icon {
    fn id(&self) -> String {
        "apex5-control".into()
    }

    fn title(&self) -> String {
        "Apex 5 Control".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn icon_name(&self) -> String {
        "input-gaming".into()
    }

    /// For icon themes without `input-gaming`.
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        pixmap(32).into_iter().collect()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Apex 5 Control".into(),
            description: "Режим DualSense включён: игры видят DualSense".into(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.wake.send(Wake::Open);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{CheckmarkItem, StandardItem};
        vec![
            StandardItem {
                label: "Открыть окно".into(),
                icon_name: "window-new".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.wake.send(Wake::Open);
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Режим DualSense".into(),
                checked: true,
                activate: Box::new(|t: &mut Self| {
                    let _ = t.wake.send(Wake::DualSenseOff);
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Выйти".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.wake.send(Wake::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// The tray icon, there while the value lives.
pub struct Tray(ksni::blocking::Handle<Icon>);

impl Tray {
    /// `None` when the desktop shows no tray.
    pub fn show(wake: Sender<Wake>) -> Option<Tray> {
        match (Icon { wake }).spawn() {
            Ok(h) => Some(Tray(h)),
            Err(e) => {
                eprintln!("apex5: tray: {e}");
                None
            }
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.0.shutdown().wait();
    }
}

/// A plain gamepad outline, white, as ARGB32 for the tray.
fn pixmap(px: u32) -> Option<ksni::Icon> {
    use resvg::{tiny_skia, usvg};
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
<path fill="#e1e8f2" fill-rule="evenodd" d="M9 8h14c4 0 6.5 3 7.3 8l.9 6c.5 3.3-1.4 5-3.4 5-1.6 0-2.7-1-3.8-2.6L22.4 22H9.6L8 24.4C6.9 26 5.8 27 4.2 27c-2 0-3.9-1.7-3.4-5l.9-6C2.5 11 5 8 9 8zm-.5 4.5v2h-2v2h2v2h2v-2h2v-2h-2v-2zm13.5.5a1.3 1.3 0 1 0 0 2.6 1.3 1.3 0 0 0 0-2.6zm2.6 2.6a1.3 1.3 0 1 0 0 2.6 1.3 1.3 0 0 0 0-2.6z"/>
</svg>"##;
    let tree = usvg::Tree::from_str(SVG, &usvg::Options::default()).ok()?;
    let mut pm = tiny_skia::Pixmap::new(px, px)?;
    let k = px as f32 / 32.0;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(k, k),
        &mut pm.as_mut(),
    );
    // premultiplied RGBA → straight ARGB, big-endian
    let data = pm
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.alpha(), c.red(), c.green(), c.blue()]
        })
        .collect();
    Some(ksni::Icon {
        width: px as i32,
        height: px as i32,
        data,
    })
}
