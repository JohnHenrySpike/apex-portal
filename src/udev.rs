//! The udev rules this app needs, and installing them with the system's own
//! password prompt (pkexec): the app never sees the password.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Result, bail};

pub const RULES: &str = include_str!("../packaging/72-flydigi-apex5.rules");
pub const PATH: &str = "/etc/udev/rules.d/72-flydigi-apex5.rules";

/// Writes `RULES` to `PATH` as root, has the uhid module load at boot (and
/// now), then reloads udev and re-applies the rules to hidraw, serial and
/// misc devices.
pub fn install() -> Result<()> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let tmp = dir.join(format!("apex5-rules-{}", std::process::id()));
    std::fs::write(&tmp, RULES)?;
    let script = "install -Dm644 \"$1\" \"$2\" \
                  && echo uhid > /etc/modules-load.d/uhid.conf && (modprobe uhid || true) \
                  && udevadm control --reload \
                  && udevadm trigger --subsystem-match=hidraw --subsystem-match=tty \
                  --subsystem-match=misc";
    let status = Command::new("pkexec")
        .args(["/bin/sh", "-c", script, "sh"])
        .arg(&tmp)
        .arg(PATH)
        .status();
    let _ = std::fs::remove_file(&tmp);
    match status {
        Err(e) => bail!("не удалось запустить pkexec: {e}"),
        Ok(s) if s.success() => Ok(()),
        // pkexec: 126 = the prompt was dismissed, 127 = not authorised
        Ok(s) if matches!(s.code(), Some(126 | 127)) => bail!("пароль не введён"),
        Ok(s) => bail!(
            "установка правила не удалась (код {})",
            s.code().unwrap_or(-1)
        ),
    }
}

/// Whether this user can create the virtual DualSense.
pub fn uhid_ready() -> bool {
    let path = std::ffi::CString::new(crate::uhid::PATH).unwrap();
    // SAFETY: a valid C string.
    unsafe { libc::access(path.as_ptr(), libc::R_OK | libc::W_OK) == 0 }
}
