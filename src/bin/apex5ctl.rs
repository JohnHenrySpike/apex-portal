//! Small command-line companion to the GUI.
//!
//!   apex5ctl info              identity, battery, firmware, settings, active profile
//!   apex5ctl led show
//!   apex5ctl led slots         read the lighting of all four slots, keep the active one
//!   apex5ctl led color RRGGBB [brightness]
//!   apex5ctl led brightness N
//!   apex5ctl led probe         light each LED slot in turn (Enter for the next), then restore
//!   apex5ctl led save          commit lighting to flash
//!   apex5ctl profile switch N  make slot N (1-4) the running profile
//!   apex5ctl profile title T   rename the active profile (RAM only)
//!   apex5ctl profile backup    save all four slots and their lighting to files
//!   apex5ctl input [seconds]   count stream reports and how far the sticks reach
//!   apex5ctl keys [seconds]    buttons as games see them (after the profile);
//!                              runs beside the GUI, it does not open hidraw
//!   apex5ctl screen preview FILE OUT.png [whole]
//!                              the frames as the screen will show them (no pad needed)
//!   apex5ctl screen upload FILE [ms] [whole]
//!                              send a picture, GIF or Flydigi's factory .bin to the
//!                              screen; cable only, asks first, takes ~25 s per frame

use anyhow::{Result, bail};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use apex5::tester::{self, Gate, SECTORS};
use apex5::{device, evdev, hid, hid::Device, library, lighting, profile, screen};

fn hex_colour(s: &str) -> Result<lighting::Rgb> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        bail!("colour must be RRGGBB");
    }
    let v = u32::from_str_radix(s, 16)?;
    Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn info(dev: &mut Device) -> Result<()> {
    let i = device::read_info(dev)?;
    println!(
        "model      {} (type {}){}",
        i.model(),
        i.device_type,
        if i.supported() {
            ""
        } else {
            " — UNSUPPORTED"
        }
    );
    println!("connection {}", if i.wired { "cable" } else { "dongle" });
    println!(
        "battery    {}/5{}",
        i.battery_level,
        if i.charging { " charging" } else { "" }
    );
    println!("name       {:?}", i.nickname);
    println!("uid        {}", i.uid);
    for (n, v) in &i.firmware {
        println!("fw {n:<8}{v}");
    }
    let s = device::read_settings(dev)?;
    println!("settings   {s:?}");
    println!("flags      {:?}", device::read_flags(dev)?);
    let st = profile::status(dev)?;
    println!(
        "profile    active {} versions {:?}",
        st.active + 1,
        st.versions
    );
    // Reading the active slot does not switch anything.
    let p = profile::read(dev, st.active)?;
    let g = p.grip_vibration();
    println!(
        "  title {:?} proto {:#06x} tag {}  grip-rumble master={}",
        p.title(),
        p.proto_version(),
        p.data_version(),
        g.master,
    );
    for (name, m) in [("L", g.left), ("R", g.right)] {
        println!(
            "  grip {name}: on={} window {}..{} scale {}%",
            m.on, m.min, m.max, m.scale
        );
    }
    for side in 0..2 {
        println!("  stick {side}: {:?}", p.stick(side));
        println!(
            "  travel {side}: {:?}  effect {:?}",
            p.travel(side),
            p.force(side)
        );
    }
    println!("  gyro {:?}", p.gyro());
    for (id, name) in profile::PAD_KEYS {
        let k = p.key(id);
        if k.target != profile::TARGET_IDENTITY || k.turbo_freq != 0 {
            println!("  key {name}: {k:?}");
        }
    }
    for m in p.macros() {
        println!("  macro {m:?}");
    }
    Ok(())
}

