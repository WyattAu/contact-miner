//! SQLite lead store — thin, sync, no schema opinions beyond contacts.

use rusqlite::Connection;
use std::sync::Mutex;

pub struct LeadStore {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Lead {
    pub email: String,
    pub name: Option<String>,
    pub website: Option<String>,
    pub phone: Option<String>,
    pub whatsapp: Option<String>,
    pub telegram: Option<String>,
    pub signal: Option<String>,
    pub source: Option<String>,
}

impl LeadStore {
    pub fn open(path: &str) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            r#"CREATE TABLE IF NOT EXISTS leads (
                email TEXT PRIMARY KEY,
                name TEXT,
                website TEXT,
                phone TEXT,
                whatsapp TEXT,
                telegram TEXT,
                signal TEXT,
                source TEXT,
                first_seen DATETIME DEFAULT CURRENT_TIMESTAMP,
                last_seen DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX IF NOT EXISTS idx_leads_source ON leads(source);
            "#,
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Insert or enrich. Never overwrites existing channel data —
    /// COALESCE keeps the first non-null value for each field.
    pub fn upsert(&self, lead: &Lead) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"INSERT INTO leads (email, name, website, phone, whatsapp, telegram, signal, source)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(email) DO UPDATE SET
                name = COALESCE(excluded.name, name),
                website = COALESCE(excluded.website, website),
                phone = COALESCE(excluded.phone, phone),
                whatsapp = COALESCE(excluded.whatsapp, whatsapp),
                telegram = COALESCE(excluded.telegram, telegram),
                signal = COALESCE(excluded.signal, signal),
                source = COALESCE(excluded.source, source),
                last_seen = CURRENT_TIMESTAMP"#,
            rusqlite::params![
                lead.email, lead.name, lead.website, lead.phone,
                lead.whatsapp, lead.telegram, lead.signal, lead.source,
            ],
        )?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM leads", [], |r| r.get(0))
    }

    pub fn export_json(&self) -> Result<String, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT email, name, website, phone, whatsapp, telegram, signal, source FROM leads",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Lead {
                email: r.get(0)?,
                name: r.get(1)?,
                website: r.get(2)?,
                phone: r.get(3)?,
                whatsapp: r.get(4)?,
                telegram: r.get(5)?,
                signal: r.get(6)?,
                source: r.get(7)?,
            })
        })?;
        let leads: Vec<Lead> = rows.filter_map(|r| r.ok()).collect();
        serde_json::to_string_pretty(&leads)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
    }
}
