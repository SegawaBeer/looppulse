use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const APP_DIR: &str = "looppulse";
const LEGACY_APP_DIR: &str = "observer";
const DB_FILE: &str = "events.sqlite3";
const MAX_EVENTS: i64 = 500;
const SECONDS_PER_DAY: i64 = 86_400;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEvent {
    pub id: String,
    pub session_id: String,
    pub project_name: String,
    pub agent_type: String,
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub message: String,
    pub created_at: i64,
}

pub fn load_recent_events(limit: u32) -> Result<Vec<SessionEvent>, String> {
    let conn = open_connection()?;
    let limit = limit.clamp(1, MAX_EVENTS as u32) as i64;
    let mut statement = conn
        .prepare(
            "SELECT id, session_id, project_name, agent_type, kind, severity, title, message, created_at
             FROM session_events
             ORDER BY created_at DESC, rowid DESC
             LIMIT ?1",
        )
        .map_err(|error| error.to_string())?;

    let rows = statement
        .query_map(params![limit], |row| {
            Ok(SessionEvent {
                id: row.get(0)?,
                session_id: row.get(1)?,
                project_name: row.get(2)?,
                agent_type: row.get(3)?,
                kind: row.get(4)?,
                severity: row.get(5)?,
                title: row.get(6)?,
                message: row.get(7)?,
                created_at: row.get(8)?,
            })
        })
        .map_err(|error| error.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

pub fn append_events(
    events: Vec<SessionEvent>,
    retention_days: u64,
) -> Result<Vec<SessionEvent>, String> {
    if events.is_empty() {
        return load_recent_events(200);
    }

    let mut conn = open_connection()?;
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    {
        let mut insert = tx
            .prepare(
                "INSERT OR IGNORE INTO session_events
                 (id, session_id, project_name, agent_type, kind, severity, title, message, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .map_err(|error| error.to_string())?;

        for event in events {
            insert
                .execute(params![
                    event.id,
                    event.session_id,
                    event.project_name,
                    event.agent_type,
                    event.kind,
                    event.severity,
                    event.title,
                    event.message,
                    event.created_at,
                ])
                .map_err(|error| error.to_string())?;
        }
    }
    prune_old_events_tx(&tx, retention_days)?;
    tx.execute(
        "DELETE FROM session_events
         WHERE rowid NOT IN (
           SELECT rowid FROM session_events
           ORDER BY created_at DESC, rowid DESC
           LIMIT ?1
         )",
        params![MAX_EVENTS],
    )
    .map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())?;

    load_recent_events(200)
}

pub fn clear_events() -> Result<(), String> {
    let conn = open_connection()?;
    conn.execute("DELETE FROM session_events", [])
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn prune_old_events_tx(tx: &rusqlite::Transaction<'_>, retention_days: u64) -> Result<(), String> {
    let cutoff = current_timestamp() - retention_days.clamp(1, 365) as i64 * SECONDS_PER_DAY;
    tx.execute(
        "DELETE FROM session_events WHERE created_at < ?1",
        params![cutoff],
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn open_connection() -> Result<Connection, String> {
    let path = db_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    // 新库不存在但旧 observer 库存在时，一次性迁移历史，保住已安装用户的事件记录。
    if !path.exists() {
        if let Some(legacy) = legacy_db_path() {
            if legacy.exists() {
                let _ = std::fs::copy(&legacy, &path);
            }
        }
    }

    let conn = Connection::open(path).map_err(|error| error.to_string())?;
    init_schema(&conn)?;
    Ok(conn)
}

fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS session_events (
           id TEXT PRIMARY KEY,
           session_id TEXT NOT NULL,
           project_name TEXT NOT NULL,
           agent_type TEXT NOT NULL,
           kind TEXT NOT NULL,
           severity TEXT NOT NULL,
           title TEXT NOT NULL,
           message TEXT NOT NULL,
           created_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_session_events_created_at
           ON session_events(created_at DESC);
         CREATE INDEX IF NOT EXISTS idx_session_events_session_id
           ON session_events(session_id);",
    )
    .map_err(|error| error.to_string())
}

fn db_path() -> Result<PathBuf, String> {
    dirs::data_local_dir()
        .map(|dir| dir.join(APP_DIR).join(DB_FILE))
        .ok_or_else(|| "cannot resolve local data directory".to_string())
}

fn legacy_db_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join(LEGACY_APP_DIR).join(DB_FILE))
}

fn current_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_uses_frontend_field_names() {
        let event = sample_event("event-1", 1);
        let raw = serde_json::to_string(&event).unwrap();

        assert!(raw.contains("sessionId"));
        assert!(raw.contains("projectName"));
        assert!(raw.contains("createdAt"));
        assert!(!raw.contains("session_id"));
    }

    fn sample_event(id: &str, created_at: i64) -> SessionEvent {
        SessionEvent {
            id: id.to_string(),
            session_id: "session".to_string(),
            project_name: "project".to_string(),
            agent_type: "Codex".to_string(),
            kind: "status_changed".to_string(),
            severity: "info".to_string(),
            title: "状态变化".to_string(),
            message: "thinking -> waiting".to_string(),
            created_at,
        }
    }
}
