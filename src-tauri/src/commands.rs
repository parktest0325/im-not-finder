//! Tauri command layer bridging the frontend to the transports.

use crate::session::AppState;
use crate::transport::adb::{self, AdbTransport};
use crate::transport::ssh::{SshConnectOpts, SshTransport};
use crate::transport::{DirEntry, ElevateStatus, ExecResult, Transport};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, State};

type R<T> = Result<T, String>;
fn e<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    id: String,
    kind: String,
    label: String,
    home: String,
}

fn info(id: String, kind: &str, t: &Arc<dyn Transport>) -> SessionInfo {
    SessionInfo {
        id,
        kind: kind.to_string(),
        label: t.label(),
        home: t.home(),
    }
}

// ---------------- sessions ----------------

#[tauri::command]
pub async fn list_adb_devices() -> R<Vec<adb::AdbDevice>> {
    adb::list_devices().await.map_err(e)
}

#[tauri::command]
pub async fn connect_adb(state: State<'_, AppState>, serial: String) -> R<SessionInfo> {
    let t = AdbTransport::connect(&serial).await.map_err(e)?;
    let t: Arc<dyn Transport> = Arc::new(t);
    let id = state.next_id("adb");
    state.add_session(id.clone(), t.clone()).await;
    Ok(info(id, "adb", &t))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshOptsArg {
    host: String,
    port: u16,
    username: String,
    password: Option<String>,
    key_path: Option<String>,
    key_passphrase: Option<String>,
}

#[tauri::command]
pub async fn connect_ssh(
    app: AppHandle,
    state: State<'_, AppState>,
    opts: SshOptsArg,
) -> R<SessionInfo> {
    let auth_mode = if opts.password.is_some() {
        "password"
    } else {
        "key"
    };
    let hist = crate::config::SshHistoryEntry {
        id: format!("{}@{}:{}", opts.username, opts.host, opts.port),
        host: opts.host.clone(),
        port: opts.port,
        username: opts.username.clone(),
        auth_mode: auth_mode.to_string(),
        key_path: opts.key_path.clone(),
        last_used: 0,
    };
    let t = SshTransport::connect(SshConnectOpts {
        host: opts.host,
        port: opts.port,
        username: opts.username,
        password: opts.password,
        key_path: opts.key_path,
        key_passphrase: opts.key_passphrase,
    })
    .await
    .map_err(e)?;
    crate::config::upsert(&app, hist); // remember on success (no secrets)
    let t: Arc<dyn Transport> = Arc::new(t);
    let id = state.next_id("ssh");
    state.add_session(id.clone(), t.clone()).await;
    Ok(info(id, "ssh", &t))
}

#[tauri::command]
pub fn list_ssh_history(app: AppHandle) -> R<Vec<crate::config::SshHistoryEntry>> {
    Ok(crate::config::load(&app))
}

#[tauri::command]
pub fn delete_ssh_history(app: AppHandle, id: String) -> R<()> {
    crate::config::delete(&app, &id);
    Ok(())
}

#[tauri::command]
pub async fn disconnect_session(state: State<'_, AppState>, session_id: String) -> R<()> {
    state.remove_session(&session_id).await;
    Ok(())
}

// ---------------- filesystem ----------------

async fn session(state: &State<'_, AppState>, id: &str) -> R<Arc<dyn Transport>> {
    state.get(id).await.ok_or_else(|| "no such session".into())
}

#[tauri::command]
pub async fn list_dir(state: State<'_, AppState>, session_id: String, path: String) -> R<Vec<DirEntry>> {
    let t = session(&state, &session_id).await?;
    t.list_dir(&path).await.map_err(e)
}

#[tauri::command]
pub async fn read_chunk(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
    offset: u64,
    len: u64,
) -> R<Vec<u8>> {
    let t = session(&state, &session_id).await?;
    t.read_chunk(&path, offset, len).await.map_err(e)
}

#[tauri::command]
pub async fn stage_for_drag(state: State<'_, AppState>, session_id: String, path: String) -> R<String> {
    let t = session(&state, &session_id).await?;
    let base = path.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("file");
    let dir = tempfile::Builder::new()
        .prefix("imnf-drag-")
        .tempdir()
        .map_err(e)?
        .keep(); // persist so the file survives until the OS drag completes
    let local = dir.join(base);
    t.download(&path, &local).await.map_err(e)?;
    Ok(local.to_string_lossy().to_string())
}

/// Materialise a small PNG on disk and return its path, for use as the native
/// drag preview image (the drag plugin requires a real image file).
#[tauri::command]
pub fn drag_icon() -> R<String> {
    use std::io::Write;
    let dir = std::env::temp_dir().join("imnf");
    std::fs::create_dir_all(&dir).map_err(e)?;
    let path = dir.join("drag-icon.png");
    if !path.exists() {
        let bytes = include_bytes!("../icons/32x32.png");
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(bytes))
            .map_err(e)?;
    }
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn upload(
    state: State<'_, AppState>,
    session_id: String,
    local_path: String,
    remote_dir: String,
) -> R<()> {
    let t = session(&state, &session_id).await?;
    t.upload(std::path::Path::new(&local_path), &remote_dir)
        .await
        .map_err(e)
}

#[tauri::command]
pub async fn elevate(
    state: State<'_, AppState>,
    session_id: String,
    password: Option<String>,
) -> R<ElevateStatus> {
    let t = session(&state, &session_id).await?;
    t.elevate(password).await.map_err(e)
}

#[tauri::command]
pub async fn unelevate(state: State<'_, AppState>, session_id: String) -> R<()> {
    let t = session(&state, &session_id).await?;
    t.unelevate().await;
    Ok(())
}

#[tauri::command]
pub async fn remove_path(state: State<'_, AppState>, session_id: String, path: String) -> R<()> {
    let t = session(&state, &session_id).await?;
    t.remove(&path).await.map_err(e)
}

#[tauri::command]
pub async fn rename_path(
    state: State<'_, AppState>,
    session_id: String,
    from: String,
    to: String,
) -> R<()> {
    let t = session(&state, &session_id).await?;
    t.rename(&from, &to).await.map_err(e)
}

#[tauri::command]
pub async fn copy_path(
    state: State<'_, AppState>,
    session_id: String,
    from: String,
    to_dir: String,
) -> R<()> {
    let t = session(&state, &session_id).await?;
    t.copy(&from, &to_dir).await.map_err(e)
}

#[tauri::command]
pub async fn exec_command(
    state: State<'_, AppState>,
    session_id: String,
    command: String,
) -> R<ExecResult> {
    let t = session(&state, &session_id).await?;
    t.exec(&command).await.map_err(e)
}

// ---------------- shell ----------------

#[tauri::command]
pub async fn shell_open(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> R<String> {
    let t = session(&state, &session_id).await?;
    let shell_id = format!("{session_id}::{}", state.next_id("sh"));
    let handle = t
        .open_shell(app, shell_id.clone(), cols, rows)
        .await
        .map_err(e)?;
    state.add_shell(shell_id.clone(), handle).await;
    Ok(shell_id)
}

#[tauri::command]
pub async fn shell_write(state: State<'_, AppState>, shell_id: String, data: String) -> R<()> {
    state.write_shell(&shell_id, data.into_bytes()).await;
    Ok(())
}

#[tauri::command]
pub async fn shell_resize(state: State<'_, AppState>, shell_id: String, cols: u16, rows: u16) -> R<()> {
    state.resize_shell(&shell_id, cols, rows).await;
    Ok(())
}

#[tauri::command]
pub async fn shell_close(state: State<'_, AppState>, shell_id: String) -> R<()> {
    state.close_shell(&shell_id).await;
    Ok(())
}
