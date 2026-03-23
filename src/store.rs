//! SQLite session and message persistence.
//!
//! Stores conversation history so sessions survive restarts.
//! Uses a simplified snapshot approach: the full message list is stored
//! as a JSON blob per session, updated on task completion.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use rusqlite::Connection;
use tokio::sync::Mutex;

use crate::api::Message;

/// Thread-safe wrapper around a SQLite connection.
#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

/// Session metadata returned by list_sessions.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub directory: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Store {
    /// Open or create the store at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory: {}", parent.display()))?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("failed to open database: {}", path.display()))?;

        // Set pragmas for concurrent access and performance
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;",
        )
        .context("failed to set database pragmas")?;

        // Create tables
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                 id TEXT PRIMARY KEY,
                 title TEXT NOT NULL DEFAULT '',
                 directory TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS messages (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                 data TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );",
        )
        .context("failed to create tables")?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Default store path: ~/.local/share/hob/store.db
    pub fn default_path() -> PathBuf {
        let data_dir = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".local/share")
            });
        data_dir.join("hob/store.db")
    }

    /// Create a new session.
    pub async fn create_session(&self, id: &str, directory: &str) -> Result<()> {
        let conn = self.conn.lock().await;
        let now = now_unix();
        conn.execute(
            "INSERT INTO sessions (id, directory, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, directory, now, now],
        )
        .context("failed to create session")?;
        Ok(())
    }

    /// Save the full message history for a session.
    pub async fn save_messages(&self, session_id: &str, messages: &[Message]) -> Result<()> {
        let conn = self.conn.lock().await;
        let data =
            serde_json::to_string(messages).context("failed to serialize messages")?;
        let now = now_unix();

        // Delete old message snapshot and insert new one
        conn.execute(
            "DELETE FROM messages WHERE session_id = ?1",
            rusqlite::params![session_id],
        )?;
        conn.execute(
            "INSERT INTO messages (session_id, data, created_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![session_id, data, now],
        )?;
        conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, session_id],
        )?;

        Ok(())
    }

    /// Load messages for a session, if any exist.
    pub async fn load_messages(&self, session_id: &str) -> Result<Option<Vec<Message>>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare("SELECT data FROM messages WHERE session_id = ?1 ORDER BY id DESC LIMIT 1")?;
        let result = stmt.query_row(rusqlite::params![session_id], |row| {
            row.get::<_, String>(0)
        });

        match result {
            Ok(data) => {
                let messages: Vec<Message> =
                    serde_json::from_str(&data).context("failed to deserialize messages")?;
                Ok(Some(messages))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// List all sessions, newest first.
    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let conn = self.conn.lock().await;
        let mut stmt =
            conn.prepare("SELECT id, title, directory, created_at, updated_at FROM sessions ORDER BY updated_at DESC")?;
        let sessions = stmt
            .query_map([], |row| {
                Ok(SessionInfo {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    directory: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(sessions)
    }

    /// Update session title.
    pub async fn update_title(&self, id: &str, title: &str) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE sessions SET title = ?1 WHERE id = ?2",
            rusqlite::params![title, id],
        )?;
        Ok(())
    }

    /// Delete a session and its messages.
    pub async fn delete_session(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute("DELETE FROM sessions WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ContentBlock;
    use tempfile::TempDir;

    async fn test_store() -> (Store, TempDir) {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("test.db")).unwrap();
        (store, dir)
    }

    #[tokio::test]
    async fn test_create_and_list_sessions() {
        let (store, _dir) = test_store().await;
        store.create_session("s1", "/tmp/a").await.unwrap();
        store.create_session("s2", "/tmp/b").await.unwrap();
        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_save_and_load_messages() {
        let (store, _dir) = test_store().await;
        store.create_session("s1", "/tmp").await.unwrap();

        let msgs = vec![Message::User {
            content: vec![ContentBlock::Text {
                text: "hello".into(),
            }],
        }];
        store.save_messages("s1", &msgs).await.unwrap();

        let loaded = store.load_messages("s1").await.unwrap().unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[tokio::test]
    async fn test_load_nonexistent_session() {
        let (store, _dir) = test_store().await;
        let result = store.load_messages("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_delete_session() {
        let (store, _dir) = test_store().await;
        store.create_session("s1", "/tmp").await.unwrap();
        store.delete_session("s1").await.unwrap();
        let sessions = store.list_sessions().await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_wal_mode() {
        let (store, _dir) = test_store().await;
        let conn = store.conn.lock().await;
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[tokio::test]
    async fn test_store_creates_directory() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("subdir/nested/test.db");
        let store = Store::open(&path);
        assert!(store.is_ok());
    }
}
