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

/// A picture as decoded, frames kept a little larger than the screen.
pub struct Source {
    pub frames: Vec<RgbaImage>,
    /// Average GIF frame delay; `None` for a still picture.
    pub interval_ms: Option<u32>,
    /// More frames than `MAX_FRAMES` were in the file.
    pub cut: bool,
}

/// Decodes a PNG, JPEG or GIF; a GIF gives its first `MAX_FRAMES` frames.
pub fn decode(path: &Path) -> Result<Source> {
    let reader = ImageReader::open(path)
        .with_context(|| format!("read {}", path.display()))?
        .with_guessed_format()?;
    if reader.format() != Some(ImageFormat::Gif) {
        let img = reader.decode().context("не удалось прочитать картинку")?;
        return Ok(Source {
            frames: vec![shrink(img.to_rgba8())],
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
        frames.push(shrink(f.into_buffer()));
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

/// Keeps at least twice the screen in both directions, so either fit stays
/// sharp, without holding full-size GIF frames in memory.
fn shrink(img: RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let scale = (2.0 * W as f32 / w as f32).max(2.0 * H as f32 / h as f32);
    if scale >= 1.0 {
        return img;
    }
    let (nw, nh) = (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    );
    imageops::resize(&img, nw, nh, FilterType::Triangle)
}

/// One screen frame from a picture: fitted to 160 × 80 on black, colours
/// rounded the way Space Station rounds them.
pub fn encode(img: &RgbaImage, fit: Fit) -> Vec<u8> {
    let (w, h) = img.dimensions();
    let mut canvas = RgbaImage::from_pixel(W, H, Rgba([0, 0, 0, 255]));
    match fit {
        Fit::Fill => {
            let (cw, ch) = if w * H > h * W {
                ((h * W / H).max(1), h)
            } else {
                (w, (w * H / W).max(1))
            };
            let crop = imageops::crop_imm(img, (w - cw) / 2, (h - ch) / 2, cw, ch).to_image();
            let scaled = imageops::resize(&crop, W, H, FilterType::CatmullRom);
            imageops::overlay(&mut canvas, &scaled, 0, 0);
        }
        Fit::Whole => {
            let scale = (W as f32 / w as f32).min(H as f32 / h as f32);
            let (sw, sh) = (
                ((w as f32 * scale).round() as u32).clamp(1, W),
                ((h as f32 * scale).round() as u32).clamp(1, H),
            );
            let scaled = imageops::resize(img, sw, sh, FilterType::CatmullRom);
            let (x, y) = ((W - sw) / 2, (H - sh) / 2);
            imageops::overlay(&mut canvas, &scaled, x as i64, y as i64);
        }
    }
    let mut out = Vec::with_capacity(FRAME);
    out.extend_from_slice(&HEADER);
    for p in canvas.pixels() {
        let [r, g, b, _] = p.0;
        let r5 = ((r as u32 + 4) / 8).min(31);
        let g6 = ((g as u32 + 2) / 4).min(63);
        let b5 = ((b as u32 + 4) / 8).min(31);
        let v = (r5 << 11) | (g6 << 5) | b5;
        out.extend_from_slice(&[(v >> 8) as u8, v as u8]);
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
        let ms = interval_ms.clamp(INTERVAL_MS.0, INTERVAL_MS.1);
        Ok(Upload {
            data: frames.concat(),
            frames: frames.len(),
            frame_rate: ((ms + 5) / 10).clamp(1, 255) as u8,
            restore_default: false,
        })
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
