//! Persistent connection history (stored as JSON in the app config dir).
//! Secrets (passwords / passphrases) are intentionally NOT saved.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SshHistoryEntry {
    pub id: String, // stable: "user@host:port"
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_mode: String, // "password" | "key"
    pub key_path: Option<String>,
    pub last_used: i64, // unix seconds
}

fn history_path(app: &AppHandle) -> anyhow::Result<PathBuf> {
    let dir = app.path().app_config_dir()?;
    std::fs::create_dir_all(&dir).ok();
    Ok(dir.join("ssh_history.json"))
}

pub fn load(app: &AppHandle) -> Vec<SshHistoryEntry> {
    let Ok(path) = history_path(app) else {
        return vec![];
    };
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => vec![],
    }
}

fn save_all(app: &AppHandle, list: &[SshHistoryEntry]) -> anyhow::Result<()> {
    let path = history_path(app)?;
    std::fs::write(path, serde_json::to_string_pretty(list)?)?;
    Ok(())
}

/// Insert or refresh an entry, keeping most-recently-used first.
pub fn upsert(app: &AppHandle, mut entry: SshHistoryEntry) {
    entry.last_used = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut list = load(app);
    list.retain(|e| e.id != entry.id);
    list.insert(0, entry);
    list.truncate(50);
    let _ = save_all(app, &list);
}

pub fn delete(app: &AppHandle, id: &str) {
    let mut list = load(app);
    list.retain(|e| e.id != id);
    let _ = save_all(app, &list);
}
