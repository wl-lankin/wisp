use std::path::{Path, PathBuf};

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, ToSql};
use uuid::Uuid;

use crate::fsutil::{create_private_dir, write_private_file};
use crate::item::{ClipItem, ClipPayload, ItemKind, Preview};
use crate::Result;

pub const HISTORY_CAP: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub id: Uuid,
    pub source: Uuid,
    pub kind: ItemKind,
    pub created_ms: i64,
    pub size: u64,
    pub preview: Preview,
}

#[derive(Debug, Clone)]
pub struct HistoryQuery {
    pub text: Option<String>,
    pub source: Option<Uuid>,
    pub kind: Option<ItemKind>,
    pub limit: usize,
}

impl Default for HistoryQuery {
    fn default() -> Self {
        Self { text: None, source: None, kind: None, limit: 50 }
    }
}

pub struct History {
    conn: Connection,
    blobs: PathBuf,
}

fn kind_str(k: ItemKind) -> &'static str {
    match k {
        ItemKind::Text => "text",
        ItemKind::Html => "html",
        ItemKind::Image => "image",
        ItemKind::File => "file",
    }
}

fn kind_from(s: &str) -> ItemKind {
    match s {
        "html" => ItemKind::Html,
        "image" => ItemKind::Image,
        "file" => ItemKind::File,
        _ => ItemKind::Text,
    }
}

fn search_text(p: &ClipPayload) -> String {
    match p {
        ClipPayload::Text(t) => t.clone(),
        ClipPayload::Html { plain, .. } => plain.clone(),
        ClipPayload::Image { .. } => String::new(),
    }
}

/// Every whitespace-separated token becomes a quoted prefix term, so user input can
/// never be parsed as FTS5 syntax.
pub fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|t| t.replace('"', "\"\""))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

