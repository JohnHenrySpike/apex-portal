//! What the app takes from Flydigi's own server (`api.flydigi.com/pc`), the
//! same requests Flydigi Space Station makes: firmware news, the official
//! screen pictures, and the games' trigger vibration presets. Nothing goes
//! out unless the user asks for it, no account or token is involved, and
//! only the model and firmware versions are sent (for the firmware check).
//! Files are kept in `~/.cache/apex5-control/flydigi/`.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::device::Info;
use crate::profile::TriggerBind;

const API: &str = "https://api.flydigi.com/pc";
/// Space Station's version: the server answers by it.
const APP_VERSION: &str = "4.2.0.9";
const DEVICE_CODE: &str = "k5";
/// Pictures are tens of kilobytes; this only stops a runaway download.
const MAX_FILE: u64 = 16 << 20;

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

const API_TIMEOUT: Duration = Duration::from_secs(20);
/// Flydigi's CDN can take seconds for a few kilobytes.
const FILE_TIMEOUT: Duration = Duration::from_secs(60);

/// Space Station's headers; the server needs no others.
fn with_headers<B>(r: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
    r.header("language", "ru").header("appVersion", APP_VERSION)
}

/// The envelope is `{code|status, message, data}`; 0 or 200 is success.
fn unwrap(body: &str) -> Result<Value> {
    let v: Value = serde_json::from_str(body).context("сервер ответил не JSON")?;
    let code = v
        .get("code")
        .or_else(|| v.get("status"))
        .and_then(Value::as_i64);
    if !matches!(code, Some(0 | 200)) {
        let msg = v
            .get("message")
            .or_else(|| v.get("msg"))
            .and_then(Value::as_str);
        bail!("сервер ответил ошибкой: {}", msg.unwrap_or("без описания"));
    }
    Ok(v.get("data").cloned().unwrap_or(Value::Null))
}

fn get(path: &str) -> Result<Value> {
    let mut r = with_headers(agent(API_TIMEOUT).get(format!("{API}{path}")))
        .call()
        .context("нет связи с сервером Flydigi")?;
    unwrap(&r.body_mut().read_to_string()?)
}

fn post(path: &str, body: &Value) -> Result<Value> {
    let mut r = with_headers(agent(API_TIMEOUT).post(format!("{API}{path}")))
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .context("нет связи с сервером Flydigi")?;
    unwrap(&r.body_mut().read_to_string()?)
}

pub fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("apex5-control").join("flydigi"))
}

fn text(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn number(v: &Value, key: &str) -> i64 {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

// ------------------------------------------------------------------ firmware

/// A chip whose firmware on the server differs from the pad's.
#[derive(Clone, Debug)]
pub struct FirmwareNews {
    /// The chip as `Info::firmware` names it.
    pub chip: &'static str,
    pub current: String,
    pub latest: String,
    /// Flydigi's release notes, usually in Chinese.
    pub notes: String,
    /// Space Station offers it by itself on connect.
    pub pushed: bool,
}

/// `Info::firmware` names against the request's fields; ADC is not sent.
const CHIPS: [(&str, &str); 6] = [
    ("main", "main_chip"),
    ("dongle", "dongle_chip"),
    ("switch", "si_chip"),
    ("trigger", "trigger_chip"),
    ("screen", "screen_chip"),
    ("NearLink", "rf_chip"),
];

/// Asks the server about the pad's chips. It answers only for the chips
/// whose version differs from the one sent, so an empty list means all are
/// current. Nothing is downloaded: firmware is flashed by Space Station.
pub fn firmware(info: &Info) -> Result<Vec<FirmwareNews>> {
    let mut body = json!({
        "device_code": DEVICE_CODE,
        "device_id": info.device_type,
        "app_version": APP_VERSION,
    });
    let mut sent = Vec::new();
    for (name, version) in &info.firmware {
        if let Some((chip, field)) = CHIPS.iter().find(|(n, _)| n == name) {
            body[*field] = json!(version);
            sent.push((*chip, *field, version.clone()));
        }
    }
    let data = post("/Update/firmware", &body)?;
    // The notes are Flydigi's, in Chinese, so the app does not show them;
    // the whole answer stays on disk for whoever wants to read it.
    if let Some(path) = firmware_answer() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, serde_json::to_string_pretty(&data)?);
    }
    Ok(firmware_news(&data, &sent))
}

/// Where the last firmware answer is kept.
pub fn firmware_answer() -> Option<PathBuf> {
    Some(cache_dir()?.join("firmware.json"))
}