/// Everything the vendor node sends for a while, with the stream on.
fn input(dev: &mut Device, secs: u64) -> Result<()> {
    let was_on = device::read_flags(dev)?.raw_data;
    if !was_on {
        device::set_stream(dev, true)?;
    }
    dev.drain();
    println!("reading for {secs} s: circle both sticks along the edge");
    let mut kinds: BTreeMap<u8, u32> = BTreeMap::new();
    let (mut same, mut prev) = (0u32, Vec::new());
    let mut gates = [Gate::default(); 2];
    let mut out = [Gate::default(); 2];
    let mut pad = match evdev::find(&dev.path).and_then(|p| evdev::Pad::open(&p)) {
        Ok(p) => Some(p),
        Err(e) => {
            println!("no gamepad node: {e:#}");
            None
        }
    };
    let start = Instant::now();
    let until = start + Duration::from_secs(secs);
    let mut read = Ok(());
    while Instant::now() < until {
        match dev.read_raw(Duration::from_millis(50)) {
            Ok(Some(raw)) => {
                *kinds.entry(raw.get(3).copied().unwrap_or(0)).or_default() += 1;
                if raw == prev {
                    same += 1;
                }
                if let Some(s) = device::InputState::parse(&raw) {
                    gates[0].add(tester::axis(s.lx), tester::axis(s.ly));
                    gates[1].add(tester::axis(s.rx), tester::axis(s.ry));
                }
                prev = raw;
            }
            Ok(None) => {}
            Err(e) => {
                read = Err(e);
                break;
            }
        }
        if let Some(p) = pad.as_mut() {
            for o in p.read()? {
                for (g, (x, y)) in out.iter_mut().zip(o.sticks) {
                    g.add(x, y);
                }
            }
        }
    }
    let took = start.elapsed().as_secs_f32();
    if !was_on {
        device::set_stream(dev, false)?;
    }
    read?;
    for (cmd, n) in &kinds {
        println!(
            "cmd {cmd:#04x}  {n:>6} reports  {:>6.0} Hz",
            *n as f32 / took
        );
    }
    println!("repeats of the previous report: {same}");
    let rows = [("left", &gates[0]), ("right", &gates[1])]
        .into_iter()
        .chain(
            pad.is_some()
                .then_some([("left out", &out[0]), ("right out", &out[1])])
                .into_iter()
                .flatten(),
        );
    for (name, g) in rows {
        let dirs: Vec<String> = (0..8)
            .map(|k| format!("{:.2}", g.reach[k * SECTORS / 8]))
            .collect();
        let err = g.error().map_or("-".into(), |e| format!("{e:.1} %"));
        println!(
            "{name:<9}  reach at 0°,45°..315°: {}  covered {:.0} %  off-circle {err}",
            dirs.join(" "),
            g.coverage() * 100.0
        );
    }
    Ok(())
}

/// xpad's names for what it reports: keys, the d-pad hat and the triggers.
fn control(kind: u16, code: u16, value: i32) -> Option<String> {
    let key = |c| match c {
        304 => "A",
        305 => "B",
        307 => "X",
        308 => "Y",
        310 => "LB",
        311 => "RB",
        314 => "View",
        315 => "Menu",
        316 => "Home",
        317 => "L3",
        318 => "R3",
        704 => "←",
        705 => "→",
        706 => "↑",
        707 => "↓",
        _ => "",
    };
    match (kind, code) {
        (evdev::EV_KEY, c) => Some(match key(c) {
            "" => format!("key {c}"),
            n => n.into(),
        }),
        (evdev::EV_ABS, 16) => Some(if value < 0 { "←" } else { "→" }.into()),
        (evdev::EV_ABS, 17) => Some(if value < 0 { "↑" } else { "↓" }.into()),
        (evdev::EV_ABS, 2) => Some("LT".into()),
        (evdev::EV_ABS, 5) => Some("RT".into()),
        _ => None,
    }
}

