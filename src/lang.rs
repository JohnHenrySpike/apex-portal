//! The app's language. Texts are written in English in the code, and each
//! `locales/<code>.json` gives them in another language, keyed by the English
//! text: `t("Save")`, or `tr!("Profile {slot}", slot)` to fill in values.
//!
//! A key may end in `#context` to tell apart the same English words that
//! another language says differently ("Off#turbo", "Off#gyro"); English shows
//! the part before `#`. A text missing from a translation stays English.
//!
//! The files in `locales/` are built in. Files in
//! `~/.config/apex5-control/locales/` are read on top of them, so a
//! translation can be tried without rebuilding the app.

use std::collections::HashMap;
use std::fmt::{Display, Write};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

// `BUILT_IN: &[(&str, &str)]`: the code and text of each `locales/*.json`.
include!(concat!(env!("OUT_DIR"), "/locales.rs"));

/// The language the texts are written in.
pub const ENGLISH: &str = "en";

/// What a `locales/<code>.json` holds.
#[derive(serde::Deserialize)]
struct File {
    /// The language's own name, for the language menu.
    language: String,
    /// English text (the key) → the text in this language.
    strings: HashMap<String, String>,
}

struct Locale {
    code: String,
    name: String,
    strings: HashMap<String, String>,
}

/// The translations, read once.
fn locales() -> &'static [Locale] {
    static ALL: OnceLock<Vec<Locale>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut all: Vec<Locale> = Vec::new();
        let mut add = |code: &str, text: &str| {
            let f: File = match serde_json::from_str(text) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("locale {code}: {e}");
                    return;
                }
            };
            match all.iter_mut().find(|l| l.code == code) {
                Some(l) => {
                    l.name = f.language;
                    l.strings.extend(f.strings);
                }
                None => all.push(Locale {
                    code: code.into(),
                    name: f.language,
                    strings: f.strings,
                }),
            }
        };
        for (code, text) in BUILT_IN {
            add(code, text);
        }
        if let Some(dir) = config_dir().map(|d| d.join("locales"))
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for p in paths {
                if p.extension().is_some_and(|x| x == "json")
                    && let Some(code) = p.file_stem().and_then(|s| s.to_str())
                    && code != ENGLISH
                    && let Ok(text) = std::fs::read_to_string(&p)
                {
                    add(code, &text);
                }
            }
        }
        all
    })
}

/// The picked translation's place in `locales()` plus one; 0 is English.
static CURRENT: AtomicUsize = AtomicUsize::new(0);

fn current_locale() -> Option<&'static Locale> {
    match CURRENT.load(Ordering::Relaxed) {
        0 => None,
        i => locales().get(i - 1),
    }
}

/// The languages to choose from, English first: code and own name.
pub fn languages() -> Vec<(&'static str, &'static str)> {
    let mut v = vec![(ENGLISH, "English")];
    v.extend(locales().iter().map(|l| (l.code.as_str(), l.name.as_str())));
    v
}

/// The code of the language in use.
pub fn current() -> &'static str {
    current_locale().map_or(ENGLISH, |l| l.code.as_str())
}

