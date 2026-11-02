//! Only the daemon opens the database. Data, pins, ordering and eviction commit together.
use crate::Result;
use image::{ImageFormat, ImageReader, Limits};
use rusqlite::{Connection, Transaction, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Cursor,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const MAX_TEXT: usize = 1024 * 1024;
pub const MAX_IMAGE: usize = 16 * 1024 * 1024;
const MAX_PIXELS: u64 = 24 * 1024 * 1024;
pub const PAGE_SIZE: usize = 12;

#[derive(Clone, Copy)]
pub struct Budget {
    pub recent_count: usize,
    pub recent_bytes: usize,
    pub pinned_count: usize,
    pub pinned_bytes: usize,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            recent_count: 100,
            recent_bytes: 64 * 1024 * 1024,
            pinned_count: 100,
            pinned_bytes: 128 * 1024 * 1024,
        }
    }
}
#[derive(Debug)]
pub struct Payload {
    pub mime: String,
    pub bytes: Vec<u8>,
    pub hash: String,
    pub preview: String,
    pub width: u32,
    pub height: u32,
    pub thumb_width: u32,
    pub thumb_height: u32,
    pub thumbnail: Vec<u8>,
}
impl Payload {
    pub fn parse(mime: &str, bytes: Vec<u8>) -> Result<Self> {
        if bytes.is_empty() {
            return Err("빈 클립보드 내용입니다".into());
        }
        if mime.starts_with("text/plain") || mime == "UTF8_STRING" {
            if bytes.len() > MAX_TEXT {
                return Err("텍스트는 항목당 1 MiB까지 기록합니다".into());
            }
            let text = std::str::from_utf8(&bytes)?;
            if text.contains('\0') {
                return Err("텍스트에 NUL 문자가 있습니다".into());
            }
            let hash = format!("t{:x}", Sha256::digest(&bytes));
            let preview = text.chars().take(240).collect();
            return Ok(Self {
                mime: "text/plain;charset=utf-8".into(),
                bytes,
                hash,
                preview,
                width: 0,
                height: 0,
                thumb_width: 0,
                thumb_height: 0,
                thumbnail: vec![],
            });
        }
        if bytes.len() > MAX_IMAGE {
            return Err("이미지는 항목당 16 MiB까지 기록합니다".into());
        }
        let format = match mime {
            "image/png" => ImageFormat::Png,
            "image/jpeg" => ImageFormat::Jpeg,
            _ => return Err("지원하지 않는 클립보드 형식입니다".into()),
        };
        let (width, height) =
            ImageReader::with_format(Cursor::new(&bytes), format).into_dimensions()?;
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
            return Err("이미지는 24메가픽셀까지 기록합니다".into());
        }
        let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
        let mut limits = Limits::default();
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode()?;
        let rgba = image.to_rgba8();
        let mut digest = Sha256::new();
        digest.update(width.to_le_bytes());
        digest.update(height.to_le_bytes());
        digest.update(rgba.as_raw());
        let hash = format!("i{:x}", digest.finalize());
        drop(rgba);
        let thumb = image.thumbnail(width.min(128), height.min(80)).to_rgba8();
        Ok(Self {
            mime: mime.into(),
            bytes,
            hash,
            preview: format!("이미지 {width} × {height}"),
            width,
            height,
            thumb_width: thumb.width(),
            thumb_height: thumb.height(),
            thumbnail: thumb.into_raw(),
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: i64,
    pub mime: String,
    pub preview: String,
    pub width: u32,
    pub height: u32,
    pub size: u64,
    pub pinned: bool,
    pub created: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub total: usize,
    pub recent: usize,
    pub pinned: usize,
    pub revision: u64,
}
pub struct Store {
    connection: Connection,
    budget: Budget,
    revision: u64,
}

impl Store {
    pub fn open(directory: &Path) -> Result<Self> {
        Self::open_with_budget(directory, Budget::default())
    }
    fn open_with_budget(directory: &Path, budget: Budget) -> Result<Self> {
        if fs::symlink_metadata(directory).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("기록 디렉터리 심볼릭 링크는 사용하지 않습니다".into());
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)?;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        let path = directory.join("history.sqlite3");
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("기록 파일 심볼릭 링크는 사용하지 않습니다".into());
        }
        let existed = path.exists();
        if !existed {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
        }
        let connection = Connection::open(&path)?;
        connection.busy_timeout(Duration::from_secs(2))?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if existed && version != 1 {
            return Err("알 수 없는 기록 형식입니다. 원본을 보존했습니다".into());
        }
        if !existed {
            connection.execute_batch("BEGIN; CREATE TABLE entries(id INTEGER PRIMARY KEY AUTOINCREMENT, hash TEXT NOT NULL UNIQUE, mime TEXT NOT NULL, data BLOB NOT NULL, preview TEXT NOT NULL, width INTEGER NOT NULL, height INTEGER NOT NULL, thumbnail BLOB NOT NULL, thumb_width INTEGER NOT NULL, thumb_height INTEGER NOT NULL, pinned INTEGER NOT NULL DEFAULT 0, created INTEGER NOT NULL, used INTEGER NOT NULL); CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL); INSERT INTO meta VALUES('last_hash',''); PRAGMA user_version=1; COMMIT;")?;
        }
        let integrity: String = connection.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if integrity != "ok" {
            return Err("클립보드 기록 파일 오류 · 원본 보존".into());
        }
        // Validate the schema before changing journal settings or accepting input.
        connection.prepare("SELECT id,hash,mime,data,preview,width,height,thumbnail,thumb_width,thumb_height,pinned,created,used FROM entries LIMIT 0")?;
        connection.query_row("SELECT value FROM meta WHERE key='last_hash'", [], |r| {
            r.get::<_, String>(0)
        })?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA secure_delete=ON;",
        )?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            connection,
            budget,
            revision: 1,
        })
    }
    pub fn ingest(&mut self, payload: &Payload) -> Result<Option<i64>> {
        self.ingest_current(payload, false)
    }
    pub fn ingest_current(&mut self, payload: &Payload, restored: bool) -> Result<Option<i64>> {
        let tx = self.connection.transaction()?;
        let last: String =
            tx.query_row("SELECT value FROM meta WHERE key='last_hash'", [], |r| {
                r.get(0)
            })?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE hash=?1)",
            [&payload.hash],
            |r| r.get(0),
        )?;
        if last == payload.hash && (restored || exists) {
            return Ok(None);
        }
        let used = next_used(&tx)?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        tx.execute("INSERT INTO entries(hash,mime,data,preview,width,height,thumbnail,thumb_width,thumb_height,created,used) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(hash) DO UPDATE SET used=excluded.used",params![payload.hash,payload.mime,payload.bytes,payload.preview,payload.width,payload.height,payload.thumbnail,payload.thumb_width,payload.thumb_height,now,used])?;
        let id = tx.query_row(
            "SELECT id FROM entries WHERE hash=?1",
            [&payload.hash],
            |r| r.get(0),
        )?;
        tx.execute(
            "UPDATE meta SET value=?1 WHERE key='last_hash'",
            [&payload.hash],
        )?;
        prune(&tx, self.budget)?;
        tx.commit()?;
        self.revision += 1;
        Ok(Some(id))
    }
    pub fn list(&self, query: &str, filter: &str, page: usize) -> Result<Listing> {
        if query.len() > 1024 || page > 100 {
            return Err("검색 범위를 초과했습니다".into());
        }
        let clause = match filter {
            "all" => "1",
            "text" => "mime LIKE 'text/%'",
            "image" => "mime LIKE 'image/%'",
            "pinned" => "pinned=1",
            _ => return Err("잘못된 기록 필터입니다".into()),
        };
        let predicate = format!(
            "({clause}) AND (?1='' OR (mime LIKE 'text/%' AND instr(lower(CAST(data AS TEXT)),lower(?1))>0))"
        );
        let total = self.connection.query_row(
            &format!("SELECT COUNT(*) FROM entries WHERE {predicate}"),
            [query],
            |r| r.get(0),
        )?;
        let mut stmt=self.connection.prepare(&format!("SELECT id,mime,preview,width,height,length(data),pinned,created FROM entries WHERE {predicate} ORDER BY pinned DESC,used DESC,id DESC LIMIT ?2 OFFSET ?3"))?;
        let entries = stmt
            .query_map(params![query, PAGE_SIZE, page * PAGE_SIZE], |r| {
                Ok(Entry {
                    id: r.get(0)?,
                    mime: r.get(1)?,
                    preview: r.get(2)?,
                    width: r.get(3)?,
                    height: r.get(4)?,
                    size: r.get(5)?,
                    pinned: r.get(6)?,
                    created: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let recent =
            self.connection
                .query_row("SELECT COUNT(*) FROM entries WHERE pinned=0", [], |r| {
                    r.get(0)
                })?;
        let pinned =
            self.connection
                .query_row("SELECT COUNT(*) FROM entries WHERE pinned=1", [], |r| {
                    r.get(0)
                })?;
        Ok(Listing {
            entries,
            total,
            recent,
            pinned,
            revision: self.revision,
        })
    }
    pub fn pin(&mut self, id: i64, pinned: bool) -> Result<()> {
        let tx = self.connection.transaction()?;
        let present: bool =
            tx.query_row("SELECT pinned FROM entries WHERE id=?1", [id], |r| r.get(0))?;
        if pinned && !present {
            let (count, bytes): (usize, usize) = tx.query_row(
                "SELECT COUNT(*),coalesce(sum(length(data)),0) FROM entries WHERE pinned=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let size: usize =
                tx.query_row("SELECT length(data) FROM entries WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
            if count >= self.budget.pinned_count || bytes + size > self.budget.pinned_bytes {
                return Err(
                    "고정 한도(100개 · 128 MiB)에 도달했습니다. 기존 고정을 해제해 주세요".into(),
                );
            }
        }
        tx.execute(
            "UPDATE entries SET pinned=?2 WHERE id=?1",
            params![id, pinned],
        )?;
        prune(&tx, self.budget)?;
        tx.commit()?;
        self.revision += 1;
        Ok(())
    }
    pub fn delete(&mut self, id: i64) -> Result<()> {
        let changed = self
            .connection
            .execute("DELETE FROM entries WHERE id=?1 AND pinned=0", [id])?;
        if changed == 0 {
            return Err("고정 항목은 고정을 해제한 뒤 삭제하세요".into());
        }
        self.revision += 1;
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }
    pub fn clear_recent(&mut self) -> Result<()> {
        self.connection
            .execute("DELETE FROM entries WHERE pinned=0", [])?;
        self.revision += 1;
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }
    pub fn data(&self, id: i64) -> Result<(String, Vec<u8>)> {
        Ok(self
            .connection
            .query_row("SELECT mime,data FROM entries WHERE id=?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?)
    }
    pub fn thumbnail(&self, id: i64) -> Result<(u32, u32, Vec<u8>)> {
        Ok(self.connection.query_row(
            "SELECT thumb_width,thumb_height,thumbnail FROM entries WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?)
    }
    pub fn touch(&mut self, id: i64) -> Result<()> {
        let tx = self.connection.transaction()?;
        let used = next_used(&tx)?;
        tx.execute("UPDATE entries SET used=?2 WHERE id=?1", params![id, used])?;
        tx.commit()?;
        self.revision += 1;
        Ok(())
    }
}
fn next_used(tx: &Transaction<'_>) -> Result<i64> {
    Ok(
        tx.query_row("SELECT coalesce(max(used),0)+1 FROM entries", [], |r| {
            r.get(0)
        })?,
    )
}
fn prune(tx: &Transaction<'_>, budget: Budget) -> Result<()> {
    loop {
        let (count, bytes): (usize, usize) = tx.query_row(
            "SELECT COUNT(*),coalesce(sum(length(data)),0) FROM entries WHERE pinned=0",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if count <= budget.recent_count && bytes <= budget.recent_bytes {
            break;
        }
        tx.execute("DELETE FROM entries WHERE id=(SELECT id FROM entries WHERE pinned=0 ORDER BY used,id LIMIT 1)",[])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text(s: &str) -> Payload {
        Payload::parse("text/plain", s.as_bytes().to_vec()).unwrap()
    }
    #[test]
    fn pins_survive_eviction_clear_and_restart_and_duplicates_keep_identity() {
        let dir = tempfile::tempdir().unwrap();
        let budget = Budget {
            recent_count: 2,
            recent_bytes: 100,
            pinned_count: 2,
            pinned_bytes: 100,
        };
        let mut s = Store::open_with_budget(dir.path(), budget).unwrap();
        let a = s.ingest(&text("고정 텍스트")).unwrap().unwrap();
        s.pin(a, true).unwrap();
        for value in ["first", "second", "third"] {
            s.ingest(&text(value)).unwrap();
        }
        assert_eq!(s.list("", "all", 0).unwrap().recent, 2);
        assert_eq!(s.ingest(&text("고정 텍스트")).unwrap(), Some(a));
        assert!(s.delete(a).is_err());
        s.clear_recent().unwrap();
        drop(s);
        let mut s = Store::open(dir.path()).unwrap();
        let list = s.list("고정", "pinned", 0).unwrap();
        assert_eq!(list.total, 1);
        assert_eq!(list.entries[0].id, a);
        s.pin(a, false).unwrap();
        s.delete(a).unwrap();
        assert!(
            s.ingest_current(&text("고정 텍스트"), true)
                .unwrap()
                .is_none()
        );
        assert!(s.ingest(&text("고정 텍스트")).unwrap().is_some());
    }
    #[test]
    fn pin_quota_and_byte_eviction_do_not_drop_pins() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open_with_budget(
            dir.path(),
            Budget {
                recent_count: 10,
                recent_bytes: 6,
                pinned_count: 1,
                pinned_bytes: 5,
            },
        )
        .unwrap();
        let a = s.ingest(&text("aaaaa")).unwrap().unwrap();
        s.pin(a, true).unwrap();
        let b = s.ingest(&text("bbbbb")).unwrap().unwrap();
        assert!(s.pin(b, true).is_err());
        s.ingest(&text("ccccc")).unwrap();
        assert_eq!(s.list("", "all", 0).unwrap().entries.len(), 2);
        assert_eq!(s.data(a).unwrap().1, b"aaaaa");
    }
    #[test]
    fn invalid_database_and_symlinks_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite3");
        fs::write(&path, b"broken original").unwrap();
        assert!(Store::open(dir.path()).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken original");
        let target = tempfile::tempdir().unwrap();
        let link = target.path().join("history");
        std::os::unix::fs::symlink(dir.path(), &link).unwrap();
        assert!(Store::open(&link).is_err());
    }
    #[test]
    fn images_are_validated_and_deduplicated_by_pixels() {
        let image = image::RgbaImage::from_pixel(32, 16, image::Rgba([20, 40, 60, 255]));
        let mut bytes = Cursor::new(vec![]);
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        let p = Payload::parse("image/png", bytes.into_inner()).unwrap();
        assert_eq!((p.width, p.height), (32, 16));
        assert_eq!(p.thumbnail.len(), 32 * 16 * 4);
        assert!(Payload::parse("image/png", b"not PNG".to_vec()).is_err());
        assert!(Payload::parse("text/plain", vec![0xff]).is_err());
        assert!(Payload::parse("text/plain", vec![b'x'; MAX_TEXT + 1]).is_err());
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path()).unwrap();
        let id = s.ingest(&p).unwrap().unwrap();
        s.pin(id, true).unwrap();
        assert!(s.ingest(&p).unwrap().is_none());
        assert_eq!(s.thumbnail(id).unwrap().2, p.thumbnail);
        assert_eq!(
            fs::metadata(dir.path().join("history.sqlite3"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
