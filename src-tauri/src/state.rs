use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};
use std::time::SystemTime;

use rusqlite::Connection;
use tokio::sync::broadcast;

use crate::audio::EngineCmd;
use crate::dto::TickDto;

pub struct AppContext {
    /// main-connection for synchronous commands (short queries only)
    pub db: Mutex<Connection>,
    pub db_path: PathBuf,
    pub covers_dir: PathBuf,
    pub engine: mpsc::Sender<EngineCmd>,
    pub ticks: broadcast::Sender<TickDto>,
    pub settings: Mutex<crate::dto::SettingsDto>,
    pub scanning: Arc<AtomicBool>,
    /// Per-track fetch generations; retain slots so an old request can never
    /// become current again after a metadata edit or a newer request.
    pub metadata_fetches: Mutex<HashMap<i64, Arc<Mutex<u64>>>>,
}

impl AppContext {
    pub fn metadata_fetch_slot(&self, track_id: i64) -> Arc<Mutex<u64>> {
        self.metadata_fetches
            .lock()
            .unwrap()
            .entry(track_id)
            .or_insert_with(|| Arc::new(Mutex::new(0)))
            .clone()
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
