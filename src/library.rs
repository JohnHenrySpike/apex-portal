//! Profiles kept on this computer: the 840-byte profile and its 380-byte
//! lighting as hex in a small JSON file, one file per profile.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::lang::t;
use crate::lighting::LedConfig;
use crate::profile::{Profile, SIZE};

const FORMAT: &str = "apex5-control profile";
const LED_SIZE: usize = 380;

#[derive(Serialize, Deserialize)]
struct File {
    format: String,
    version: u32,
    /// Pad model the profile was read from, for information.
    device_type: u8,
    title: String,
    profile: String,
    lighting: Option<String>,
}

pub struct Entry {
    pub path: PathBuf,
    /// File name without `.json`.
    pub name: String,
    /// Profile title inside.
    pub title: String,
    pub has_lighting: bool,
    pub modified: Option<std::time::SystemTime>,
}

pub fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("apex5-control").join("profiles"))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        bail!("odd hex length");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).context("bad hex"))
        .collect()
}

/// A file name from what the user typed: no path separators, no hidden files.
fn file_name(name: &str) -> String {
    let n: String = name
        .trim()
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let n = n.trim_start_matches('.').to_string();
    if n.is_empty() { t("profile").into() } else { n }
}

/// Saves under `name` in `dir`, never over an existing file.
pub fn save_in(
    dir: &Path,
    name: &str,
    p: &Profile,
    led: Option<&LedConfig>,
    device_type: u8,
) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let base = file_name(name);
    let path = (1..)
        .map(|i| {
            let n = if i == 1 {
                base.clone()
            } else {
                format!("{base} ({i})")
            };
            dir.join(format!("{n}.json"))
        })
        .find(|p| !p.exists())
        .unwrap();
    let f = File {
        format: FORMAT.into(),
        version: 1,
        device_type,
        title: p.title(),
        profile: hex(&p.blob),
        lighting: led.map(|l| hex(&l.blob)),
    };
    std::fs::write(&path, serde_json::to_vec_pretty(&f)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

pub fn save(name: &str, p: &Profile, led: Option<&LedConfig>, device_type: u8) -> Result<PathBuf> {
    let dir = dir().context("no home directory")?;
    save_in(&dir, name, p, led, device_type)
}

/// The profile (slot 0 until written somewhere) and its lighting.
pub fn load(path: &Path) -> Result<(Profile, Option<LedConfig>)> {
    let b = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let f: File = serde_json::from_slice(&b).context("not a profile file")?;
    if f.format != FORMAT || f.version != 1 {
        bail!("unknown profile file format");
    }
    let blob = unhex(&f.profile)?;
    if blob.len() != SIZE {
        bail!("profile is {} bytes, not {SIZE}", blob.len());
    }
    let led = match f.lighting {
        Some(h) => {
            let blob = unhex(&h)?;
            if blob.len() != LED_SIZE {
                bail!("lighting is {} bytes, not {LED_SIZE}", blob.len());
            }
            Some(LedConfig { slot: 0, blob })
        }
        None => None,
    };
    Ok((Profile { slot: 0, blob }, led))
}

/// Saved profiles, newest first; files that do not parse are left out.
pub fn list() -> Vec<Entry> {
    let Some(dir) = dir() else { return vec![] };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut out: Vec<Entry> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|path| {
            let (p, led) = load(&path).ok()?;
            Some(Entry {
                name: path.file_stem()?.to_string_lossy().into_owned(),
                title: p.title(),
                has_lighting: led.is_some(),
                modified: std::fs::metadata(&path).and_then(|m| m.modified()).ok(),
                path,
            })
        })
        .collect();
    out.sort_by_key(|e| std::cmp::Reverse(e.modified));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_profile_loads_back() {
        let dir = std::env::temp_dir().join(format!("apex5-library-{}", std::process::id()));
        let p = Profile {
            slot: 2,
            blob: (0..SIZE).map(|i| i as u8).collect(),
        };
        let led = LedConfig::factory(2);
        let a = save_in(&dir, "../Шутер", &p, Some(&led), 128).unwrap();
        let b = save_in(&dir, "../Шутер", &p, None, 128).unwrap();
        assert_eq!(a.parent().unwrap(), dir, "stays in the folder");
        assert_ne!(a, b, "never overwrites");
        let (q, l) = load(&a).unwrap();
        assert_eq!(q.blob, p.blob);
        assert_eq!(l.unwrap().blob, led.blob);
        assert!(load(&b).unwrap().1.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn names_cannot_leave_the_folder() {
        assert_eq!(file_name("../a/b"), "_a_b");
        assert_eq!(file_name("  "), "profile");
        assert_eq!(file_name(".hidden"), "hidden");
    }
}
