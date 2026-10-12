//! Own picture or animation on the 160 × 80 screen (docs/spec-device.md §4.4,
//! docs/space-station.md §9). Command 31 hands the screen chip to its
//! bootloader, which shows up as a serial port `ffaa:5555`; the frames go
//! through that port the way Flydigi's `OtaNewUpdater` sends them.
//!
//! Only the picture area is ever written: its address comes from the chip,
//! every erase and write stays inside `base .. base + size`, and only the
//! picture opcodes can be built here.

use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use image::imageops::{self, FilterType};
use image::{AnimationDecoder, ImageFormat, ImageReader, Rgba, RgbaImage};

use crate::hid::Device;

pub const W: u32 = 160;
pub const H: u32 = 80;
/// LVGL v8 true-colour frame: header, then RGB565 high byte first.
pub const FRAME: usize = 4 + (W * H * 2) as usize;
const HEADER: [u8; 4] = [0x04, 0x80, 0x02, 0x0A];
/// Space Station's limit for own pictures; the protocol allows 255.
pub const MAX_FRAMES: usize = 150;
/// One frame takes about this long to write [HW].
pub const SECONDS_PER_FRAME: u32 = 25;
pub const INTERVAL_MS: (u32, u32) = (10, 2550);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// Cover the screen, cutting off what sticks out.
    Fill,
    /// Show the whole picture, with black bars.
    Whole,
}

impl Fit {
    /// The part of a `w × h` picture this fit puts on the screen.
    pub fn crop(self, w: u32, h: u32) -> Crop {
        match self {
            Fit::Fill => Crop::fill(w, h),
            Fit::Whole => Crop::whole(w, h),
        }
    }
}

/// The part of a picture that goes on the screen, in fractions of the
/// picture's width and height, so it fits every frame of a GIF and survives
/// `decode`'s shrinking. In pixels it is 2 : 1 like the screen. It may reach
/// past the picture: the screen is black there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Crop {
    /// `w` wide, centred on (cx, cy), the height following from 2 : 1.
    fn sized(w: f32, (cx, cy): (f32, f32), src_w: u32, src_h: u32) -> Crop {
        let h = w * src_w as f32 / (2.0 * src_h.max(1) as f32);
        Crop {
            x: cx - w / 2.0,
            y: cy - h / 2.0,
            w,
            h,
        }
    }

    /// The largest part inside the picture, centred.
    pub fn fill(src_w: u32, src_h: u32) -> Crop {
        let w = (2.0 * src_h as f32 / src_w.max(1) as f32).min(1.0);
        Crop::sized(w, (0.5, 0.5), src_w, src_h)
    }

    /// The smallest part holding the whole picture, centred.
    pub fn whole(src_w: u32, src_h: u32) -> Crop {
        let w = (2.0 * src_h as f32 / src_w.max(1) as f32).max(1.0);
        Crop::sized(w, (0.5, 0.5), src_w, src_h)
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// The same size over the middle of the picture.
    pub fn centered(self) -> Crop {
        Crop {
            x: (1.0 - self.w) / 2.0,
            y: (1.0 - self.h) / 2.0,
            ..self
        }
    }

    /// Widths the editor allows: at least 8 picture pixels across (or the
    /// whole of a tinier picture), at most four times `whole`.
    pub fn width_range(src_w: u32, src_h: u32) -> (f32, f32) {
        let whole = Crop::whole(src_w, src_h).w;
        ((8.0 / src_w.max(1) as f32).min(whole), 4.0 * whole)
    }

    /// Brought within the editor's limits: the width in `width_range`, the
    /// height matching it, the centre inside the picture.
    pub fn limited(self, src_w: u32, src_h: u32) -> Crop {
        let (lo, hi) = Crop::width_range(src_w, src_h);
        let (cx, cy) = self.center();
        Crop::sized(
            self.w.clamp(lo, hi),
            (cx.clamp(0.0, 1.0), cy.clamp(0.0, 1.0)),
            src_w,
            src_h,
        )
    }

    /// `f` times wider and taller, the point (px, py) staying where it is;
    /// `f > 1` shows more of the picture.
    pub fn zoomed(self, f: f32, (px, py): (f32, f32), src_w: u32, src_h: u32) -> Crop {
        let (lo, hi) = Crop::width_range(src_w, src_h);
        let w = (self.w * f).clamp(lo, hi);
        let f = w / self.w;
        Crop {
            x: px - (px - self.x) * f,
            y: py - (py - self.y) * f,
            w,
            h: self.h * f,
        }
        .limited(src_w, src_h)
    }
}

