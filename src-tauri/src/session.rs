//! Live session + shell registry, stored as Tauri managed state.

use crate::transport::{ShellHandle, Transport};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Default)]
pub struct AppState {
    sessions: Mutex<HashMap<String, Arc<dyn Transport>>>,
    shells: Mutex<HashMap<String, ShellHandle>>,
    counter: AtomicU64,
}

impl AppState {
    pub fn next_id(&self, prefix: &str) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("{prefix}-{n}")
    }

    pub async fn add_session(&self, id: String, t: Arc<dyn Transport>) {
        self.sessions.lock().await.insert(id, t);
    }

    pub async fn get(&self, id: &str) -> Option<Arc<dyn Transport>> {
        self.sessions.lock().await.get(id).cloned()
    }

    pub async fn remove_session(&self, id: &str) {
        self.sessions.lock().await.remove(id);
        // also drop any shells that belong to this session
        let mut shells = self.shells.lock().await;
        let to_close: Vec<String> = shells
            .keys()
            .filter(|k| k.starts_with(&format!("{id}::")))
            .cloned()
            .collect();
        for k in to_close {
            if let Some(h) = shells.remove(&k) {
                (h.close)();
            }
        }
    }

    pub async fn add_shell(&self, id: String, handle: ShellHandle) {
        self.shells.lock().await.insert(id, handle);
    }

    pub async fn write_shell(&self, id: &str, data: Vec<u8>) -> bool {
        match self.shells.lock().await.get(id) {
            Some(h) => h.input.send(data).is_ok(),
            None => false,
        }
    }

    pub async fn resize_shell(&self, id: &str, cols: u16, rows: u16) -> bool {
        match self.shells.lock().await.get(id) {
            Some(h) => h.resize.send((cols, rows)).is_ok(),
            None => false,
        }
    }

    pub async fn close_shell(&self, id: &str) {
        if let Some(h) = self.shells.lock().await.remove(id) {
            (h.close)();
        }
    }
}
