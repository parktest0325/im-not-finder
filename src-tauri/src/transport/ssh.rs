//! SSH/SFTP transport built on russh + russh-sftp (pure Rust).

use super::*;
use async_trait::async_trait;
use russh::client::{self, Handle};
use russh::keys::*;
use russh::ChannelMsg;
use russh_sftp::client::SftpSession;
use std::collections::VecDeque;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::Emitter;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex as AsyncMutex;

pub struct SshConnectOpts {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub key_path: Option<String>,
    pub key_passphrase: Option<String>,
}

struct ClientHandler;

#[async_trait]
impl client::Handler for ClientHandler {
    type Error = russh::Error;
    // Trust-on-first-use: accept any host key for now (MVP).
    async fn check_server_key(
        &mut self,
        _server_public_key: &key::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        Ok(true)
    }
}

pub struct SshTransport {
    label: String,
    home: String,
    session: Handle<ClientHandler>,
    sftp: SftpSession,
    elevated: AtomicBool,
    sudo_password: AsyncMutex<Option<String>>,
}

fn join_remote(dir: &str, name: &str) -> String {
    format!("{}/{}", dir.trim_end_matches('/'), name)
}

impl SshTransport {
    pub async fn connect(opts: SshConnectOpts) -> Result<Self> {
        let config = Arc::new(client::Config {
            // Don't self-disconnect on quiet periods; instead send keepalives so
            // the server / NAT doesn't drop an idle connection (which later shows
            // up as "failed to open channel" on the next shell/exec).
            inactivity_timeout: None,
            keepalive_interval: Some(Duration::from_secs(20)),
            keepalive_max: 3,
            ..Default::default()
        });
        let mut session =
            client::connect(config, (opts.host.as_str(), opts.port), ClientHandler).await?;

        let authed = if let Some(pw) = opts.password.as_deref() {
            session
                .authenticate_password(opts.username.as_str(), pw)
                .await?
        } else if let Some(kp) = opts.key_path.as_deref() {
            let key = load_secret_key(kp, opts.key_passphrase.as_deref())?;
            session
                .authenticate_publickey(opts.username.as_str(), Arc::new(key))
                .await?
        } else {
            anyhow::bail!("no authentication method supplied");
        };
        if !authed {
            anyhow::bail!("authentication failed");
        }

        let channel = session.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let sftp = SftpSession::new(channel.into_stream()).await?;
        let home = sftp
            .canonicalize(".")
            .await
            .unwrap_or_else(|_| "/".to_string());

        Ok(Self {
            label: format!("{}@{}", opts.username, opts.host),
            home,
            session,
            sftp,
            elevated: AtomicBool::new(false),
            sudo_password: AsyncMutex::new(None),
        })
    }

    fn is_elevated(&self) -> bool {
        self.elevated.load(Ordering::Relaxed)
    }

    /// Run a command over a fresh channel; returns (stdout, stderr, exit code).
    /// Loops until the channel CLOSE so the exit-status is never missed.
    async fn raw_exec(
        &self,
        cmd: &str,
        stdin: Option<Vec<u8>>,
    ) -> Result<(Vec<u8>, Vec<u8>, i32)> {
        let mut channel = self.session.channel_open_session().await?;
        channel.exec(true, cmd).await?;
        if let Some(data) = stdin {
            channel.data(&data[..]).await?;
            channel.eof().await?;
        }
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut code = -1;
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { ref data } => out.extend_from_slice(data),
                ChannelMsg::ExtendedData { ref data, .. } => err.extend_from_slice(data),
                ChannelMsg::ExitStatus { exit_status } => code = exit_status as i32,
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        Ok((out, err, code))
    }

    /// Run `inner` under sudo (NOPASSWD `-n`, or `-S` with the cached password).
    /// `extra_stdin` is appended after the password line (for e.g. `tee`).
    async fn sudo_run(
        &self,
        inner: &str,
        extra_stdin: Option<Vec<u8>>,
    ) -> Result<(Vec<u8>, Vec<u8>, i32)> {
        let pw = self.sudo_password.lock().await.clone();
        let (cmd, mut sin) = match &pw {
            Some(p) => {
                let mut v = p.clone().into_bytes();
                v.push(b'\n');
                (format!("sudo -S -p '' {inner}"), v)
            }
            None => (format!("sudo -n {inner}"), Vec::new()),
        };
        if let Some(extra) = extra_stdin {
            sin.extend_from_slice(&extra);
        }
        let stdin = if sin.is_empty() { None } else { Some(sin) };
        self.raw_exec(&cmd, stdin).await
    }