/// A picture as decoded, frames kept larger than the screen.
pub struct Source {
    pub frames: Vec<RgbaImage>,
    /// Average GIF frame delay; `None` for a still picture.
    pub interval_ms: Option<u32>,
    /// More frames than `MAX_FRAMES` were in the file.
    pub cut: bool,
}

/// How many times the screen a still picture keeps, so the editor can zoom
/// in without blur; GIF frames keep less, there may be 150 of them.
const STILL_KEEP: f32 = 6.0;
const GIF_KEEP: f32 = 2.0;

/// Decodes a PNG, JPEG or GIF; a GIF gives its first `MAX_FRAMES` frames.
pub fn decode(path: &Path) -> Result<Source> {
    let reader = ImageReader::open(path)
        .with_context(|| format!("read {}", path.display()))?
        .with_guessed_format()?;
    if reader.format() != Some(ImageFormat::Gif) {
        let img = reader.decode().context("не удалось прочитать картинку")?;
        return Ok(Source {
            frames: vec![shrink(img.to_rgba8(), STILL_KEEP)],
            interval_ms: None,
            cut: false,
        });
    }
    let file = BufReader::new(File::open(path)?);
    let gif = image::codecs::gif::GifDecoder::new(file).context("не удалось прочитать GIF")?;
    let mut frames = Vec::new();
    let mut total_ms = 0;
    let mut cut = false;
    for f in gif.into_frames() {
        if frames.len() == MAX_FRAMES {
            cut = true;
            break;
        }
        let f = f.context("не удалось прочитать кадр GIF")?;
        let (n, d) = f.delay().numer_denom_ms();
        total_ms += n / d.max(1);
        frames.push(shrink(f.into_buffer(), GIF_KEEP));
    }
    if frames.is_empty() {
        bail!("в GIF нет кадров");
    }
    let avg = total_ms / frames.len() as u32;
    // browsers play a zero delay at 100 ms
    let interval = if avg == 0 { 100 } else { avg };
    Ok(Source {
        interval_ms: (frames.len() > 1).then(|| interval.clamp(INTERVAL_MS.0, INTERVAL_MS.1)),
        frames,
        cut,
    })
}

/// Keeps at least `keep` times the screen in both directions, so the
/// filled screen stays sharp, without holding full-size frames in memory.
fn shrink(img: RgbaImage, keep: f32) -> RgbaImage {
    let (w, h) = img.dimensions();
    let scale = (keep * W as f32 / w as f32).max(keep * H as f32 / h as f32);
    if scale >= 1.0 {
        return img;
    }
    let (nw, nh) = (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    );
    imageops::resize(&img, nw, nh, FilterType::Triangle)
}

/// For each of `out` screen pixels along one axis, the picture pixels it
/// reads and their weights. The screen spans `len` picture pixels from
/// `start`; the picture is `n` pixels long and black beyond.
fn taps(start: f32, len: f32, n: u32, out: u32) -> Vec<Vec<(u32, f32)>> {
    let step = len / out as f32;
    (0..out)
        .map(|i| {
            let a = start + i as f32 * step;
            let b = a + step;
            if step >= 1.0 {
                // shrinking: the average of what the screen pixel covers
                let lo = a.floor().max(0.0) as i64;
                let hi = (b.ceil() as i64).min(n as i64);
                (lo..hi)
                    .filter_map(|p| {
                        let cover = (b.min(p as f32 + 1.0) - a.max(p as f32)) / step;
                        (cover > 0.0).then_some((p as u32, cover))
                    })
                    .collect()
            } else {
                // enlarging: linear between the nearest pixel centres, the
                // edge pixels stretched out to the picture's border
                let c = (a + b) / 2.0;
                if c < 0.0 || c >= n as f32 {
                    return vec![];
                }
                let t = c - 0.5;
                let p0 = t.floor();
                let f = t - p0;
                let at = |p: f32| (p.max(0.0) as u32).min(n - 1);
                vec![(at(p0), 1.0 - f), (at(p0 + 1.0), f)]
            }
        })
        .collect()
}

