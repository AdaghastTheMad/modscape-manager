//! SQLite storage owned by the core: packs and the API response cache.

use crate::error::{AppError, AppResult};
use crate::model::Pack;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSummary {
    pub id: String,
    pub name: String,
    pub mc_version: String,
    pub loader: String,
    pub entry_count: u32,
    pub updated_at: String,
    pub icon: Option<String>,
}

pub struct CacheHit {
    pub fetched_at: i64,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub entries: u64,
    pub bytes: u64,
}

/// Current time as RFC 3339 UTC (seconds precision), without a date crate.
pub fn now_iso() -> String {
    iso_from_secs(now_secs())
}

pub fn iso_from_secs(t: i64) -> String {
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    // Civil from days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs % 3600 / 60, secs % 60)
}

pub fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Storage(e.to_string())
}

const MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE packs (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        mc_version TEXT NOT NULL,
        loader TEXT NOT NULL,
        entry_count INTEGER NOT NULL DEFAULT 0,
        updated_at TEXT NOT NULL,
        json TEXT NOT NULL
     );
     CREATE TABLE api_cache (
        key TEXT PRIMARY KEY,
        fetched_at INTEGER NOT NULL,
        content_type TEXT,
        body BLOB NOT NULL
     );",
    // v2: payloads (M8) and a small key/value table for one-time setup flags.
    "CREATE TABLE payloads (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        json TEXT NOT NULL
     );
     CREATE TABLE meta (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
     );",
];

