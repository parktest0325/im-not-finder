//! Protocol-agnostic transport abstraction shared by the SSH, ADB and local backends.

pub mod adb;
pub mod local;
pub mod ssh;

use async_trait::async_trait;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex};
use tauri::Emitter;
use tokio::sync::mpsc;

pub type Result<T> = anyhow::Result<T>;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub kind: String, // "file" | "dir" | "symlink" | "other"
    pub size: u64,
    pub mode: u32,
    pub mtime: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ElevateStatus {
    pub elevated: bool,
    /// SSH: sudo needs a password we don't have yet (prompt the user).
    pub needs_password: bool,
    pub message: String,
}

/// Handle to a live interactive shell. Dropping it (via `close`) tears the shell down.
pub struct ShellHandle {
    /// raw bytes typed by the user, forwarded to the shell's stdin
    pub input: mpsc::UnboundedSender<Vec<u8>>,
    /// (cols, rows) resize requests
    pub resize: mpsc::UnboundedSender<(u16, u16)>,
    /// stops the streaming task / kills the child
    pub close: Box<dyn Fn() + Send + Sync>,
}

#[async_trait]
pub trait Transport: Send + Sync {
    /// Human-readable label shown in the tab/status bar.
    fn label(&self) -> String;
    /// Initial directory to open in the file tree.
    fn home(&self) -> String;

    async fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>>;
    async fn read_chunk(&self, path: &str, offset: u64, len: u64) -> Result<Vec<u8>>;
    /// Delete a file or directory (recursively).
    async fn remove(&self, path: &str) -> Result<()>;
    /// Rename/move `from` to the absolute path `to`.
    async fn rename(&self, from: &str, to: &str) -> Result<()>;
    /// Copy `from` (file or dir) into the directory `to_dir`, keeping its name.
    async fn copy(&self, from: &str, to_dir: &str) -> Result<()>;
    /// Download a remote file (or directory tree) to a local path.
    async fn download(&self, remote: &str, local: &Path) -> Result<()>;
    /// Upload a local file (or directory) into a remote directory.
    async fn upload(&self, local: &Path, remote_dir: &str) -> Result<()>;
    async fn exec(&self, cmd: &str) -> Result<ExecResult>;

    /// Open an interactive shell that streams output via `term://<shell_id>` events.
    async fn open_shell(
        &self,
        app: tauri::AppHandle,
        shell_id: String,
        cols: u16,
        rows: u16,
    ) -> Result<ShellHandle>;

    /// Turn privilege elevation on for FILES/VIEWER operations.
    /// `password` is only consulted by SSH (sudo); pass None first to auto-detect.
    async fn elevate(&self, password: Option<String>) -> Result<ElevateStatus>;
    /// Turn elevation back off (and forget any cached sudo password).
    async fn unelevate(&self);
}

/// Single-quote a string for a POSIX shell (`/system/bin/sh`, bash, ...).
pub fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Spawn `cmd` inside a real PTY (ConPTY on Windows) and stream its output via
/// `term://<shell_id>` events. Shared by the ADB and local-shell transports.
pub async fn spawn_pty(
    app: tauri::AppHandle,
    shell_id: String,
    cols: u16,
    rows: u16,
    cmd: CommandBuilder,
) -> Result<ShellHandle> {
    let size = PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    };

    let (killer, reader, writer, master) =
        tokio::task::spawn_blocking(move || -> Result<_> {
            let pty = native_pty_system();
            let pair = pty.openpty(size)?;
            let child = pair.slave.spawn_command(cmd)?;
            drop(pair.slave);
            let killer = child.clone_killer();
            std::thread::spawn(move || {
                let mut child = child;
                let _ = child.wait();
            });
            let reader = pair.master.try_clone_reader()?;
            let writer = pair.master.take_writer()?;
            Ok((killer, reader, writer, pair.master))
        })
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))??;

    let ev = format!("term://{shell_id}");
    let app_out = app.clone();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let _ = app_out.emit(&ev, buf[..n].to_vec());
                }
            }
        }
    });

    let writer = Arc::new(StdMutex::new(writer));
    let master = Arc::new(StdMutex::new(master));
    let killer = Arc::new(StdMutex::new(killer));
    let (in_tx, mut in_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (rs_tx, mut rs_rx) = mpsc::unbounded_channel::<(u16, u16)>();
    let (kill_tx, mut kill_rx) = mpsc::unbounded_channel::<()>();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(data) = in_rx.recv() => {
                    let w = writer.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Ok(mut g) = w.lock() {
                            let _ = g.write_all(&data);
                            let _ = g.flush();
                        }
                    }).await;
                }
                Some((c, r)) = rs_rx.recv() => {
                    let m = master.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Ok(g) = m.lock() {
                            let _ = g.resize(PtySize { rows: r, cols: c, pixel_width: 0, pixel_height: 0 });
                        }
                    }).await;
                }
                _ = kill_rx.recv() => {
                    if let Ok(mut k) = killer.lock() {
                        let _ = k.kill();
                    }
                    break;
                }
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