    async fn sudo_list_dir(&self, path: &str) -> Result<Vec<DirEntry>> {
        let inner = format!("LC_ALL=C ls -la {}", shq(path));
        let (out, err, code) = self.sudo_run(&inner, None).await?;
        if out.is_empty() && code != 0 {
            anyhow::bail!("sudo ls failed: {}", String::from_utf8_lossy(&err).trim());
        }
        let text = String::from_utf8_lossy(&out);
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
        Ok(entries)
    }

    /// Run a filesystem command (sudo-aware), failing on a non-zero exit.
    async fn fs_exec(&self, inner: &str) -> Result<()> {
        let (_, err, code) = if self.is_elevated() {
            self.sudo_run(inner, None).await?
        } else {
            self.raw_exec(inner, None).await?
        };
        if code != 0 {
            anyhow::bail!("{}", String::from_utf8_lossy(&err).trim());
        }
        Ok(())
    }
}

#[async_trait]
impl Transport for SshTransport {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn home(&self) -> String {
        self.home.clone()
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>> {
        if self.is_elevated() {
            return self.sudo_list_dir(path).await;
        }
        let rd = self.sftp.read_dir(path.to_string()).await?;
        let mut out = Vec::new();
        for entry in rd {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let ft = entry.file_type();
            let md = entry.metadata();
            let kind = if ft.is_dir() {
                "dir"
            } else if ft.is_symlink() {
                "symlink"
            } else if ft.is_file() {
                "file"
            } else {
                "other"
            };
            out.push(DirEntry {
                path: join_remote(path, &name),
                name,
                kind: kind.to_string(),
                size: md.size.unwrap_or(0),
                mode: md.permissions.unwrap_or(0),
                mtime: md.mtime.unwrap_or(0) as i64,
            });
        }
        Ok(out)
    }

    async fn read_chunk(&self, path: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        if self.is_elevated() {
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
            let (out, _e, _) = self.sudo_run(&dd, None).await?;
            return Ok(out);
        }
        let mut f = self.sftp.open(path.to_string()).await?;
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
        self.fs_exec(&format!("rm -rf {}", shq(path))).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        self.fs_exec(&format!("mv -f {} {}", shq(from), shq(to))).await
    }

    async fn copy(&self, from: &str, to_dir: &str) -> Result<()> {
        self.fs_exec(&format!("cp -r {} {}", shq(from), shq(to_dir)))
            .await
    }

    async fn walk_files(&self, root: &str) -> Result<Vec<(String, u64)>> {
        let inner = format!("find {} -type f", shq(root));
        let (out, _e, _c) = if self.is_elevated() {
            self.sudo_run(&inner, None).await?
        } else {
            self.raw_exec(&inner, None).await?
        };
        let text = String::from_utf8_lossy(&out);
        Ok(text
            .lines()
            .map(|l| l.trim_end())
            .filter(|l| !l.is_empty())
            .map(|l| (l.to_string(), 0u64))
            .collect())
    }