/// One screen frame from a picture: fitted to 160 × 80 on black, colours
/// rounded the way Space Station rounds them.
pub fn encode(img: &RgbaImage, fit: Fit) -> Vec<u8> {
    let (w, h) = img.dimensions();
    encode_crop(img, fit.crop(w, h))
}

/// One screen frame from the `crop` part of a picture, black outside it.
pub fn encode_crop(img: &RgbaImage, crop: Crop) -> Vec<u8> {
    let (w, h) = img.dimensions();
    let cols = taps(crop.x * w as f32, crop.w * w as f32, w, W);
    let rows = taps(crop.y * h as f32, crop.h * h as f32, h, H);
    // the picture rows some screen row reads, squeezed to the screen's width
    // first; colours premultiplied, which puts them over black
    let (y0, y1) = rows
        .iter()
        .flatten()
        .fold((u32::MAX, 0), |(a, b), &(y, _)| (a.min(y), b.max(y + 1)));
    let mut mid = vec![[0f32; 3]; y1.saturating_sub(y0) as usize * W as usize];
    for y in y0..y1 {
        let line = &mut mid[(y - y0) as usize * W as usize..][..W as usize];
        for (acc, taps) in line.iter_mut().zip(&cols) {
            for &(x, wt) in taps {
                let [r, g, b, a] = img.get_pixel(x, y).0;
                let k = wt * a as f32 / 255.0;
                acc[0] += r as f32 * k;
                acc[1] += g as f32 * k;
                acc[2] += b as f32 * k;
            }
        }
    }
    let mut out = Vec::with_capacity(FRAME);
    out.extend_from_slice(&HEADER);
    for taps in &rows {
        for ox in 0..W as usize {
            let mut c = [0f32; 3];
            for &(y, wt) in taps {
                let m = mid[(y - y0) as usize * W as usize + ox];
                for k in 0..3 {
                    c[k] += m[k] * wt;
                }
            }
            let [r, g, b] = c.map(|v| v.round().clamp(0.0, 255.0) as u32);
            let r5 = ((r + 4) / 8).min(31);
            let g6 = ((g + 2) / 4).min(63);
            let b5 = ((b + 4) / 8).min(31);
            let v = (r5 << 11) | (g6 << 5) | b5;
            out.extend_from_slice(&[(v >> 8) as u8, v as u8]);
        }
    }
    out
}

/// A screen frame back as RGBA, for the preview.
pub fn to_rgba(frame: &[u8]) -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| {
        let i = 4 + 2 * (y * W + x) as usize;
        let v = ((frame[i] as u32) << 8) | frame[i + 1] as u32;
        let (r, g, b) = ((v >> 11) & 31, (v >> 5) & 63, v & 31);
        Rgba([(r << 3) as u8, (g << 2) as u8, (b << 3) as u8, 255])
    })
}

/// What goes to the chip in one session.
#[derive(Clone)]
pub struct Upload {
    pub data: Vec<u8>,
    pub frames: usize,
    /// Hundredths of a second per frame, 1..255.
    pub frame_rate: u8,
    /// Flydigi's factory file, sent the way "Restore default" sends it.
    pub restore_default: bool,
}

impl Upload {
    pub fn new(frames: &[Vec<u8>], interval_ms: u32) -> Result<Self> {
        if frames.is_empty() || frames.len() > MAX_FRAMES {
            bail!("кадров должно быть от 1 до {MAX_FRAMES}");
        }
        if frames.iter().any(|f| f.len() != FRAME || f[..4] != HEADER) {
            bail!("кадр не в формате экрана");
        }
        let mut up = Upload {
            data: frames.concat(),
            frames: frames.len(),
            frame_rate: 0,
            restore_default: false,
        };
        up.set_interval(interval_ms);
        Ok(up)
    }

    /// Changes an own animation's speed without encoding it again.
    pub fn set_interval(&mut self, interval_ms: u32) {
        let ms = interval_ms.clamp(INTERVAL_MS.0, INTERVAL_MS.1);
        self.frame_rate = ((ms + 5) / 10).clamp(1, 255) as u8;
    }

    /// `default_screen_image_<deviceType>.bin` from Space Station.
    pub fn factory(data: Vec<u8>) -> Result<Self> {
        if data.is_empty() || !data.len().is_multiple_of(FRAME) {
            bail!("размер файла не кратен кадру экрана ({FRAME} байт)");
        }
        let frames = data.len() / FRAME;
        if frames > 255 {
            bail!("в файле {frames} кадров, больше 255");
        }
        if data.chunks(FRAME).any(|f| f[..4] != HEADER) {
            bail!("файл не похож на анимацию экрана Flydigi");
        }
        Ok(Upload {
            data,
            frames,
            frame_rate: 0,
            restore_default: true,
        })
    }

