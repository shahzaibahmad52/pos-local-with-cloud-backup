//! Daily backup files on the main PC itself: a copy of the store database
//! in a `backups` folder next to it, made once a day, last 14 kept. They
//! work with no internet and are easy to copy to a USB drive.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;

use crate::error::{AppError, AppResult};

pub const KEEP: usize = 14;
const PREFIX: &str = "store-";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub name: String,
    pub size: u64,
}

/// Backup files, newest first.
pub fn list(dir: &Path) -> Vec<BackupFile> {
    let mut files: Vec<BackupFile> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    (name.starts_with(PREFIX) && name.ends_with(".sqlite"))
                        .then(|| BackupFile { size: e.metadata().map(|m| m.len()).unwrap_or(0), name })
                })
                .collect()
        })
        .unwrap_or_default();
    // Names hold the date and time, so they sort in time order.
    files.sort_by(|a, b| b.name.cmp(&a.name));
    files
}

/// Copies the database into `dir` now, then removes the oldest copies.
pub fn backup_now(conn: &Connection, dir: &Path) -> AppResult<PathBuf> {
    std::fs::create_dir_all(dir).map_err(|e| AppError::Internal(format!("can't create backup folder: {e}")))?;
    let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
    let path = dir.join(format!("{PREFIX}{stamp}.sqlite"));
    // VACUUM INTO writes a clean, consistent copy while the shop keeps working.
    conn.execute("VACUUM INTO ?1", [path.to_string_lossy()])?;
    for old in list(dir).into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(dir.join(old.name));
    }
    Ok(path)
}

/// True when there is no backup from today yet.
pub fn due(dir: &Path) -> bool {
    let today = format!("{PREFIX}{}", chrono::Local::now().format("%Y-%m-%d"));
    !list(dir).iter().any(|f| f.name.starts_with(&today))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_copies() {
        let dir = std::env::temp_dir().join(format!("pos-backup-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for day in 1..=16 {
            std::fs::write(dir.join(format!("{PREFIX}2026-01-{day:02}-000000.sqlite")), b"x").unwrap();
        }
        let conn = crate::db::open_in_memory().unwrap();
        crate::db::set_setting(&conn, "receipt_footer", "hello").unwrap();
        assert!(due(&dir));
        let made = backup_now(&conn, &dir).unwrap();
        assert!(!due(&dir));
        let files = list(&dir);
        assert_eq!(files.len(), KEEP);
        assert_eq!(dir.join(&files[0].name), made);
        let copy = Connection::open(&made).unwrap();
        assert_eq!(crate::db::get_setting(&copy, "receipt_footer").unwrap().as_deref(), Some("hello"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