    async fn download(&self, remote: &str, local: &Path) -> Result<()> {
        if self.is_elevated() {
            let (_, _e, code) = self
                .sudo_run(&format!("test -d {}", shq(remote)), None)
                .await?;
            if code == 0 {
                // recursive dir copy via `sudo ls` + `sudo cat`
                let mut stack: VecDeque<(String, PathBuf)> = VecDeque::new();
                stack.push_back((remote.to_string(), local.to_path_buf()));
                while let Some((rdir, ldir)) = stack.pop_front() {
                    tokio::fs::create_dir_all(&ldir).await?;
                    for e in self.sudo_list_dir(&rdir).await? {
                        if e.kind == "dir" {
                            stack.push_back((e.path, ldir.join(&e.name)));
                        } else {
                            let (data, _e, _) =
                                self.sudo_run(&format!("cat {}", shq(&e.path)), None).await?;
                            tokio::fs::write(ldir.join(&e.name), data).await?;
                        }
                    }
                }
            } else {
                if let Some(parent) = local.parent() {
                    tokio::fs::create_dir_all(parent).await.ok();
                }
                let (data, _e, _) = self.sudo_run(&format!("cat {}", shq(remote)), None).await?;
                tokio::fs::write(local, data).await?;
            }
            return Ok(());
        }
        let md = self.sftp.metadata(remote.to_string()).await?;
        if md.is_dir() {
            // breadth-first directory copy
            let mut stack: VecDeque<(String, PathBuf)> = VecDeque::new();
            stack.push_back((remote.to_string(), local.to_path_buf()));
            while let Some((rdir, ldir)) = stack.pop_front() {
                tokio::fs::create_dir_all(&ldir).await?;
                for entry in self.sftp.read_dir(rdir.clone()).await? {
                    let name = entry.file_name();
                    if name == "." || name == ".." {
                        continue;
                    }
                    let rpath = join_remote(&rdir, &name);
                    let lpath = ldir.join(&name);
                    if entry.file_type().is_dir() {
                        stack.push_back((rpath, lpath));
                    } else {
                        let mut rf = self.sftp.open(rpath).await?;
                        let mut data = Vec::new();
                        rf.read_to_end(&mut data).await?;
                        tokio::fs::write(lpath, data).await?;
                    }
                }
            }
        } else {
            if let Some(parent) = local.parent() {
                tokio::fs::create_dir_all(parent).await.ok();
            }
            let mut rf = self.sftp.open(remote.to_string()).await?;
            let mut data = Vec::new();
            rf.read_to_end(&mut data).await?;
            tokio::fs::write(local, data).await?;
        }
        Ok(())
    }

    async fn upload(&self, local: &Path, remote_dir: &str) -> Result<()> {
        let meta = tokio::fs::metadata(local).await?;
        let base = local
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "upload".to_string());
        if self.is_elevated() {
            if meta.is_dir() {
                let mut stack: VecDeque<(PathBuf, String)> = VecDeque::new();
                stack.push_back((local.to_path_buf(), join_remote(remote_dir, &base)));
                while let Some((ldir, rdir)) = stack.pop_front() {
                    self.sudo_run(&format!("mkdir -p {}", shq(&rdir)), None).await?;
                    let mut rd = tokio::fs::read_dir(&ldir).await?;
                    while let Some(ent) = rd.next_entry().await? {
                        let name = ent.file_name().to_string_lossy().to_string();
                        let lpath = ent.path();
                        let rpath = join_remote(&rdir, &name);
                        if ent.file_type().await?.is_dir() {
                            stack.push_back((lpath, rpath));
                        } else {
                            let data = tokio::fs::read(&lpath).await?;
                            let (_, err, code) = self
                                .sudo_run(&format!("tee {} >/dev/null", shq(&rpath)), Some(data))
                                .await?;
                            if code != 0 {
                                anyhow::bail!(
                                    "sudo upload failed: {}",
                                    String::from_utf8_lossy(&err).trim()
                                );
                            }
                        }
                    }
                }
            } else {
                let data = tokio::fs::read(local).await?;
                let dest = join_remote(remote_dir, &base);
                let (_, err, code) = self
                    .sudo_run(&format!("tee {} >/dev/null", shq(&dest)), Some(data))
                    .await?;
                if code != 0 {
                    anyhow::bail!("sudo upload failed: {}", String::from_utf8_lossy(&err).trim());
                }
            }
            return Ok(());
        }
        if meta.is_dir() {
            let mut stack: VecDeque<(PathBuf, String)> = VecDeque::new();
            stack.push_back((local.to_path_buf(), join_remote(remote_dir, &base)));
            while let Some((ldir, rdir)) = stack.pop_front() {
                self.sftp.create_dir(rdir.clone()).await.ok();
                let mut rd = tokio::fs::read_dir(&ldir).await?;
                while let Some(ent) = rd.next_entry().await? {
                    let name = ent.file_name().to_string_lossy().to_string();
                    let lpath = ent.path();
                    let rpath = join_remote(&rdir, &name);
                    if ent.file_type().await?.is_dir() {
                        stack.push_back((lpath, rpath));
                    } else {
                        let data = tokio::fs::read(&lpath).await?;
                        let mut wf = self.sftp.create(rpath).await?;
                        wf.write_all(&data).await?;
                        wf.shutdown().await?;
                    }
                }
            }
        } else {
            let data = tokio::fs::read(local).await?;
            let mut wf = self.sftp.create(join_remote(remote_dir, &base)).await?;
            wf.write_all(&data).await?;
            wf.shutdown().await?;
        }
        Ok(())
    }