    /// 1 = animation, 2 = picture.
    fn pic_type(&self) -> u8 {
        if self.frames == 1 && !self.restore_default {
            2
        } else {
            1
        }
    }

    pub fn frame(&self, i: usize) -> &[u8] {
        &self.data[i * FRAME..(i + 1) * FRAME]
    }
}

/// `AppOtasCrcCal`: the reflected CRC-32 table driven MSB-first, seed 0, no
/// final xor, in signed 32-bit arithmetic.
pub fn crc(data: &[u8]) -> u32 {
    let table: Vec<u32> = (0..256u32)
        .map(|n| {
            (0..8).fold(n, |c, _| {
                if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                }
            })
        })
        .collect();
    let mut crc: i32 = 0;
    for &b in data {
        let q = crc / 256;
        crc = crc.wrapping_shl(8) ^ table[((q ^ b as i32) & 0xFF) as usize] as i32;
    }
    crc as u32
}

// ------------------------------------------------------------- the session

#[derive(Clone, Debug)]
pub enum Stage {
    /// Command 31 sent, waiting for the serial port.
    Switching,
    /// Exchanges done of all.
    Writing(usize, usize),
    /// The chip took everything; it saves and the pad restarts.
    Saving,
}

const VID: &str = "ffaa";
const PID: &str = "5555";
const REPLY_TIMEOUT: Duration = Duration::from_secs(18);
const PORT_WAIT: Duration = Duration::from_secs(30);
/// Below this the chip keeps its program (Flydigi's K5 split offset).
const PROGRAM_END: u32 = 0x000A_0000;

/// The screen chip's bootloader port, when it is up.
pub fn find_port() -> Option<PathBuf> {
    let mut nodes: Vec<_> = std::fs::read_dir("/sys/class/tty")
        .ok()?
        .flatten()
        .collect();
    nodes.sort_by_key(|e| e.file_name());
    nodes.into_iter().find_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with("ttyACM") && !name.starts_with("ttyUSB") {
            return None;
        }
        // ttyACM: device = the interface, its parent = the USB device
        let usb = e.path().join("device").canonicalize().ok()?;
        let usb = usb.ancestors().find(|p| p.join("idVendor").exists())?;
        let id = |f: &str| std::fs::read_to_string(usb.join(f)).unwrap_or_default();
        (id("idVendor").trim() == VID && id("idProduct").trim() == PID)
            .then(|| PathBuf::from("/dev").join(name))
    })
}

/// Whether some udev rule mentions the bootloader port, so it will be ours.
pub fn rule_installed() -> bool {
    [
        "/etc/udev/rules.d",
        "/run/udev/rules.d",
        "/usr/lib/udev/rules.d",
        "/lib/udev/rules.d",
    ]
    .iter()
    .filter_map(|d| std::fs::read_dir(d).ok())
    .flatten()
    .flatten()
    .filter_map(|e| std::fs::read_to_string(e.path()).ok())
    .any(|s| {
        s.lines().any(|l| {
            let l = l.to_ascii_lowercase();
            !l.trim_start().starts_with('#') && l.contains(VID) && l.contains(PID)
        })
    })
}

pub const RULE_HINT: &str = "Нет правила udev для порта экрана ffaa:5555: без него порт \
    доступен только root. Установите его кнопкой «Установить правило» или по README";

