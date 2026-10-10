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

use anyhow::{Result, bail};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use apex5::tester::{self, Gate, SECTORS};
use apex5::{device, evdev, hid::Device, library, lighting, profile};

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
        "  title {:?} proto {:#06x} tag {}  grip-rumble master={} L={} R={}",
        p.title(),
        p.proto_version(),
        p.data_version(),
        g.master,
        g.left,
        g.right
    );
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
            for sticks in p.read()? {
                for (g, (x, y)) in out.iter_mut().zip(sticks) {
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

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
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
        _ => bail!("usage: apex5ctl info | input [seconds] | profile ... | led ..."),
    }
    Ok(())
}