fn firmware_news(data: &Value, sent: &[(&'static str, &str, String)]) -> Vec<FirmwareNews> {
    let list = &data["chip_list"];
    sent.iter()
        .filter_map(|(chip, field, current)| {
            let c = list.get(*field).filter(|c| c.is_object())?;
            let latest = text(c, "version");
            (!latest.is_empty() && latest != *current).then(|| FirmwareNews {
                chip,
                current: current.clone(),
                latest,
                notes: text(c, "info"),
                pushed: number(c, "is_push") == 1 || c["is_push"] == json!(true),
            })
        })
        .collect()
}

// ------------------------------------------------------------------ screen pictures

/// One of Flydigi's official screen pictures or animations.
#[derive(Clone, Debug)]
pub struct Picture {
    pub id: i64,
    pub title: String,
    pub url: String,
}

impl Picture {
    /// Where the file is kept once downloaded.
    pub fn path(&self) -> Option<PathBuf> {
        let ext = self
            .url
            .rsplit('/')
            .next()
            .and_then(|f| f.rsplit_once('.'))
            .map(|(_, e)| e.to_ascii_lowercase())
            .filter(|e| matches!(e.as_str(), "gif" | "png" | "jpg" | "jpeg"))
            .unwrap_or_else(|| "img".into());
        Some(
            cache_dir()?
                .join("screen")
                .join(format!("{}.{ext}", self.id)),
        )
    }

    /// The file, downloaded once and then taken from the cache.
    pub fn fetch(&self) -> Result<PathBuf> {
        let path = self.path().context("не найден домашний каталог")?;
        if path.metadata().is_ok_and(|m| m.len() > 0) {
            return Ok(path);
        }
        let bytes = download(&self.url)?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        let tmp = path.with_extension("part");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    }
}

/// The official pictures for the Apex 5's screen, in the server's order.
pub fn pictures() -> Result<Vec<Picture>> {
    let data = get("/screen_pic/list?version=0")?;
    Ok(parse_pictures(&data))
}

fn parse_pictures(data: &Value) -> Vec<Picture> {
    data.as_array()
        .into_iter()
        .flatten()
        .filter(|p| {
            let code = text(p, "device_code");
            code.is_empty() || code.eq_ignore_ascii_case(DEVICE_CODE)
        })
        .filter_map(|p| {
            let url = text(p, "imagePath");
            url.starts_with("https://").then(|| Picture {
                id: number(p, "id"),
                title: picture_title(&text(p, "title")),
                url,
            })
        })
        .collect()
}

/// The server's titles are Chinese catalogue labels; the common ones are
/// translated.
fn picture_title(t: &str) -> String {
    const NAMES: [(&str, &str); 4] = [
        ("精选动画", "Анимация "),
        ("精选图片", "Картинка "),
        ("第三方无版权", "Свободная картинка "),
        ("蛇年", "Год змеи"),
    ];
    for (zh, ru) in NAMES {
        if let Some(rest) = t.strip_prefix(zh) {
            return format!("{ru}{rest}").trim().to_string();
        }
    }
    t.to_string()
}

/// A file from one of Flydigi's CDN links, tried twice.
pub fn download(url: &str) -> Result<Vec<u8>> {
    if !url.starts_with("https://") {
        bail!("ссылка не по https: {url}");
    }
    let once = || -> Result<Vec<u8>> {
        let mut r = agent(FILE_TIMEOUT).get(url).call()?;
        Ok(r.body_mut().with_config().limit(MAX_FILE).read_to_vec()?)
    };
    once().or_else(|_| once()).context("файл не скачался")
}

// ------------------------------------------------------------------ game trigger presets

/// A game's "vibrating triggers" preset: the game's grip rumble bound into
/// the triggers, as Space Station binds it while the game runs.
#[derive(Clone, Debug, PartialEq)]
pub struct GamePreset {
    pub id: i64,
    pub name: String,
    /// Left and right bind, as command 82 sends them.
    pub bind: [TriggerBind; 2],
}

/// Games from Flydigi's adaptive-trigger list that have a vibration preset.
pub fn game_presets() -> Result<Vec<GamePreset>> {
    let data = get("/adapter_trigger/list?version=0")?;
    if let Some(dir) = cache_dir() {
        // kept for reference; the app always asks the server again
        let _ = std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(dir.join("adapter_trigger.json"), data.to_string()));
    }
    Ok(parse_games(&data))
}

/// "a,b,c,d,e" into five bytes; what does not parse is 0, as in Space Station.
fn params(s: &str) -> [u8; 5] {
    let mut out = [0; 5];
    for (o, v) in out.iter_mut().zip(s.split(',')) {
        *o = v.trim().parse().unwrap_or(0);
    }
    out
}

fn parse_games(data: &Value) -> Vec<GamePreset> {
    let mut games: Vec<GamePreset> = data
        .as_array()
        .into_iter()
        .flatten()
        // bind type 2 is the only one the pad's profile keeps (and the only
        // one the list uses)
        .filter(|g| number(g, "isVibration") == 1 && number(g, "vibType") == 2)
        .map(|g| {
            let byte = |k: &str| number(g, k).clamp(0, 255) as u8;
            let left = TriggerBind {
                block: byte("vibFilter"),
                scale: byte("pwmScal"),
                params: params(&text(g, "vibParams")),
            };
            // AdapterTriggerRunner.OnStartGameVibMode: the right side falls
            // back to the left one field by field
            let right_params = params(&text(g, "vibParamsRight"));
            let right = TriggerBind {
                block: if byte("vibFilterRight") > 0 {
                    byte("vibFilterRight")
                } else {
                    left.block
                },
                scale: if byte("pwmScalRight") > 0 {
                    byte("pwmScalRight")
                } else {
                    left.scale
                },
                params: if right_params.iter().any(|&b| b > 0) {
                    right_params
                } else {
                    left.params
                },
            };
            let en = text(g, "enGameName");
            GamePreset {
                id: number(g, "id"),
                name: if en.is_empty() {
                    text(g, "gameName")
                } else {
                    en
                },
                bind: [left, right],
            }
        })
        .collect();
    games.sort_by_key(|g| g.name.to_lowercase());
    games
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_takes_code_or_status() {
        assert_eq!(unwrap(r#"{"code":0,"data":[1]}"#).unwrap(), json!([1]));
        assert_eq!(
            unwrap(r#"{"status":0,"message":"OK","data":2}"#).unwrap(),
            json!(2)
        );
        assert!(unwrap(r#"{"code":599,"msg":"login"}"#).is_err());
    }

    #[test]
    fn firmware_lists_only_differing_chips() {
        // the server's answer for an Apex 5 with switch 4.5.2.8
        let data = json!({"device_code":"k5","chip_list":{
            "main_chip":null,"rf_chip":null,"screen_chip":null,
            "si_chip":{"version":"4.5.2.9","url":"https://x/y.ufw","info":"1. 支持","is_push":0}}});
        let sent = [
            ("main", "main_chip", "7.0.4.5".to_string()),
            ("switch", "si_chip", "4.5.2.8".to_string()),
            ("screen", "screen_chip", "0.1.2.8".to_string()),
        ];
        let news = firmware_news(&data, &sent);
        assert_eq!(news.len(), 1);
        assert_eq!(
            (news[0].chip, news[0].latest.as_str()),
            ("switch", "4.5.2.9")
        );
        assert_eq!(news[0].current, "4.5.2.8");
        assert!(!news[0].pushed);
    }

    #[test]
    fn game_presets_fall_back_to_the_left_side() {
        let data = json!([
            {"id":58,"enGameName":"Apex Legends™","isVibration":1,"vibType":2,
             "vibParams":"0,1,1,15,0","vibParamsRight":"0,0,0,0,0",
             "vibFilter":100,"vibFilterRight":0,"pwmScal":10,"pwmScalRight":0},
            {"id":7,"gameName":"辐射4","enGameName":"","isVibration":"1","vibType":2,
             "vibParams":"60,1,1,15,0","vibParamsRight":"60,1,1,15,0",
             "vibFilter":60,"vibFilterRight":60,"pwmScal":10,"pwmScalRight":10},
            {"id":1,"enGameName":"No preset","isVibration":0,"vibType":2}
        ]);
        let g = parse_games(&data);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].name, "Apex Legends™");
        let b = TriggerBind {
            block: 100,
            scale: 10,
            params: [0, 1, 1, 15, 0],
        };
        assert_eq!(g[0].bind, [b, b]);
        assert_eq!(g[1].name, "辐射4");
        assert_eq!(g[1].bind[1].params, [60, 1, 1, 15, 0]);
    }

    #[test]
    fn pictures_keep_https_links_for_this_model() {
        let data = json!([
            {"id":106,"title":"精选动画1","imagePath":"https://cdn/a.gif","device_code":"k5"},
            {"id":5,"title":"x","imagePath":"https://cdn/b.png","device_code":"f5"},
            {"id":6,"title":"y","imagePath":"http://cdn/c.png","device_code":"k5"}
        ]);
        let p = parse_pictures(&data);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].title, "Анимация 1");
        assert!(p[0].path().unwrap().ends_with("screen/106.gif"));
    }
}
