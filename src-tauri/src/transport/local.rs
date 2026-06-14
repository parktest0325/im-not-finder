//! Local transport: the host machine itself — its default shell (PowerShell on
//! Windows, `$SHELL` on Unix) and its filesystem. Paths use forward slashes,
//! which Windows std::fs also accepts.

use super::*;
use portable_pty::CommandBuilder;
use std::collections::VecDeque;
use std::io::SeekFrom;
use std::path::Path;
use std::time::UNIX_EPOCH;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

pub struct LocalTransport {
    home: String,
    label: String,
}

fn norm(p: &str) -> String {
    p.replace('\\', "/")
}

fn home_dir() -> String {
    #[cfg(windows)]
    let h = std::env::var("USERPROFILE").ok();
    #[cfg(not(windows))]
    let h = std::env::var("HOME").ok();
    norm(&h.unwrap_or_else(|| "/".to_string()))
}

#[cfg(windows)]
fn shell_name() -> String {
    "powershell".to_string()
}
#[cfg(not(windows))]
fn shell_name() -> String {
    std::env::var("SHELL")
        .ok()
        .and_then(|s| s.rsplit('/').next().map(|x| x.to_string()))
        .unwrap_or_else(|| "sh".to_string())
}

#[cfg(windows)]
fn shell_cmd() -> CommandBuilder {
    let mut c = CommandBuilder::new("powershell.exe");
    c.arg("-NoLogo");
    c
}
#[cfg(not(windows))]
fn shell_cmd() -> CommandBuilder {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    CommandBuilder::new(shell)
}

fn join_local(dir: &str, name: &str) -> String {
    format!("{}/{}", dir.trim_end_matches('/'), name)
}

/// A bare drive letter like `C:` means the drive root `C:/`.
fn fs_path(path: &str) -> String {
    let p = norm(path);
    if p.len() == 2 && p.ends_with(':') {
        format!("{p}/")
    } else {
        p
    }
}

impl LocalTransport {
    pub fn connect() -> Result<Self> {
        Ok(Self {
            home: home_dir(),
            label: format!("local · {}", shell_name()),
        })
    }
}

#[async_trait]
impl Transport for LocalTransport {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn home(&self) -> String {
        self.home.clone()
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>> {
        let dir = fs_path(path);
        let mut rd = tokio::fs::read_dir(&dir).await?;
        let mut out = Vec::new();
        while let Some(ent) = rd.next_entry().await? {
            let name = ent.file_name().to_string_lossy().to_string();
            let ft = ent.file_type().await?;
            let md = ent.metadata().await.ok();
            let kind = if ft.is_dir() {
                "dir"
            } else if ft.is_symlink() {
                "symlink"
            } else {
                "file"
            };
            let mtime = md
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            out.push(DirEntry {
                path: join_local(&dir, &name),
                name,
                kind: kind.to_string(),
                size: md.as_ref().map(|m| m.len()).unwrap_or(0),
                mode: 0,
                mtime,
            });
        }
        Ok(out)
    }

    async fn read_chunk(&self, path: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        let mut f = tokio::fs::File::open(norm(path)).await?;
        f.seek(SeekFrom::Start(offset)).await?;
        let mut buf = vec![0u8; len as usize];
        let mut filled = 0usize;
        while filled < buf.len() {
            let n = f.read(&mut buf[filled..]).await?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        buf.truncate(filled);
        Ok(buf)
    }

    async fn remove(&self, path: &str) -> Result<()> {
        let p = norm(path);
        let md = tokio::fs::symlink_metadata(&p).await?;
        if md.is_dir() {
            tokio::fs::remove_dir_all(&p).await?;
        } else {
            tokio::fs::remove_file(&p).await?;
        }
        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        tokio::fs::rename(norm(from), norm(to)).await?;
        Ok(())
    }

    async fn copy(&self, from: &str, to_dir: &str) -> Result<()> {
        let src = norm(from);
        let base = src.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("copy");
        let dest = join_local(&norm(to_dir), base);
        copy_tree(&src, &dest).await
    }

    async fn walk_files(&self, root: &str) -> Result<Vec<(String, u64)>> {
        let mut out = Vec::new();
        let mut stack = vec![fs_path(root)];
        while let Some(dir) = stack.pop() {
            let mut rd = match tokio::fs::read_dir(&dir).await {
                Ok(r) => r,
                Err(_) => continue,
            };
            while let Some(ent) = rd.next_entry().await? {
                let p = norm(&ent.path().to_string_lossy());
                if ent.file_type().await?.is_dir() {
                    stack.push(p);
                } else {
                    let size = ent.metadata().await.map(|m| m.len()).unwrap_or(0);
                    out.push((p, size));
                }
            }
        }
        Ok(out)
    }

    async fn download(&self, remote: &str, local: &Path) -> Result<()> {
        if let Some(parent) = local.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        copy_tree(&norm(remote), &local.to_string_lossy()).await
    }

    async fn upload(&self, local: &Path, remote_dir: &str) -> Result<()> {
        let base = local
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "upload".to_string());
        let dest = join_local(&norm(remote_dir), &base);
        copy_tree(&local.to_string_lossy(), &dest).await
    }

    async fn exec(&self, cmd: &str) -> Result<ExecResult> {
        let mut command;
        #[cfg(windows)]
        {
            command = tokio::process::Command::new("powershell.exe");
            command.args(["-NoProfile", "-NoLogo", "-Command", cmd]);
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW (tokio inherent)
        }
        #[cfg(not(windows))]
        {
            command = tokio::process::Command::new("sh");
            command.args(["-c", cmd]);
        }
        let out = command.output().await?;
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
        let mut cmd = shell_cmd();
        cmd.cwd(self.home.clone());
        super::spawn_pty(app, shell_id, cols, rows, cmd).await
    }

    async fn elevate(&self, _password: Option<String>) -> Result<ElevateStatus> {
        Ok(ElevateStatus {
            elevated: false,
            needs_password: false,
            message: "elevation not supported for local sessions".into(),
        })
    }
    async fn unelevate(&self) {}
}

/// Recursively copy a file or directory tree from `src` to `dest`.
async fn copy_tree(src: &str, dest: &str) -> Result<()> {
    let meta = tokio::fs::metadata(src).await?;
    if !meta.is_dir() {
        tokio::fs::copy(src, dest).await?;
        return Ok(());
    }
    let mut stack: VecDeque<(String, String)> = VecDeque::new();
    stack.push_back((src.to_string(), dest.to_string()));
    while let Some((s, d)) = stack.pop_front() {
        tokio::fs::create_dir_all(&d).await?;
        let mut rd = tokio::fs::read_dir(&s).await?;
        while let Some(ent) = rd.next_entry().await? {
            let name = ent.file_name().to_string_lossy().to_string();
            let sp = join_local(&s, &name);
            let dp = join_local(&d, &name);
            if ent.file_type().await?.is_dir() {
                stack.push_back((sp, dp));
            } else {
                tokio::fs::copy(&sp, &dp).await?;
            }
        }
    }
    Ok(())
}
