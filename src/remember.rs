//! The lighting editor's last effect and colours, per pad and slot. The pad
//! keeps only frames, which do not always give the colours back (blends,
//! fades), so the choice is kept here and used while the frames still match.

use std::collections::BTreeMap;
use std::path::PathBuf;

use apex5::lighting::{Effect, Rgb};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Choice {
    pub effect: Effect,
    pub colours: Vec<Rgb>,
}

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("apex5-control").join("lighting.json"))
}

fn key(uid: &str, slot: u8) -> String {
    format!("{uid}/{slot}")
}

fn load() -> BTreeMap<String, Choice> {
    path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn recall(uid: &str, slot: u8) -> Option<Choice> {
    load().remove(&key(uid, slot))
}

/// Best effort: lighting works without it.
pub fn store(uid: &str, slot: u8, choice: Choice) {
    let Some(p) = path() else {
        return;
    };
    let mut all = load();
    all.insert(key(uid, slot), choice);
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(b) = serde_json::to_vec_pretty(&all) {
        let _ = std::fs::write(p, b);
    }
}
