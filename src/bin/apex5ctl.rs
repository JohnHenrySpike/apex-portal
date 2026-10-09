//! Small command-line companion to the GUI.
//!
//!   apex5ctl info              identity, battery, firmware, settings, active profile
//!   apex5ctl led show
//!   apex5ctl led color RRGGBB [brightness]
//!   apex5ctl led brightness N
//!   apex5ctl led save          commit lighting to flash
//!   apex5ctl profile title T   rename the active profile (RAM only)

use anyhow::{Result, bail};
use apex5::{device, hid::Device, lighting, profile};

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
        "  title {:?} proto {:#06x} tag {}  grip-rumble master={} L={} R={} (read only)",
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

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut dev = Device::open()?;
    match a.as_slice() {
        ["info"] | [] => info(&mut dev)?,
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
        ["led", rest @ ..] => {
            let old = lighting::LedConfig::read(&mut dev)?;
            let mut new = old.clone();
            match rest {
                ["show"] => {
                    println!(
                        "mode {} brightness {} cycle {} {}x{}",
                        old.mode(),
                        old.brightness(),
                        old.cycle(),
                        old.frames(),
                        old.leds()
                    );
                    return Ok(());
                }
                ["color", c] => new.apply(lighting::Effect::Solid, &[hex_colour(c)?]),
                ["color", c, b] => {
                    new.apply(lighting::Effect::Solid, &[hex_colour(c)?]);
                    new.set_brightness(b.parse()?);
                }
                ["brightness", b] => new.set_brightness(b.parse()?),
                ["save"] => {
                    let st = profile::status(&mut dev)?;
                    profile::save(&mut dev, st.active, st.versions[st.active as usize])?;
                    println!("saved");
                    return Ok(());
                }
                _ => bail!("usage: apex5ctl led show|color RRGGBB [B]|brightness N|save"),
            }
            println!("wrote {} packets", new.write(&mut dev, Some(&old))?);
        }
        _ => bail!("usage: apex5ctl info | led ..."),
    }
    Ok(())
}
