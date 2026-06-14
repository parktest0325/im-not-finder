//! ADB transport: delegates to the system `adb` binary.

use super::*;
use portable_pty::CommandBuilder;
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::process::Command;

fn adb() -> Command {
    let mut cmd = Command::new("adb");
    // On Windows, stop a console window from flashing on every adb invocation.
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AdbDevice {
    pub serial: String,
    pub state: String,
    pub model: String,
}

pub async fn list_devices() -> Result<Vec<AdbDevice>> {
    let out = adb().args(["devices", "-l"]).output().await?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut devices = Vec::new();
    for line in text.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut it = line.split_whitespace();
        let serial = match it.next() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let state = it.next().unwrap_or("unknown").to_string();
        let mut model = String::new();
        for tok in it {
            if let Some(m) = tok.strip_prefix("model:") {
                model = m.replace('_', " ");
            }
        }
        devices.push(AdbDevice {
            serial,
            state,
            model,
        });
    }
    Ok(devices)
}

pub struct AdbTransport {
    serial: String,
    model: String,
    elevated: AtomicBool,
}

impl AdbTransport {
    /// Wrap a device command in `su -c '...'` when elevation is on.
    fn wrap(&self, inner: &str) -> String {
        if self.elevated.load(Ordering::Relaxed) {
            format!("su -c {}", shq(inner))
        } else {
            inner.to_string()
        }
    }

    /// Run a device shell command (elevation-aware), failing on a non-zero exit.
    async fn shell_check(&self, inner: &str) -> Result<()> {
        let out = adb()
            .args(["-s", &self.serial, "shell", &self.wrap(inner)])
            .output()
            .await?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let err = err.trim();
            anyhow::bail!(
                "{}",
                if err.is_empty() {
                    String::from_utf8_lossy(&out.stdout).trim().to_string()
                } else {
                    err.to_string()
                }
            );
        }
        Ok(())
    }

    pub async fn connect(serial: &str) -> Result<Self> {
        // Verify the device is reachable.
        let out = adb()
            .args(["-s", serial, "shell", "echo", "ok"])
            .output()
            .await?;
        if !out.status.success() {
            anyhow::bail!(
                "adb device {serial} not ready: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let model_out = adb()
            .args(["-s", serial, "shell", "getprop", "ro.product.model"])
            .output()
            .await
            .ok();
        let model = model_out
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| serial.to_string());
        Ok(Self {
            serial: serial.to_string(),
            model,
            elevated: AtomicBool::new(false),
        })
    }
}