impl Db {
    pub fn open(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path).map_err(db_err)?;
        Self::init(conn)
    }

    pub fn memory() -> Self {
        Self::init(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn init(conn: Connection) -> AppResult<Self> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;").map_err(db_err)?;
        let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(db_err)?;
        if current as usize > MIGRATIONS.len() {
            return Err(AppError::Storage(
                "The app's database was created by a newer version of Modpack Planner. Update the app to continue.".into(),
            ));
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            conn.execute_batch(&format!("BEGIN; {sql}; PRAGMA user_version = {}; COMMIT;", i + 1)).map_err(db_err)?;
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> AppResult<T> {
        let conn = self.conn.lock().map_err(|_| AppError::Storage("database lock poisoned".into()))?;
        f(&conn).map_err(db_err)
    }

    // ---------------- packs ----------------
    pub fn put_pack(&self, pack: &Pack) -> AppResult<()> {
        let json = serde_json::to_string(pack).map_err(|e| AppError::Invalid(e.to_string()))?;
        let loader = serde_json::to_value(pack.loader).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
        self.with(|c| {
            c.execute(
                "INSERT INTO packs (id, name, mc_version, loader, entry_count, updated_at, json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET name=?2, mc_version=?3, loader=?4, entry_count=?5, updated_at=?6, json=?7",
                params![pack.id, pack.name, pack.mc_version, loader, pack.entries.len() as i64, pack.updated_at, json],
            )
            .map(|_| ())
        })
    }

    pub fn get_pack(&self, id: &str) -> AppResult<Option<Pack>> {
        let json: Option<String> =
            self.with(|c| c.query_row("SELECT json FROM packs WHERE id = ?1", [id], |r| r.get(0)).optional())?;
        json.map(|j| serde_json::from_str(&j).map_err(|e| AppError::Storage(format!("Saved pack is unreadable: {e}"))))
            .transpose()
    }

    pub fn pack_exists(&self, id: &str) -> AppResult<bool> {
        self.with(|c| c.query_row("SELECT 1 FROM packs WHERE id = ?1", [id], |_| Ok(())).optional().map(|o| o.is_some()))
    }

    pub fn list_packs(&self) -> AppResult<Vec<PackSummary>> {
        let rows = self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, name, mc_version, loader, entry_count, updated_at, json_extract(json, '$.icon')
                 FROM packs ORDER BY updated_at DESC",
            )?;
            let rows = st
                .query_map([], |r| {
                    Ok(PackSummary {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        mc_version: r.get(2)?,
                        loader: r.get(3)?,
                        entry_count: r.get::<_, i64>(4)? as u32,
                        updated_at: r.get(5)?,
                        icon: r.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;
        Ok(rows)
    }

    pub fn delete_pack(&self, id: &str) -> AppResult<()> {
        self.with(|c| c.execute("DELETE FROM packs WHERE id = ?1", [id]).map(|_| ()))
    }

    // ---------------- payloads ----------------
    pub fn put_payload(&self, p: &crate::payload::Payload) -> AppResult<()> {
        let json = serde_json::to_string(p).map_err(|e| AppError::Invalid(e.to_string()))?;
        self.with(|c| {
            c.execute(
                "INSERT INTO payloads (id, name, updated_at, json) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET name=?2, updated_at=?3, json=?4",
                params![p.id, p.name, p.updated_at, json],
            )
            .map(|_| ())
        })
    }

    pub fn get_payload(&self, id: &str) -> AppResult<Option<crate::payload::Payload>> {
        let json: Option<String> = self.with(|c| c.query_row("SELECT json FROM payloads WHERE id = ?1", [id], |r| r.get(0)).optional())?;
        json.map(|j| serde_json::from_str(&j).map_err(|e| AppError::Storage(format!("Saved payload is unreadable: {e}")))).transpose()
    }

    pub fn all_payloads(&self) -> AppResult<Vec<crate::payload::Payload>> {
        let rows: Vec<String> = self.with(|c| {
            let mut st = c.prepare("SELECT json FROM payloads ORDER BY name COLLATE NOCASE")?;
            let r = st.query_map([], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
            Ok(r)
        })?;
        Ok(rows.into_iter().filter_map(|j| serde_json::from_str(&j).ok()).collect())
    }

    pub fn delete_payload(&self, id: &str) -> AppResult<()> {
        self.with(|c| c.execute("DELETE FROM payloads WHERE id = ?1", [id]).map(|_| ()))
    }

    pub fn meta_get(&self, key: &str) -> AppResult<Option<String>> {
        self.with(|c| c.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional())
    }

    /// Cross-site links you confirmed yourself: (Modrinth id, CurseForge id).
    pub fn manual_links(&self) -> AppResult<Vec<(String, u64)>> {
        Ok(self.meta_get("manual_links")?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default())
    }
    /// Remembers a link; a project has at most one partner, so older pairs with either id go.
    pub fn remember_link(&self, modrinth_id: &str, curseforge_id: u64) -> AppResult<()> {
        let mut l = self.manual_links()?;
        l.retain(|(m, c)| m != modrinth_id && *c != curseforge_id);
        l.push((modrinth_id.to_string(), curseforge_id));
        self.meta_set("manual_links", &serde_json::to_string(&l).unwrap())
    }
    pub fn forget_link(&self, modrinth_id: &str, curseforge_id: u64) -> AppResult<()> {
        let mut l = self.manual_links()?;
        l.retain(|(m, c)| !(m == modrinth_id && *c == curseforge_id));
        self.meta_set("manual_links", &serde_json::to_string(&l).unwrap())
    }

    pub fn meta_set(&self, key: &str, value: &str) -> AppResult<()> {
        self.with(|c| c.execute("INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=?2", params![key, value]).map(|_| ()))
    }

    // ---------------- API cache ----------------
    /// Returns a cached body if younger than `max_age_secs` (or any age when None).
    pub fn cache_get(&self, key: &str, max_age_secs: Option<i64>) -> AppResult<Option<CacheHit>> {
        let hit = self.with(|c| {
            c.query_row("SELECT fetched_at, body, content_type FROM api_cache WHERE key = ?1", [key], |r| {
                Ok(CacheHit { fetched_at: r.get(0)?, body: r.get(1)?, content_type: r.get(2)? })
            })
            .optional()
        })?;
        Ok(hit.filter(|h| max_age_secs.map_or(true, |m| now_secs() - h.fetched_at <= m)))
    }

    pub fn cache_put(&self, key: &str, body: &[u8], content_type: Option<&str>) -> AppResult<i64> {
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "INSERT INTO api_cache (key, fetched_at, content_type, body) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(key) DO UPDATE SET fetched_at=?2, content_type=?3, body=?4",
                params![key, now, content_type, body],
            )
            .map(|_| ())
        })?;
        Ok(now)
    }

    pub fn cache_stats(&self) -> AppResult<CacheStats> {
        self.with(|c| {
            c.query_row("SELECT COUNT(*), COALESCE(SUM(LENGTH(body)), 0) FROM api_cache", [], |r| {
                Ok(CacheStats { entries: r.get::<_, i64>(0)? as u64, bytes: r.get::<_, i64>(1)? as u64 })
            })
        })
    }

    pub fn cache_clear(&self) -> AppResult<()> {
        self.with(|c| c.execute_batch("DELETE FROM api_cache; VACUUM;"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{entry, pack};

    #[test]
    fn pack_crud() {
        let db = Db::memory();
        let mut p = pack();
        p.entries.insert("a".into(), entry(Some("AANobbMI"), None, "Sodium", "1"));
        db.put_pack(&p).unwrap();
        assert!(db.pack_exists("p1").unwrap());
        let list = db.list_packs().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].entry_count, 1);
        assert_eq!(list[0].loader, "neoforge");
        assert_eq!(db.get_pack("p1").unwrap().unwrap(), p);
        p.name = "Renamed".into();
        db.put_pack(&p).unwrap();
        assert_eq!(db.list_packs().unwrap()[0].name, "Renamed");
        db.delete_pack("p1").unwrap();
        assert!(db.get_pack("p1").unwrap().is_none());
    }

    #[test]
    fn manual_links_replace_older_pairs() {
        let db = Db::memory();
        db.remember_link("mrA", 1).unwrap();
        db.remember_link("mrB", 2).unwrap();
        db.remember_link("mrA", 3).unwrap(); // re-linked: the old pair goes
        assert_eq!(db.manual_links().unwrap(), vec![("mrB".to_string(), 2), ("mrA".to_string(), 3)]);
        db.forget_link("mrB", 2).unwrap();
        assert_eq!(db.manual_links().unwrap(), vec![("mrA".to_string(), 3)]);
    }

    #[test]
    fn iso_dates() {
        assert_eq!(iso_from_secs(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_from_secs(1_795_478_400), "2026-11-24T00:00:00Z");
        assert_eq!(iso_from_secs(951_782_400 + 3661), "2000-02-29T01:01:01Z");
    }

    #[test]
    fn cache_ttl() {
        let db = Db::memory();
        db.cache_put("GET x", b"hello", Some("application/json")).unwrap();
        assert_eq!(db.cache_get("GET x", Some(60)).unwrap().unwrap().body, b"hello");
        // Backdate the entry: stale for a 60 s TTL, still available with no TTL.
        db.with(|c| c.execute("UPDATE api_cache SET fetched_at = fetched_at - 120", [])).unwrap();
        assert!(db.cache_get("GET x", Some(60)).unwrap().is_none());
        assert!(db.cache_get("GET x", None).unwrap().is_some());
        assert_eq!(db.cache_stats().unwrap().entries, 1);
        db.cache_clear().unwrap();
        assert_eq!(db.cache_stats().unwrap().entries, 0);
    }
}