    async fn exec(&self, cmd: &str) -> Result<ExecResult> {
        let mut channel = self.session.channel_open_session().await?;
        channel.exec(true, cmd).await?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut code = -1;
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { ref data } => stdout.extend_from_slice(data),
                ChannelMsg::ExtendedData { ref data, .. } => stderr.extend_from_slice(data),
                ChannelMsg::ExitStatus { exit_status } => code = exit_status as i32,
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        Ok(ExecResult {
            stdout: String::from_utf8_lossy(&stdout).to_string(),
            stderr: String::from_utf8_lossy(&stderr).to_string(),
            code,
        })
    }

    async fn open_shell(
        &self,
        app: tauri::AppHandle,
        shell_id: String,
        cols: u16,
        rows: u16,
    ) -> Result<ShellHandle> {
        let mut channel = self.session.channel_open_session().await?;
        channel
            .request_pty(false, "xterm-256color", cols as u32, rows as u32, 0, 0, &[])
            .await?;
        channel.request_shell(true).await?;

        let (in_tx, mut in_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (rs_tx, mut rs_rx) = mpsc::unbounded_channel::<(u16, u16)>();
        let (kill_tx, mut kill_rx) = mpsc::unbounded_channel::<()>();

        let ev = format!("term://{shell_id}");
        let app2 = app.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    msg = channel.wait() => match msg {
                        Some(ChannelMsg::Data { ref data }) => { let _ = app2.emit(&ev, data.to_vec()); }
                        Some(ChannelMsg::ExtendedData { ref data, .. }) => { let _ = app2.emit(&ev, data.to_vec()); }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        _ => {}
                    },
                    Some(bytes) = in_rx.recv() => { let _ = channel.data(&bytes[..]).await; }
                    Some((c, r)) = rs_rx.recv() => { let _ = channel.window_change(c as u32, r as u32, 0, 0).await; }
                    _ = kill_rx.recv() => { let _ = channel.eof().await; break; }
                }
            }
        });

        Ok(ShellHandle {
            input: in_tx,
            resize: rs_tx,
            close: Box::new(move || {
                let _ = kill_tx.send(());
            }),
        })
    }

    async fn elevate(&self, password: Option<String>) -> Result<ElevateStatus> {
        match password {
            None => {
                // try passwordless sudo first
                let (_, _e, code) = self.raw_exec("sudo -n true", None).await?;
                if code == 0 {
                    *self.sudo_password.lock().await = None;
                    self.elevated.store(true, Ordering::Relaxed);
                    Ok(ElevateStatus {
                        elevated: true,
                        needs_password: false,
                        message: "passwordless sudo".into(),
                    })
                } else {
                    Ok(ElevateStatus {
                        elevated: false,
                        needs_password: true,
                        message: "sudo password required".into(),
                    })
                }
            }
            Some(pw) => {
                let mut sin = pw.clone().into_bytes();
                sin.push(b'\n');
                let (_, err, code) = self.raw_exec("sudo -S -p '' true", Some(sin)).await?;
                if code == 0 {
                    *self.sudo_password.lock().await = Some(pw);
                    self.elevated.store(true, Ordering::Relaxed);
                    Ok(ElevateStatus {
                        elevated: true,
                        needs_password: false,
                        message: "sudo authenticated".into(),
                    })
                } else {
                    // surface the real reason (wrong password, requiretty, etc.)
                    let detail = String::from_utf8_lossy(&err);
                    let detail = detail.trim();
                    let message = if detail.is_empty() {
                        format!("sudo failed (exit {code})")
                    } else {
                        format!("sudo: {detail}")
                    };
                    Ok(ElevateStatus {
                        elevated: false,
                        needs_password: true,
                        message,
                    })
                }
            }
        }
    }

    async fn unelevate(&self) {
        self.elevated.store(false, Ordering::Relaxed);
        *self.sudo_password.lock().await = None;
    }
}