#[async_trait]
impl Transport for AdbTransport {
    fn label(&self) -> String {
        format!("{} (adb)", self.model)
    }
    fn home(&self) -> String {
        "/sdcard".to_string()
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>> {
        let cmd = self.wrap(&format!("ls -la {}", shq(path)));
        let out = adb()
            .args(["-s", &self.serial, "shell", &cmd])
            .output()
            .await?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut entries = Vec::new();
        for line in text.lines() {
            let line = line.trim_end();
            if line.is_empty() || line.starts_with("total ") {
                continue;
            }
            if let Some(e) = parse_ls_line(line, path) {
                entries.push(e);
            }
        }
        if entries.is_empty() && !out.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(entries)
    }

    async fn read_chunk(&self, path: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        // Prefer a block-aligned read (bs=len): one fast on-device read instead of
        // `len` single-byte reads. Our viewers always page on `len` boundaries.
        let dd = if len > 0 && offset % len == 0 {
            format!(
                "dd if={} bs={} skip={} count=1 2>/dev/null",
                shq(path),
                len,
                offset / len
            )
        } else {
            format!(
                "dd if={} bs=1 skip={} count={} 2>/dev/null",
                shq(path),
                offset,
                len
            )
        };
        let out = adb()
            .args(["-s", &self.serial, "exec-out", &self.wrap(&dd)])
            .output()
            .await?;
        Ok(out.stdout)
    }

    async fn remove(&self, path: &str) -> Result<()> {
        self.shell_check(&format!("rm -rf {}", shq(path))).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        self.shell_check(&format!("mv -f {} {}", shq(from), shq(to)))
            .await
    }

    async fn copy(&self, from: &str, to_dir: &str) -> Result<()> {
        self.shell_check(&format!("cp -r {} {}", shq(from), shq(to_dir)))
            .await
    }

    async fn download(&self, remote: &str, local: &Path) -> Result<()> {
        if self.elevated.load(Ordering::Relaxed) {
            // `adb pull` runs as the shell user, so root-owned files need staging:
            // copy (as root) into a shell-readable /sdcard dir, pull, then clean up.
            let base = remote
                .rsplit('/')
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or("file");
            let stage = format!("/sdcard/.imnf_dl/{base}");
            let prep = format!(
                "rm -rf /sdcard/.imnf_dl && mkdir -p /sdcard/.imnf_dl && cp -r {} {}",
                shq(remote),
                shq(&stage)
            );
            let out = adb()
                .args(["-s", &self.serial, "shell", &self.wrap(&prep)])
                .output()
                .await?;
            if !out.status.success() {
                anyhow::bail!("su stage failed: {}", String::from_utf8_lossy(&out.stderr));
            }
            let pull = adb()
                .args(["-s", &self.serial, "pull", &stage])
                .arg(local)
                .output()
                .await?;
            let _ = adb()
                .args(["-s", &self.serial, "shell", "rm -rf /sdcard/.imnf_dl"])
                .output()
                .await;
            if !pull.status.success() {
                anyhow::bail!("pull failed: {}", String::from_utf8_lossy(&pull.stderr));
            }
            return Ok(());
        }
        let out = adb()
            .args(["-s", &self.serial, "pull", remote])
            .arg(local)
            .output()
            .await?;
        if !out.status.success() {
            anyhow::bail!("pull failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(())
    }

    async fn upload(&self, local: &Path, remote_dir: &str) -> Result<()> {
        if self.elevated.load(Ordering::Relaxed) {
            let base = local
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "upload".to_string());
            let stage = format!("/sdcard/.imnf_ul/{base}");
            // push as shell into a /sdcard staging dir, then `su cp` into place
            let _ = adb()
                .args(["-s", &self.serial, "shell", "rm -rf /sdcard/.imnf_ul && mkdir -p /sdcard/.imnf_ul"])
                .output()
                .await?;
            let push = adb()
                .args(["-s", &self.serial, "push"])
                .arg(local)
                .arg("/sdcard/.imnf_ul/")
                .output()
                .await?;
            if !push.status.success() {
                anyhow::bail!("push failed: {}", String::from_utf8_lossy(&push.stderr));
            }
            let cpc = format!("cp -r {} {}", shq(&stage), shq(remote_dir));
            let out = adb()
                .args(["-s", &self.serial, "shell", &self.wrap(&cpc)])
                .output()
                .await?;
            let _ = adb()
                .args(["-s", &self.serial, "shell", "rm -rf /sdcard/.imnf_ul"])
                .output()
                .await;
            if !out.status.success() {
                anyhow::bail!("su copy failed: {}", String::from_utf8_lossy(&out.stderr));
            }
            return Ok(());
        }
        let out = adb()
            .args(["-s", &self.serial, "push"])
            .arg(local)
            .arg(remote_dir)
            .output()
            .await?;
        if !out.status.success() {
            anyhow::bail!("push failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(())
    }

    async fn exec(&self, cmd: &str) -> Result<ExecResult> {
        let out = adb()
            .args(["-s", &self.serial, "shell", cmd])
            .output()
            .await?;
        Ok(ExecResult {
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
            code: out.status.code().unwrap_or(-1),
        })
    }

    async fn open_shell(
        &self,
        app: tauri::AppHandle,
        shell_id: String,
        cols: u16,
        rows: u16,
    ) -> Result<ShellHandle> {
        // `adb shell` inside a real PTY (ConPTY on Windows) — without a tty adb
        // buffers output and skips device-PTY allocation (laggy / double-Enter).
        let mut cmd = CommandBuilder::new("adb");
        cmd.args(["-s", self.serial.as_str(), "shell"]);
        super::spawn_pty(app, shell_id, cols, rows, cmd).await
    }

    async fn elevate(&self, _password: Option<String>) -> Result<ElevateStatus> {
        // ADB never needs a password — `su` either grants root or it doesn't.
        let out = adb()
            .args(["-s", &self.serial, "shell", "su -c id"])
            .output()
            .await?;
        let text = String::from_utf8_lossy(&out.stdout);
        if text.contains("uid=0") {
            self.elevated.store(true, Ordering::Relaxed);
            Ok(ElevateStatus {
                elevated: true,
                needs_password: false,
                message: "root via su".into(),
            })
        } else {
            let detail = if text.trim().is_empty() {
                String::from_utf8_lossy(&out.stderr).trim().to_string()
            } else {
                text.trim().to_string()
            };
            anyhow::bail!("su unavailable (device not rooted?): {detail}")
        }
    }

    async fn unelevate(&self) {
        self.elevated.store(false, Ordering::Relaxed);
    }
}