/// Switches to a language; false if there is no such translation.
fn set(code: &str) -> bool {
    if code == ENGLISH {
        CURRENT.store(0, Ordering::Relaxed);
        return true;
    }
    match locales().iter().position(|l| l.code == code) {
        Some(i) => {
            CURRENT.store(i + 1, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// The text of a key in the current language.
pub fn t(key: &'static str) -> &'static str {
    current_locale()
        .and_then(|l| l.strings.get(key))
        .filter(|s| !s.is_empty())
        .map_or_else(|| english(key), String::as_str)
}

/// Marks a key in a table, to be passed to [`t`] where it is shown; the
/// translation tools find keys by `t(`, `tr!(` and `key(`.
pub const fn key(k: &'static str) -> &'static str {
    k
}

fn english(key: &str) -> &str {
    key.split_once('#').map_or(key, |(en, _)| en)
}

/// [`t`], then each `{name}` replaced by its value; `{{` and `}}` are braces.
pub fn fill(key: &'static str, values: &[(&str, &dyn Display)]) -> String {
    let pattern = t(key);
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut rest = pattern;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        if rest.starts_with("{{") || rest.starts_with("}}") {
            out.push_str(&rest[..1]);
            rest = &rest[2..];
            continue;
        }
        let value = rest
            .strip_prefix('{')
            .and_then(|r| r.split_once('}'))
            .and_then(|(name, after)| {
                let (_, v) = values.iter().find(|(n, _)| *n == name)?;
                Some((v, after))
            });
        match value {
            Some((v, after)) => {
                let _ = write!(out, "{v}");
                rest = after;
            }
            None => {
                out.push_str(&rest[..1]);
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A key with `{name}` places, filled in the current language:
/// `tr!("Profile {slot} · {title}", slot = n + 1, title)`.
#[macro_export]
macro_rules! tr {
    (@value $name:ident) => {
        $name
    };
    (@value $name:ident = $value:expr) => {
        $value
    };
    ($key:literal $(, $name:ident $(= $value:expr)?)* $(,)?) => {
        $crate::lang::fill(
            $key,
            &[$((
                stringify!($name),
                &$crate::tr!(@value $name $(= $value)?) as &dyn ::std::fmt::Display,
            )),*],
        )
    };
}

fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("apex5-control"))
}

fn choice_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("language"))
}

/// Takes the language picked earlier, if any; `APEX5_LANG=<code>` overrides it
/// for this run.
pub fn load() {
    let picked = std::env::var("APEX5_LANG")
        .ok()
        .or_else(|| choice_path().and_then(|p| std::fs::read_to_string(p).ok()));
    if !picked.is_some_and(|c| set(c.trim())) {
        set(ENGLISH);
    }
}

/// Switches and remembers the language; remembering is best effort.
pub fn pick(code: &str) {
    if !set(code) {
        return;
    }
    let Some(p) = choice_path() else {
        return;
    };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, code);
}

/// The `{name}` places of a text.
pub fn places(text: &str) -> Vec<&str> {
    let mut v = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        rest = &rest[i..];
        if rest.starts_with("{{") || rest.starts_with("}}") {
            rest = &rest[2..];
        } else if let Some((name, after)) = rest.strip_prefix('{').and_then(|r| r.split_once('}')) {
            v.push(name);
            rest = after;
        } else {
            rest = &rest[1..];
        }
    }
    v.sort_unstable();
    v
}

// Tests stay in English: the language is global and tests run at once.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_by_default() {
        let slot = 3;
        assert_eq!(t("Save"), "Save");
        assert_eq!(t("Off#turbo"), "Off");
        assert_eq!(tr!("Profile {slot}", slot), "Profile 3");
        assert_eq!(
            tr!("P{slot} · {title}", slot = slot + 1, title = "Race"),
            "P4 · Race"
        );
    }

    #[test]
    fn fills_places() {
        let v: [(&str, &dyn Display); 2] = [("a", &1), ("b", &"x")];
        assert_eq!(fill("{a}-{b}-{a}", &v), "1-x-1");
        assert_eq!(fill("{{a}} {c} {", &v), "{a} {c} {");
        assert_eq!(fill("}} {b}}", &v), "} x}");
    }

    /// Every built-in translation reads, and keeps the places of its key.
    #[test]
    fn built_in_locales_match_their_keys() {
        for (code, text) in BUILT_IN {
            let f: File = serde_json::from_str(text).unwrap_or_else(|e| panic!("{code}: {e}"));
            assert!(!f.language.is_empty(), "{code}: no language name");
            for (key, text) in &f.strings {
                if text.is_empty() {
                    continue;
                }
                assert_eq!(
                    places(english(key)),
                    places(text),
                    "{code}: places differ in {key:?}"
                );
            }
        }
    }
}