impl History {
    pub fn open(dir: &Path) -> Result<Self> {
        let blobs = dir.join("blobs");
        create_private_dir(dir)?;
        create_private_dir(&blobs)?;
        let db_path = dir.join("history.sqlite");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if !db_path.exists() {
                write_private_file(&db_path, &[])?;
            }
            std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600))?;
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS items (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                kind TEXT NOT NULL,
                created_ms INTEGER NOT NULL,
                size INTEGER NOT NULL,
                preview BLOB NOT NULL
            );
            CREATE INDEX IF NOT EXISTS items_created ON items(created_ms);
            CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(id UNINDEXED, body);",
        )?;
        Ok(Self { conn, blobs })
    }

    fn blob_path(&self, id: Uuid) -> PathBuf {
        self.blobs.join(format!("{id}.bin"))
    }

    pub fn insert(&mut self, item: &ClipItem) -> Result<()> {
        let exists: bool = self
            .conn
            .query_row("SELECT 1 FROM items WHERE id = ?1", [item.id.to_string()], |_| Ok(true))
            .optional()?
            .unwrap_or(false);
        if exists {
            return Ok(());
        }
        let blob = self.blob_path(item.id);
        write_private_file(&blob, &postcard::to_allocvec(&item.payload)?)?;
        let result = self.insert_rows(item);
        if result.is_err() {
            let _ = std::fs::remove_file(&blob);
            return result;
        }
        self.prune()
    }

    fn insert_rows(&mut self, item: &ClipItem) -> Result<()> {
        let preview = postcard::to_allocvec(&item.payload.preview())?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO items (id, source, kind, created_ms, size, preview) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                item.id.to_string(),
                item.source.to_string(),
                kind_str(item.payload.kind()),
                item.created_ms,
                item.payload.size() as i64,
                preview
            ],
        )?;
        tx.execute(
            "INSERT INTO items_fts (id, body) VALUES (?1, ?2)",
            params![item.id.to_string(), search_text(&item.payload)],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn prune(&mut self) -> Result<()> {
        let stale: Vec<String> = {
            let mut stmt = self.conn.prepare(
                "SELECT id FROM items ORDER BY created_ms DESC, rowid DESC LIMIT -1 OFFSET ?1",
            )?;
            let rows = stmt.query_map([HISTORY_CAP as i64], |r| r.get::<_, String>(0))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        for id in stale {
            self.conn.execute("DELETE FROM items WHERE id = ?1", [&id])?;
            self.conn.execute("DELETE FROM items_fts WHERE id = ?1", [&id])?;
            let _ = std::fs::remove_file(self.blobs.join(format!("{id}.bin")));
        }
        Ok(())
    }

    pub fn load(&self, id: Uuid) -> Result<Option<ClipItem>> {
        let row = self
            .conn
            .query_row(
                "SELECT source, created_ms FROM items WHERE id = ?1",
                [id.to_string()],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?;
        let Some((source, created_ms)) = row else {
            return Ok(None);
        };
        let payload: ClipPayload = postcard::from_bytes(&std::fs::read(self.blob_path(id))?)?;
        Ok(Some(ClipItem {
            id,
            source: Uuid::parse_str(&source).unwrap_or_default(),
            created_ms,
            payload,
        }))
    }

    pub fn query(&self, q: &HistoryQuery) -> Result<Vec<HistoryEntry>> {
        let mut sql = String::from(
            "SELECT id, source, kind, created_ms, size, preview FROM items WHERE 1 = 1",
        );
        let mut args: Vec<Box<dyn ToSql>> = Vec::new();
        if let Some(fts) = q.text.as_deref().and_then(fts_query) {
            sql.push_str(" AND id IN (SELECT id FROM items_fts WHERE items_fts MATCH ?)");
            args.push(Box::new(fts));
        }
        if let Some(source) = q.source {
            sql.push_str(" AND source = ?");
            args.push(Box::new(source.to_string()));
        }
        if let Some(kind) = q.kind {
            sql.push_str(" AND kind = ?");
            args.push(Box::new(kind_str(kind)));
        }
        sql.push_str(" ORDER BY created_ms DESC, rowid DESC LIMIT ?");
        args.push(Box::new(q.limit as i64));

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Vec<u8>>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, source, kind, created_ms, size, preview) = row?;
            out.push(HistoryEntry {
                id: Uuid::parse_str(&id).unwrap_or_default(),
                source: Uuid::parse_str(&source).unwrap_or_default(),
                kind: kind_from(&kind),
                created_ms,
                size: size as u64,
                preview: postcard::from_bytes(&preview)?,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(source: Uuid, text: &str) -> ClipItem {
        ClipItem::new(source, ClipPayload::Text(text.into()))
    }

    #[cfg(unix)]
    #[test]
    fn history_files_are_private_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut h = History::open(dir.path()).unwrap();
        let it = item(Uuid::new_v4(), "secret");
        h.insert(&it).unwrap();
        assert_eq!(mode(dir.path()), 0o700);
        assert_eq!(mode(&dir.path().join("blobs")), 0o700);
        assert_eq!(mode(&dir.path().join("history.sqlite")), 0o600);
        assert_eq!(mode(&dir.path().join("blobs").join(format!("{}.bin", it.id))), 0o600);
    }

    #[test]
    fn insert_load_round_trip_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let src = Uuid::new_v4();
        let img = ClipItem::new(src, ClipPayload::Image { png: vec![1, 2, 3], width: 1, height: 1 });
        {
            let mut h = History::open(dir.path()).unwrap();
            h.insert(&img).unwrap();
            h.insert(&img).unwrap();
        }
        let h = History::open(dir.path()).unwrap();
        assert_eq!(h.load(img.id).unwrap(), Some(img.clone()));
        assert_eq!(h.query(&HistoryQuery::default()).unwrap().len(), 1);
    }

    #[test]
    fn text_search_matches_word_prefixes() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::open(dir.path()).unwrap();
        let src = Uuid::new_v4();
        h.insert(&item(src, "hello world")).unwrap();
        h.insert(&item(src, "goodbye")).unwrap();
        let q = HistoryQuery { text: Some("wor".into()), ..Default::default() };
        let found = h.query(&q).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].preview, Preview::Text("hello world".into()));
    }

    #[test]
    fn hostile_search_text_does_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::open(dir.path()).unwrap();
        h.insert(&item(Uuid::new_v4(), "a \"quoted\" text")).unwrap();
        for text in ["\"", "a\" OR \"b", "NEAR(", "*", "-", "   "] {
            let q = HistoryQuery { text: Some(text.into()), ..Default::default() };
            assert!(h.query(&q).is_ok(), "query {text:?} failed");
        }
        let q = HistoryQuery { text: Some("\"quoted".into()), ..Default::default() };
        assert_eq!(h.query(&q).unwrap().len(), 1);
    }

    #[test]
    fn filters_by_source_and_kind_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::open(dir.path()).unwrap();
        let (mac, pc) = (Uuid::new_v4(), Uuid::new_v4());
        let first = item(mac, "one");
        h.insert(&first).unwrap();
        let second = item(mac, "two");
        h.insert(&second).unwrap();
        h.insert(&item(pc, "three")).unwrap();
        let q = HistoryQuery { source: Some(mac), kind: Some(ItemKind::Text), ..Default::default() };
        let ids: Vec<Uuid> = h.query(&q).unwrap().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![second.id, first.id]);
    }

    #[test]
    fn prune_keeps_newest_200_and_deletes_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::open(dir.path()).unwrap();
        let src = Uuid::new_v4();
        let oldest = item(src, "oldest");
        h.insert(&oldest).unwrap();
        for i in 0..HISTORY_CAP {
            h.insert(&item(src, &format!("n{i}"))).unwrap();
        }
        let all = h.query(&HistoryQuery { limit: 1000, ..Default::default() }).unwrap();
        assert_eq!(all.len(), HISTORY_CAP);
        assert_eq!(h.load(oldest.id).unwrap(), None);
        assert!(!dir.path().join("blobs").join(format!("{}.bin", oldest.id)).exists());
    }
}