/// Puts the screen chip into its bootloader unless it already is, then
/// sends `up`. Takes the pad's HID handle and drops it: the pad restarts at
/// the end. Never retries: every attempt erases the picture area.
pub fn upload(dev: Option<Device>, up: &Upload, progress: &mut dyn FnMut(Stage)) -> Result<()> {
    let port = match find_port() {
        Some(p) => {
            drop(dev);
            p
        }
        None => {
            if !rule_installed() {
                bail!(RULE_HINT);
            }
            let mut dev = dev.ok_or_else(|| anyhow!("геймпад не подключён"))?;
            crate::device::enter_screen_upload(&mut dev)?;
            drop(dev);
            progress(Stage::Switching);
            std::thread::sleep(Duration::from_secs(5));
            let until = Instant::now() + PORT_WAIT;
            loop {
                if let Some(p) = find_port() {
                    break p;
                }
                if Instant::now() > until {
                    bail!(
                        "порт экрана не появился. Выключите и включите геймпад \
                         переключателем на задней стороне"
                    );
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    };
    // udev applies the rule a moment after the node appears
    let mut port_file = None;
    for _ in 0..10 {
        match Port::open(&port) {
            Ok(p) => {
                port_file = Some(p);
                break;
            }
            Err(e) if e.to_string().contains("Permission denied") => {
                std::thread::sleep(Duration::from_millis(300))
            }
            Err(e) => return Err(e),
        }
    }
    let mut s = Session {
        port: port_file.ok_or_else(|| anyhow!("нет доступа к {}. {RULE_HINT}", port.display()))?,
        base: 0,
        size: up.data.len() as u32,
    };
    s.run(up, progress)
}

struct Session {
    port: Port,
    base: u32,
    size: u32,
}

/// Requests the chip may get: the picture opcodes and nothing else.
enum Req<'a> {
    BaseAddr {
        pic_type: u8,
        count: u8,
        frame_rate: u8,
        restore: bool,
    },
    Version,
    Erase(u32),
    Write(u32, &'a [u8]),
    Finish {
        size: u32,
        crc: u32,
    },
}

impl Req<'_> {
    fn bytes(&self) -> Vec<u8> {
        match *self {
            Req::BaseAddr {
                pic_type,
                count,
                frame_rate,
                restore,
            } => {
                vec![11, 4, 0, pic_type, count, frame_rate, restore as u8]
            }
            Req::Version => vec![10, 6, 0],
            Req::Erase(a) => [&[3, 6, 0][..], &a.to_le_bytes()].concat(),
            // the length fields stay 64 and 55 for a short last packet too
            Req::Write(a, d) => [&[5, 0x40, 0][..], &a.to_le_bytes(), &[55, 0], d].concat(),
            Req::Finish { size, crc } => {
                [&[12, 8, 0][..], &size.to_le_bytes(), &crc.to_le_bytes()].concat()
            }
        }
    }
}

impl Session {
    fn run(&mut self, up: &Upload, progress: &mut dyn FnMut(Stage)) -> Result<()> {
        let erases = up.data.len().div_ceil(4096);
        let writes = up.data.len().div_ceil(55);
        let total = 2 + erases + writes + 1;
        let mut done = 0;
        let mut last = usize::MAX;
        let mut tick = |done: usize| {
            let permille = done * 1000 / total;
            if permille != last {
                last = permille;
                progress(Stage::Writing(done, total));
            }
        };
        tick(0);
        let r = self.xfer(&Req::BaseAddr {
            pic_type: up.pic_type(),
            count: up.frames as u8,
            frame_rate: up.frame_rate,
            restore: up.restore_default,
        })?;
        if r.len() < 8 {
            bail!("экран не сообщил адрес картинки");
        }
        let base = u32::from_le_bytes([r[4], r[5], r[6], r[7]]);
        if base < PROGRAM_END || !base.is_multiple_of(4096) || base.checked_add(self.size).is_none()
        {
            bail!("экран сообщил странный адрес картинки {base:#x}, загрузка остановлена");
        }
        self.base = base;
        // Flydigi waits after the address too
        std::thread::sleep(Duration::from_millis(200));
        done += 1;
        tick(done);
        self.xfer(&Req::Version)?;
        done += 1;
        tick(done);
        for i in 0..erases {
            self.xfer(&Req::Erase(base + (i * 4096) as u32))?;
            done += 1;
            tick(done);
        }
        for (j, chunk) in up.data.chunks(55).enumerate() {
            self.xfer(&Req::Write(base + (j * 55) as u32, chunk))?;
            done += 1;
            tick(done);
        }
        self.xfer(&Req::Finish {
            size: self.size,
            crc: crc(&up.data),
        })?;
        progress(Stage::Saving);
        Ok(())
    }

    fn xfer(&mut self, req: &Req) -> Result<Vec<u8>> {
        // the last guard: nothing outside the picture area, ever
        let span = self.base as u64..self.base as u64 + self.size.next_multiple_of(4096) as u64;
        match *req {
            Req::Erase(a) | Req::Write(a, _) if !span.contains(&(a as u64)) => {
                bail!("адрес {a:#x} вне области картинки")
            }
            Req::Write(_, d) if d.is_empty() || d.len() > 55 => bail!("пакет данных не по размеру"),
            _ => {}
        }
        let bytes = req.bytes();
        let op = bytes[0];
        let r = self.port.exchange(&bytes, REPLY_TIMEOUT)?;
        if r.len() < 2 || r[1] != op {
            bail!("экран ответил не на тот запрос ({op}): {r:02x?}");
        }
        Ok(r)
    }
}

/// The bootloader's serial port: 921600 8N1, no flow control, DTR and RTS up.
struct Port(File);

impl Port {
    fn open(path: &Path) -> Result<Self> {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
            .open(path)
            .map_err(|e| anyhow!("{}: {e}", path.display()))?;
        let fd = f.as_raw_fd();
        let os = |r: i32, what: &str| {
            if r < 0 {
                Err(anyhow!("{what}: {}", std::io::Error::last_os_error()))
            } else {
                Ok(())
            }
        };
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            os(libc::tcgetattr(fd, &mut t), "tcgetattr")?;
            libc::cfmakeraw(&mut t);
            t.c_cflag |= libc::CLOCAL | libc::CREAD;
            t.c_cflag &= !(libc::CRTSCTS | libc::CSTOPB | libc::PARENB);
            t.c_iflag &= !(libc::IXON | libc::IXOFF | libc::IXANY);
            t.c_cc[libc::VMIN] = 0;
            t.c_cc[libc::VTIME] = 0;
            os(libc::cfsetispeed(&mut t, libc::B921600), "speed")?;
            os(libc::cfsetospeed(&mut t, libc::B921600), "speed")?;
            os(libc::tcsetattr(fd, libc::TCSANOW, &t), "tcsetattr")?;
            let bits: libc::c_int = libc::TIOCM_DTR | libc::TIOCM_RTS;
            os(libc::ioctl(fd, libc::TIOCMBIS, &bits), "DTR/RTS")?;
            os(libc::tcflush(fd, libc::TCIOFLUSH), "flush")?;
            // blocking writes from here; reads wait in poll
            let fl = libc::fcntl(fd, libc::F_GETFL);
            os(
                libc::fcntl(fd, libc::F_SETFL, fl & !libc::O_NONBLOCK),
                "fcntl",
            )?;
        }
        Ok(Port(f))
    }

    fn wait(&self, timeout: Duration) -> bool {
        let mut pfd = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        unsafe {
            libc::poll(
                &mut pfd,
                1,
                timeout.as_millis().min(i32::MAX as u128) as i32,
            ) > 0
        }
    }

    /// Sends one request and collects its reply: whatever arrives until the
    /// line stays quiet for a moment.
    fn exchange(&mut self, req: &[u8], timeout: Duration) -> Result<Vec<u8>> {
        self.0
            .write_all(req)
            .map_err(|e| anyhow!("порт экрана: {e}"))?;
        let mut out = Vec::new();
        let mut buf = [0u8; 256];
        let deadline = Instant::now() + timeout;
        loop {
            let left = if out.is_empty() {
                deadline.saturating_duration_since(Instant::now())
            } else {
                Duration::from_millis(20)
            };
            if left.is_zero() || !self.wait(left) {
                break;
            }
            match self.0.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => bail!("порт экрана: {e}"),
            }
        }
        if out.is_empty() {
            bail!(
                "экран не ответил за {} с, загрузка остановлена",
                timeout.as_secs()
            );
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_lvgl_rgb565_high_byte_first() {
        let red = RgbaImage::from_pixel(320, 160, Rgba([255, 0, 0, 255]));
        let f = encode(&red, Fit::Fill);
        assert_eq!(f.len(), FRAME);
        assert_eq!(f[..4], HEADER);
        assert_eq!(f[4..6], [0xF8, 0x00]);
        let blue = encode(
            &RgbaImage::from_pixel(10, 10, Rgba([0, 0, 255, 255])),
            Fit::Fill,
        );
        assert_eq!(blue[4..6], [0x00, 0x1F]);
        // rounding, not truncation: 6 rounds up to 8, 1 stays 0
        let px = encode(
            &RgbaImage::from_pixel(4, 2, Rgba([6, 2, 1, 255])),
            Fit::Fill,
        );
        assert_eq!(px[4..6], [0x08, 0x20]);
        // transparent is black
        let clear = encode(
            &RgbaImage::from_pixel(4, 2, Rgba([255, 255, 255, 0])),
            Fit::Fill,
        );
        assert_eq!(clear[4..6], [0, 0]);
        assert_eq!(to_rgba(&f).get_pixel(5, 5).0, [248, 0, 0, 255]);
    }

    #[test]
    fn whole_fit_leaves_black_bars() {
        // a square picture: bars left and right, picture in the middle
        let white = RgbaImage::from_pixel(100, 100, Rgba([255; 4]));
        let f = to_rgba(&encode(&white, Fit::Whole));
        assert_eq!(f.get_pixel(0, 40).0, [0, 0, 0, 255]);
        assert_eq!(f.get_pixel(80, 40).0, [248, 252, 248, 255]);
        let f = to_rgba(&encode(&white, Fit::Fill));
        assert_eq!(f.get_pixel(0, 40).0, [248, 252, 248, 255]);
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    /// 2 : 1 in pixels, like the screen.
    fn screen_shaped(c: Crop, w: u32, h: u32) -> bool {
        close(c.w * w as f32, 2.0 * c.h * h as f32)
    }

    #[test]
    fn fill_and_whole_crops() {
        for (w, h) in [(100, 100), (1000, 100), (90, 160), (320, 160), (1, 1)] {
            let (fill, whole) = (Crop::fill(w, h), Crop::whole(w, h));
            assert!(screen_shaped(fill, w, h) && screen_shaped(whole, w, h));
            // fill: inside, touching two opposite edges
            assert!(fill.x >= -1e-6 && fill.y >= -1e-6);
            assert!(fill.x + fill.w <= 1.0 + 1e-6 && fill.y + fill.h <= 1.0 + 1e-6);
            assert!(close(fill.w, 1.0) || close(fill.h, 1.0));
            // whole: around the picture, touching two opposite edges
            assert!(whole.x <= 1e-6 && whole.y <= 1e-6);
            assert!(whole.x + whole.w >= 1.0 - 1e-6 && whole.y + whole.h >= 1.0 - 1e-6);
            assert!(close(whole.w, 1.0) || close(whole.h, 1.0));
            for c in [fill, whole] {
                assert!(close(c.center().0, 0.5) && close(c.center().1, 0.5));
            }
        }
        // a square: fill is the middle half, whole twice as wide
        let sq = Crop::fill(100, 100);
        assert!(close(sq.y, 0.25) && close(sq.h, 0.5) && close(sq.w, 1.0));
        let sq = Crop::whole(100, 100);
        assert!(close(sq.x, -0.5) && close(sq.w, 2.0) && close(sq.h, 1.0));
    }

    #[test]
    fn crop_picks_the_chosen_part() {
        // four colours in quarters of a 2 : 1 picture
        let quad = RgbaImage::from_fn(320, 160, |x, y| match (x < 160, y < 80) {
            (true, true) => Rgba([255, 0, 0, 255]),
            (false, true) => Rgba([0, 255, 0, 255]),
            (true, false) => Rgba([0, 0, 255, 255]),
            (false, false) => Rgba([255, 255, 255, 255]),
        });
        let px = |c: Crop, x, y| to_rgba(&encode_crop(&quad, c)).get_pixel(x, y).0;
        let green = Crop {
            x: 0.5,
            y: 0.0,
            w: 0.5,
            h: 0.5,
        };
        assert_eq!(px(green, 0, 0), [0, 252, 0, 255]);
        assert_eq!(px(green, 159, 79), [0, 252, 0, 255]);
        // half off the right edge: picture on the left, black on the right
        let off = Crop {
            x: 0.75,
            y: 0.5,
            w: 0.5,
            h: 0.5,
        };
        assert_eq!(px(off, 10, 40), [248, 252, 248, 255]);
        assert_eq!(px(off, 150, 40), [0, 0, 0, 255]);
        // entirely outside: all black
        let gone = Crop { x: 3.0, ..off };
        assert!(encode_crop(&quad, gone)[4..].iter().all(|&b| b == 0));
        // a crop of fill's shape gives the same frame as Fit::Fill
        let photo = RgbaImage::from_fn(300, 200, |x, y| Rgba([x as u8, y as u8, 7, 255]));
        assert_eq!(
            encode_crop(&photo, Crop::fill(300, 200)),
            encode(&photo, Fit::Fill)
        );
        // zoomed in past the pixels: the edge pixels reach the border, no
        // black seam
        let tiny = RgbaImage::from_pixel(4, 2, Rgba([255, 0, 0, 255]));
        let f = to_rgba(&encode_crop(&tiny, Crop::fill(4, 2)));
        assert_eq!(f.get_pixel(0, 0).0, [248, 0, 0, 255]);
        assert_eq!(f.get_pixel(159, 79).0, [248, 0, 0, 255]);
    }

    #[test]
    fn crop_limits_and_zoom() {
        let (w, h) = (400, 300);
        let (lo, hi) = Crop::width_range(w, h);
        assert!(close(lo, 8.0 / 400.0));
        assert!(close(hi, 4.0 * Crop::whole(w, h).w));
        let fill = Crop::fill(w, h);
        // zooming in about a point keeps it in place, relative to the frame
        let p = (0.3, 0.4);
        let z = fill.zoomed(0.5, p, w, h);
        assert!(close(z.w, fill.w * 0.5) && screen_shaped(z, w, h));
        assert!(close((p.0 - z.x) / z.w, (p.0 - fill.x) / fill.w));
        assert!(close((p.1 - z.y) / z.h, (p.1 - fill.y) / fill.h));
        // and stops at the limits
        assert!(close(fill.zoomed(1e-6, p, w, h).w, lo));
        assert!(close(fill.zoomed(1e6, p, w, h).w, hi));
        // dragged far away, the centre comes back onto the picture
        let away = Crop {
            x: 5.0,
            y: -9.0,
            ..fill
        }
        .limited(w, h);
        assert!(close(away.center().0, 1.0) && close(away.center().1, 0.0));
        assert!(screen_shaped(away, w, h));
        let mid = Crop { x: 0.9, ..fill }.centered();
        assert!(close(mid.center().0, 0.5) && close(mid.w, fill.w));
    }

    #[test]
    fn crc_matches_flydigi_arithmetic() {
        // from a separate transcription of `Crc32CalByByte` with C# int
        // semantics; the long input goes negative, where the division
        // must truncate
        assert_eq!(crc(&[]), 0);
        assert_eq!(crc(&[1]), 0x7707_3096);
        assert_eq!(crc(b"123456789"), 0x6a85_17bd);
        let long: Vec<u8> = (0..3).flat_map(|_| 0..=255u8).collect();
        assert_eq!(crc(&long), 0xc9bc_3de4);
    }

    #[test]
    fn uploads_describe_themselves_like_space_station() {
        let f = encode(&RgbaImage::new(2, 1), Fit::Fill);
        let one = Upload::new(std::slice::from_ref(&f), 1000).unwrap();
        assert_eq!((one.pic_type(), one.frame_rate), (2, 100));
        let two = Upload::new(&[f.clone(), f.clone()], 33).unwrap();
        assert_eq!(
            (two.pic_type(), two.frame_rate, two.data.len()),
            (1, 3, 2 * FRAME)
        );
        assert!(Upload::new(&vec![f.clone(); MAX_FRAMES + 1], 100).is_err());
        let fac = Upload::factory([f.clone(), f].concat()).unwrap();
        assert_eq!(
            (fac.pic_type(), fac.frame_rate, fac.restore_default),
            (1, 0, true)
        );
        assert!(Upload::factory(vec![0; FRAME]).is_err());
    }

    #[test]
    fn requests_copy_flydigi_lengths() {
        assert_eq!(Req::Version.bytes(), [10, 6, 0]);
        assert_eq!(
            Req::BaseAddr {
                pic_type: 1,
                count: 14,
                frame_rate: 10,
                restore: false
            }
            .bytes(),
            [11, 4, 0, 1, 14, 10, 0]
        );
        assert_eq!(Req::Erase(0x2FF000).bytes(), [3, 6, 0, 0x00, 0xF0, 0x2F, 0]);
        let w = Req::Write(0x2FF037, &[9; 3]).bytes();
        assert_eq!(w, [5, 0x40, 0, 0x37, 0xF0, 0x2F, 0, 55, 0, 9, 9, 9]);
        assert_eq!(
            Req::Finish {
                size: 25604,
                crc: 0x01020304
            }
            .bytes(),
            [12, 8, 0, 0x04, 0x64, 0, 0, 4, 3, 2, 1]
        );
    }
}