/// Presses and releases on the gamepad node, then presses per control and
/// their rate (turbo shows as a steady rate while one button is held).
fn keys(secs: u64) -> Result<()> {
    let node = evdev::find(&hid::find_device()?)?;
    let mut pad = evdev::Pad::open(&node)?;
    pad.events()?;
    println!("{}: reading for {secs} s", node.display());
    // per control: is it down, times it went down
    let mut seen: BTreeMap<String, (bool, Vec<f64>)> = BTreeMap::new();
    // the hat axes report one direction at a time; remember which
    let mut hat: [Option<String>; 2] = [None, None];
    let mut start = None;
    let until = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < until {
        for ev in pad.events()? {
            let hat_axis = (ev.kind == evdev::EV_ABS && matches!(ev.code, 16 | 17))
                .then(|| (ev.code - 16) as usize);
            let (name, down) = if let Some(h) = hat_axis {
                match (ev.value, hat[h].take()) {
                    (0, Some(n)) => (n, false),
                    (0, None) => continue,
                    (v, _) => {
                        let n = control(ev.kind, ev.code, v).unwrap();
                        hat[h] = Some(n.clone());
                        (n, true)
                    }
                }
            } else {
                match control(ev.kind, ev.code, ev.value) {
                    Some(n) => (n, ev.value != 0),
                    None => continue,
                }
            };
            let e = seen.entry(name.clone()).or_default();
            if e.0 == down {
                continue;
            }
            e.0 = down;
            let t0 = *start.get_or_insert(ev.time);
            if down {
                e.1.push(ev.time);
            }
            println!(
                "{:>8.3} s  {name:<5} {}",
                ev.time - t0,
                if down { "down" } else { "up" }
            );
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    println!();
    for (name, (_, downs)) in &seen {
        let mut gaps: Vec<f64> = downs.windows(2).map(|w| w[1] - w[0]).collect();
        gaps.sort_by(f64::total_cmp);
        let rate = gaps.get(gaps.len() / 2).map_or(String::new(), |g| {
            format!("  median {:.1} presses/s", 1.0 / g)
        });
        println!("{name:<5} {:>4} presses{rate}", downs.len());
    }
    Ok(())
}

/// The frames to send: a factory `.bin` as it is, anything else decoded and
/// fitted. Returns the upload and how it was made.
fn screen_frames(file: &str, rest: &[&str]) -> Result<(screen::Upload, String)> {
    let path = std::path::Path::new(file);
    if path.extension().is_some_and(|e| e == "bin") {
        let up = screen::Upload::factory(std::fs::read(path)?)?;
        let what = format!("factory animation, {} frames", up.frames);
        return Ok((up, what));
    }
    let fit = if rest.contains(&"whole") {
        screen::Fit::Whole
    } else {
        screen::Fit::Fill
    };
    let ms = rest.iter().find_map(|a| a.parse::<u32>().ok());
    let src = screen::decode(path)?;
    let frames: Vec<Vec<u8>> = src.frames.iter().map(|f| screen::encode(f, fit)).collect();
    let interval = ms.or(src.interval_ms).unwrap_or(100);
    let up = screen::Upload::new(&frames, interval)?;
    let mut what = format!("{} frame(s), {} ms", up.frames, up.frame_rate as u32 * 10);
    if src.cut {
        what += &format!(", only the first {}", screen::MAX_FRAMES);
    }
    Ok((up, what))
}

fn screen_preview(file: &str, out: &str, rest: &[&str]) -> Result<()> {
    let (up, what) = screen_frames(file, rest)?;
    // up to 12 frames, 3 across, twice the size
    let n = up.frames.min(12) as u32;
    let (w, h) = (screen::W * 2, screen::H * 2);
    let cols = n.min(3);
    let mut sheet = image::RgbaImage::new(cols * (w + 8), n.div_ceil(3) * (h + 8));
    for i in 0..n {
        let f = screen::to_rgba(up.frame(i as usize));
        let big = image::imageops::resize(&f, w, h, image::imageops::FilterType::Nearest);
        let (x, y) = ((i % 3) * (w + 8), (i / 3) * (h + 8));
        image::imageops::overlay(&mut sheet, &big, x as i64, y as i64);
    }
    sheet.save(out)?;
    println!("{what}; wrote {out}");
    Ok(())
}

fn screen_upload(file: &str, rest: &[&str]) -> Result<()> {
    let (up, what) = screen_frames(file, rest)?;
    let minutes = (up.frames as u32 * screen::SECONDS_PER_FRAME).div_ceil(60);
    println!("{what}. About {minutes} min. Keep the cable in and the pad on.");
    println!("Close the GUI first. Type yes to upload:");
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if answer.trim() != "yes" {
        bail!("not uploaded");
    }
    let dev = if screen::find_port().is_some() {
        None
    } else {
        Some(Device::open()?)
    };
    let started = Instant::now();
    let mut shown = usize::MAX;
    screen::upload(dev, &up, &mut |s| match s {
        screen::Stage::Switching => println!("waiting for the screen's port…"),
        screen::Stage::Writing(done, total) => {
            if done * 100 / total != shown {
                shown = done * 100 / total;
                println!("{shown}% ({:.0} s)", started.elapsed().as_secs_f32());
            }
        }
        screen::Stage::Saving => println!("done; the screen saves for ~15 s and the pad restarts"),
    })
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    // Only the gamepad node: the GUI may keep the vendor node open.
    match a.as_slice() {
        ["keys"] => return keys(10),
        ["keys", secs] => return keys(secs.parse()?),
        ["screen", "preview", file, out, rest @ ..] => return screen_preview(file, out, rest),
        ["screen", "upload", file, rest @ ..] => return screen_upload(file, rest),
        _ => {}
    }
    let mut dev = Device::open()?;
    match a.as_slice() {
        ["info"] | [] => info(&mut dev)?,
        ["input"] => input(&mut dev, 5)?,
        ["input", secs] => input(&mut dev, secs.parse()?)?,
        ["profile", "switch", n] => {
            let slot: u8 = n.parse()?;
            if !(1..=profile::SLOTS).contains(&slot) {
                bail!("slot must be 1..{}", profile::SLOTS);
            }
            // Reading a slot is what makes it the running one.
            profile::read(&mut dev, slot - 1)?;
            println!("active slot {}", profile::status(&mut dev)?.active + 1);
        }
        ["profile", "backup"] => {
            let info = device::read_info(&mut dev)?;
            let before = profile::status(&mut dev)?.active;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            let mut saved = Ok(());
            for slot in 0..profile::SLOTS {
                // reading switches the running slot; restored below
                let r = profile::read(&mut dev, slot).and_then(|p| {
                    let led = lighting::LedConfig::read(&mut dev, slot)?;
                    let name = format!("backup-{stamp}-P{}", slot + 1);
                    let path = library::save(&name, &p, Some(&led), info.device_type)?;
                    Ok((path, p.title()))
                });
                match r {
                    Ok((path, title)) => println!("P{}  {title:<12} {}", slot + 1, path.display()),
                    Err(e) => {
                        saved = Err(e);
                        break;
                    }
                }
            }
            profile::read(&mut dev, before)?;
            println!("active slot {}", profile::status(&mut dev)?.active + 1);
            saved?;
        }
        ["profile", "title", name] => {
            // Working memory only; not committed to flash.
            let st = profile::status(&mut dev)?;
            let old = profile::read(&mut dev, st.active)?;
            let mut new = old.clone();
            new.set_title(name);
            let n = profile::write(&mut dev, &new, &old)?;
            let back = profile::read(&mut dev, st.active)?;
            println!("wrote {n} packets; pad now says {:?}", back.title());
        }
        ["led", "slots"] => {
            let before = profile::status(&mut dev)?.active;
            let leds: Vec<_> = (0..profile::SLOTS)
                .map(|slot| lighting::LedConfig::read(&mut dev, slot))
                .collect::<Result<_>>()?;
            // Reading 167 makes that slot the running one: go back.
            let after = profile::status(&mut dev)?.active;
            profile::read(&mut dev, before)?;
            let restored = profile::status(&mut dev)?.active;
            println!(
                "active slot {} before, {} after reading, {} restored",
                before + 1,
                after + 1,
                restored + 1
            );
            for (slot, l) in leds.iter().enumerate() {
                let same: Vec<_> = (0..slot)
                    .filter(|&o| leds[o] == *l)
                    .map(|o| o + 1)
                    .collect();
                println!(
                    "slot {}: mode {} brightness {} cycle {} {:?} first LED {:?}{}",
                    slot + 1,
                    l.mode(),
                    l.brightness(),
                    l.cycle(),
                    l.effect(),
                    l.led(0, 0),
                    if same.is_empty() {
                        String::new()
                    } else {
                        format!("  (same as slot {same:?})")
                    }
                );
            }
        }
        ["led", rest @ ..] => {
            let st = profile::status(&mut dev)?;
            let old = lighting::LedConfig::read(&mut dev, st.active)?;
            let mut new = old.clone();
            match rest {
                ["show"] => {
                    println!(
                        "mode {} brightness {} cycle {} {}x{} flare-with-rumble {}",
                        old.mode(),
                        old.brightness(),
                        old.cycle(),
                        old.frames(),
                        old.leds(),
                        old.grip_sync()
                    );
                    return Ok(());
                }
                ["color", c] => new.apply(lighting::Effect::Solid, &[hex_colour(c)?]),
                ["color", c, b] => {
                    new.apply(lighting::Effect::Solid, &[hex_colour(c)?]);
                    new.set_brightness(b.parse()?);
                }
                ["brightness", b] => new.set_brightness(b.parse()?),
                ["probe"] => {
                    // RAM only: the pad drops this on sleep, and the end
                    // writes back what was there.
                    let mut prev = old.clone();
                    for slot in 0..old.leds() {
                        let mut one = old.clone();
                        one.single(slot, [255, 255, 255]);
                        one.set_brightness(60);
                        one.write(&mut dev, Some(&prev))?;
                        prev = one;
                        println!("slot {slot} is lit; Enter for the next");
                        std::io::stdin().read_line(&mut String::new())?;
                    }
                    old.write(&mut dev, Some(&prev))?;
                    println!("lighting restored");
                    return Ok(());
                }
                ["save"] => {
                    profile::save(&mut dev, st.active, st.versions[st.active as usize])?;
                    println!("saved");
                    return Ok(());
                }
                _ => bail!("usage: apex5ctl led show|color RRGGBB [B]|brightness N|probe|save"),
            }
            println!("wrote {} packets", new.write(&mut dev, Some(&old))?);
        }
        _ => {
            bail!(
                "usage: apex5ctl info | input [seconds] | keys [seconds] | profile ... | led ... | screen ..."
            )
        }
    }
    Ok(())
}