fn is_hhmm(t: &str) -> bool {
    // 10:11 or 10:11:12
    t.contains(':') && t.len() >= 4 && t.chars().all(|c| c.is_ascii_digit() || c == ':')
}
fn is_year(t: &str) -> bool {
    t.len() == 4 && t.chars().all(|c| c.is_ascii_digit()) && (t.starts_with("19") || t.starts_with("20"))
}
fn is_month(t: &str) -> bool {
    matches!(
        t,
        "Jan" | "Feb" | "Mar" | "Apr" | "May" | "Jun" | "Jul" | "Aug" | "Sep" | "Oct" | "Nov" | "Dec"
    )
}

/// Parse a single `ls -la` line from toybox / BusyBox / GNU coreutils.
/// Column layout is `perms links owner group size <date...> name`; the date
/// block varies (`2024-06-14 10:11`, `Jun 14 10:11`, `Jun 14 2024`), so we find
/// the name by locating the time (HH:MM) or year token. Size is column 5.
pub fn parse_ls_line(line: &str, dir: &str) -> Option<DirEntry> {
    let perms = line.split_whitespace().next()?;
    if perms.len() < 10 {
        return None;
    }
    let kind = kind_from_perm(perms.chars().next()?);

    // tokens with their byte offsets so we can slice the name out by position
    let tokens: Vec<(usize, &str)> = line
        .match_indices(|c: char| !c.is_whitespace())
        .fold(Vec::<(usize, usize)>::new(), |mut acc, (i, _)| {
            match acc.last_mut() {
                Some(last) if last.1 == i => last.1 = i + 1,
                _ => acc.push((i, i + 1)),
            }
            acc
        })
        .into_iter()
        .map(|(s, e)| (s, &line[s..e]))
        .collect();

    if tokens.len() < 6 {
        return None;
    }

    // name begins right after the time token (HH:MM), or after the year token
    // (old files with no time, e.g. `Jun 14 2024 name`).
    let mut name_idx = None;
    for i in 5..tokens.len() {
        if is_hhmm(tokens[i].1) {
            name_idx = Some(i + 1);
            break;
        }
    }
    if name_idx.is_none() {
        for i in 6..tokens.len() {
            if is_year(tokens[i].1) && is_month(tokens[i - 2].1) {
                name_idx = Some(i + 1);
                break;
            }
        }
    }
    let name_tok = tokens.get(name_idx?)?;

    let size = tokens[4].1.parse::<u64>().unwrap_or(0); // column 5 = size
    let mut name = line[name_tok.0..].trim_end().to_string();
    if kind == "symlink" {
        if let Some(pos) = name.find(" -> ") {
            name = name[..pos].to_string();
        }
    }
    if name == "." || name == ".." || name.is_empty() {
        return None;
    }
    let path = if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    };
    Some(DirEntry {
        name,
        path,
        kind: kind.to_string(),
        size,
        mode: mode_from_perms(perms),
        mtime: 0,
    })
}

/// Classify a unix `ls -l` permission string's first character.
pub fn kind_from_perm(first: char) -> &'static str {
    match first {
        'd' => "dir",
        'l' => "symlink",
        '-' => "file",
        _ => "other",
    }
}

/// Parse the leading `rwx` permission triplets into mode bits (best effort).
pub fn mode_from_perms(perms: &str) -> u32 {
    let bytes = perms.as_bytes();
    if bytes.len() < 10 {
        return 0;
    }
    let mut mode = 0u32;
    for (start, shift) in [(1usize, 6u32), (4, 3), (7, 0)] {
        if bytes[start] == b'r' {
            mode |= 4 << shift;
        }
        if bytes[start + 1] == b'w' {
            mode |= 2 << shift;
        }
        let x = bytes[start + 2];
        if x == b'x' || x == b's' || x == b't' {
            mode |= 1 << shift;
        }
    }
    mode
}
