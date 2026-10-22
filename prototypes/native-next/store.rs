//! Dependency-free memo storage for the native comparison build.
//!
//! `catalog.tsv` is strict UTF-8, with these tab-separated lines:
//! `minimal-editor-native-v1`, `active\t<ID or ->`, then
//! `memo\t<ID>\t<created milliseconds>\t<0 or 1 archived>\t<title UTF-8 hex or ->`.
//! A `-` title follows the first text line (empty means 無題). Text lives in
//! `memos/<ID>.txt`. IDs contain only ASCII letters, digits, `-`, `_` (1..=80).
//! Seeders must publish every text before publishing the catalog, and must not
//! overwrite an existing store. No automatic recovery ever replaces user data.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HEADER: &str = "minimal-editor-native-v1";
const HISTORY_INTERVAL: Duration = Duration::from_secs(60);
static NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Memo {
    pub id: String,
    pub title: String,
    pub created: u64,
    pub archived: bool,
}

#[derive(Clone)]
struct Record {
    memo: Memo,
    manual_title: Option<String>,
    revision: [u8; 32],
    checkpoint: Option<Instant>,
}

pub struct Store {
    dir: PathBuf,
    records: Vec<Record>,
    active: Option<String>,
    catalog: Vec<u8>,
    // Advisory lock is released by the OS, including after forced termination.
    _lock: File,
}

impl Store {
    pub fn open(dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&dir).map_err(|e| err("保存先を作れません", e))?;
        require_directory(&dir)?;
        let lock_path = dir.join(".store.lock");
        reject_link(&lock_path)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| err("保存先のロックを開けません", e))?;
        lock.try_lock()
            .map_err(|e| err("この保存先は別のネイティブ版が使用中です", e))?;
        let catalog_path = dir.join("catalog.tsv");
        reject_link(&catalog_path)?;
        let fresh = !catalog_path
            .try_exists()
            .map_err(|e| err("保存先を確認できません", e))?;
        if fresh {
            // A missing catalog beside any other data is a recovery condition,
            // including leftovers from an interrupted first initialization.
            let entries = fs::read_dir(&dir).map_err(|e| err("保存先を読めません", e))?;
            for entry in entries {
                let entry = entry.map_err(|e| err("保存先を読めません", e))?;
                if entry.file_name() != ".store.lock" {
                    return Err("一覧がありません。既存ファイルを保ったまま復旧が必要です".into());
                }
            }
            fs::create_dir(dir.join("memos")).map_err(|e| err("メモの保存先を作れません", e))?;
            let id = new_id();
            write_new(&dir.join("memos").join(format!("{id}.txt")), b"")?;
            let raw = format!("{HEADER}\nactive\t{id}\nmemo\t{id}\t{}\t0\t-\n", now_ms());
            write_new(&catalog_path, raw.as_bytes())?;
        }
        require_directory(&dir.join("memos"))?;
        let raw = read_regular(&catalog_path)?;
        let (active, mut records) = parse_catalog(&raw)?;
        for record in &mut records {
            let bytes = read_regular(&dir.join("memos").join(format!("{}.txt", record.memo.id)))?;
            let text = std::str::from_utf8(&bytes).map_err(|_| {
                format!(
                    "メモ {} が UTF-8 ではありません。元のファイルは変更していません",
                    record.memo.id
                )
            })?;
            record.revision = sha256(&bytes);
            record.memo.title = record
                .manual_title
                .clone()
                .unwrap_or_else(|| derived_title(text));
        }
        Ok(Self {
            dir,
            records,
            active,
            catalog: raw,
            _lock: lock,
        })
    }

    /// This is a display snapshot. Reads and every mutation separately verify
    /// the underlying files, so a stale list can never authorize overwriting.
    pub fn list(&self, archived: bool) -> Vec<Memo> {
        let mut values: Vec<_> = self
            .records
            .iter()
            .filter(|r| r.memo.archived == archived)
            .map(|r| r.memo.clone())
            .collect();
        values.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| a.id.cmp(&b.id)));
        values
    }

    pub fn active(&self) -> Option<String> {
        self.active.clone()
    }

    pub fn text(&self, id: &str) -> Result<String, String> {
        self.check_catalog()?;
        let record = self.record(id)?;
        self.read_verified(record)
    }

    pub fn save(&mut self, id: &str, text: &str) -> Result<(), String> {
        self.check_catalog()?;
        let index = self.index(id)?;
        if self.records[index].memo.archived {
            return Err("保管中のメモは編集できません".into());
        }
        let previous = self.read_verified(&self.records[index])?;
        if previous == text {
            return Ok(());
        }
        let needs_checkpoint = self.records[index]
            .checkpoint
            .map(|at| at.elapsed() >= HISTORY_INTERVAL)
            .unwrap_or(true);
        if needs_checkpoint {
            self.checkpoint(id, previous.as_bytes())?;
            self.records[index].checkpoint = Some(Instant::now());
        }
        let path = self.memo_path(id);
        // The preceding version is durable before its successor is published.
        atomic_replace(&path.with_extension("txt.bak"), previous.as_bytes())?;
        // Check again after writing backup/history, to narrow the conflict window.
        self.check_catalog()?;
        self.read_verified(&self.records[index])?;
        atomic_replace(&path, text.as_bytes())?;
        self.records[index].revision = sha256(text.as_bytes());
        if self.records[index].manual_title.is_none() {
            self.records[index].memo.title = derived_title(text);
        }
        Ok(())
    }

    /// Create and select a new memo. A failed catalog update can leave an
    /// unlisted file, but cannot publish a record with missing contents.
    pub fn create(&mut self) -> Result<String, String> {
        self.check_catalog()?;
        require_directory(&self.dir.join("memos"))?;
        let (id, created) = (new_id(), now_ms());
        write_new(&self.memo_path(&id), b"")?;
        let mut records = self.records.clone();
        records.push(Record {
            memo: Memo {
                id: id.clone(),
                title: "無題".into(),
                created,
                archived: false,
            },
            manual_title: None,
            revision: sha256(b""),
            checkpoint: None,
        });
        self.publish(records, Some(id.clone()))?;
        Ok(id)
    }

    pub fn activate(&mut self, id: &str) -> Result<(), String> {
        self.check_catalog()?;
        let record = self.record(id)?;
        if record.memo.archived {
            return Err("保管中のメモは開けません".into());
        }
        self.read_verified(record)?;
        if self.active.as_deref() != Some(id) {
            self.publish(self.records.clone(), Some(id.to_owned()))?;
        }
        Ok(())
    }

    pub fn rename(&mut self, id: &str, title: &str) -> Result<(), String> {
        self.check_catalog()?;
        let index = self.index(id)?;
        if self.records[index].memo.archived {
            return Err("保管中のメモは改名できません".into());
        }
        let title = title.trim();
        validate_title(title)?;
        self.read_verified(&self.records[index])?;
        let manual_title = Some(title.to_owned());
        if self.records[index].manual_title == manual_title {
            return Ok(());
        }
        let mut records = self.records.clone();
        records[index].memo.title = title.to_owned();
        records[index].manual_title = manual_title;
        self.publish(records, self.active.clone())
    }

    pub fn archive(&mut self, id: &str, archived: bool) -> Result<(), String> {
        self.check_catalog()?;
        let index = self.index(id)?;
        self.read_verified(&self.records[index])?;
        if self.records[index].memo.archived == archived {
            return Ok(());
        }
        let mut records = self.records.clone();
        records[index].memo.archived = archived;
        let active = if self.active.as_deref() == Some(id) && archived || self.active.is_none() {
            newest_active(&records)
        } else {
            self.active.clone()
        };
        // Validate the next visible text before publishing the active change.
        if let Some(ref next) = active {
            let record = records.iter().find(|r| r.memo.id == *next).unwrap();
            self.read_verified(record)?;
        }
        self.publish(records, active)
    }

    /// Preserve an unsaved editor buffer even after an external-edit conflict.
    /// The caller must surface the returned filename; recovery is never automatic.
    pub fn recover(&self, id: &str, text: &str) -> Result<PathBuf, String> {
        self.record(id)?;
        let directory = self.dir.join("recovery");
        make_directory(&directory)?;
        let path = directory.join(format!("{id}-{}-{}.txt", now_ms(), next_nonce()));
        write_new(&path, text.as_bytes())?;
        Ok(path)
    }

    fn memo_path(&self, id: &str) -> PathBuf {
        self.dir.join("memos").join(format!("{id}.txt"))
    }

    fn index(&self, id: &str) -> Result<usize, String> {
        if !valid_id(id) {
            return Err("不正なメモ ID です".into());
        }
        self.records
            .iter()
            .position(|r| r.memo.id == id)
            .ok_or_else(|| "メモが見つかりません".into())
    }

    fn record(&self, id: &str) -> Result<&Record, String> {
        Ok(&self.records[self.index(id)?])
    }

    fn check_catalog(&self) -> Result<(), String> {
        if read_regular(&self.dir.join("catalog.tsv"))? != self.catalog {
            return Err("一覧がアプリの外で変更されました。上書きせず停止しました".into());
        }
        Ok(())
    }

    fn read_verified(&self, record: &Record) -> Result<String, String> {
        require_directory(&self.dir.join("memos"))?;
        let bytes = read_regular(&self.memo_path(&record.memo.id))?;
        let text = String::from_utf8(bytes).map_err(|_| {
            "メモが UTF-8 ではありません。元のファイルは変更していません".to_owned()
        })?;
        if sha256(text.as_bytes()) != record.revision {
            return Err("メモがアプリの外で変更されました。上書きせず停止しました".into());
        }
        Ok(text)
    }

    fn checkpoint(&self, id: &str, text: &[u8]) -> Result<(), String> {
        let history = self.dir.join("history");
        make_directory(&history)?;
        let directory = history.join(id);
        make_directory(&directory)?;
        write_new(
            &directory.join(format!("{}-{}.txt", now_ms(), next_nonce())),
            text,
        )
    }

    fn publish(&mut self, records: Vec<Record>, active: Option<String>) -> Result<(), String> {
        self.check_catalog()?;
        let raw = encode_catalog(&records, active.as_deref());
        atomic_replace(&self.dir.join("catalog.tsv.bak"), &self.catalog)?;
        self.check_catalog()?;
        atomic_replace(&self.dir.join("catalog.tsv"), &raw)?;
        self.records = records;
        self.active = active;
        self.catalog = raw;
        Ok(())
    }
}

fn parse_catalog(raw: &[u8]) -> Result<(Option<String>, Vec<Record>), String> {
    let text = std::str::from_utf8(raw).map_err(|_| "一覧が UTF-8 ではありません".to_owned())?;
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err("一覧の形式が不正です".into());
    }
    let active_line = lines.next().ok_or("一覧に選択中のメモがありません")?;
    let active_id = active_line
        .strip_prefix("active\t")
        .ok_or("一覧の選択中メモが不正です")?;
    let active = if active_id == "-" {
        None
    } else {
        if !valid_id(active_id) {
            return Err("一覧の選択中メモ ID が不正です".into());
        }
        Some(active_id.to_owned())
    };
    let mut records = Vec::new();
    let mut ids = HashSet::new();
    for line in lines {
        let parts: Vec<_> = line.split('\t').collect();
        if parts.len() != 5
            || parts[0] != "memo"
            || !valid_id(parts[1])
            || !ids.insert(parts[1].to_ascii_lowercase())
        {
            return Err("一覧のメモ情報が不正です".into());
        }
        let created = parts[2]
            .parse::<u64>()
            .map_err(|_| "一覧の作成時刻が不正です".to_owned())?;
        let archived = match parts[3] {
            "0" => false,
            "1" => true,
            _ => return Err("一覧の保管状態が不正です".into()),
        };
        let manual_title = if parts[4] == "-" {
            None
        } else {
            let title = decode_hex(parts[4])?;
            validate_title(&title)?;
            Some(title)
        };
        records.push(Record {
            memo: Memo {
                id: parts[1].into(),
                title: String::new(),
                created,
                archived,
            },
            manual_title,
            revision: [0; 32],
            checkpoint: None,
        });
    }
    if records.is_empty() {
        return Err("一覧にメモがありません".into());
    }
    if active
        .as_ref()
        .map(|id| records.iter().any(|r| r.memo.id == *id && !r.memo.archived))
        .unwrap_or(false)
        != records.iter().any(|r| !r.memo.archived)
    {
        return Err("一覧の選択中メモと保管状態が一致しません".into());
    }
    // `None` is valid only when every memo is archived. An unknown active ID is
    // invalid even in an all-archived catalog.
    if active.is_some()
        && !records
            .iter()
            .any(|r| Some(&r.memo.id) == active.as_ref() && !r.memo.archived)
    {
        return Err("一覧の選択中メモが見つかりません".into());
    }
    Ok((active, records))
}

fn encode_catalog(records: &[Record], active: Option<&str>) -> Vec<u8> {
    let mut raw = format!("{HEADER}\nactive\t{}\n", active.unwrap_or("-"));
    for record in records {
        raw.push_str(&format!(
            "memo\t{}\t{}\t{}\t{}\n",
            record.memo.id,
            record.memo.created,
            if record.memo.archived { 1 } else { 0 },
            record
                .manual_title
                .as_ref()
                .map(|t| encode_hex(t.as_bytes()))
                .unwrap_or_else(|| "-".into())
        ));
    }
    raw.into_bytes()
}

fn newest_active(records: &[Record]) -> Option<String> {
    records
        .iter()
        .filter(|r| !r.memo.archived)
        .max_by(|a, b| {
            a.memo
                .created
                .cmp(&b.memo.created)
                .then_with(|| b.memo.id.cmp(&a.memo.id))
        })
        .map(|r| r.memo.id.clone())
}

fn valid_id(id: &str) -> bool {
    (1..=80).contains(&id.len()) && id != "-" && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        // Windows device names remain special even with a .txt extension.
        && !matches!(id.to_ascii_uppercase().as_str(), "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6" | "COM7" | "COM8" | "COM9" | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9")
}

fn validate_title(title: &str) -> Result<(), String> {
    if title.is_empty()
        || title.trim() != title
        || title.chars().count() > 200
        || title
            .chars()
            .any(|c| c.is_control() || c == '\u{2028}' || c == '\u{2029}')
    {
        Err("名前は改行や制御文字を含まない 200 文字以内にしてください".into())
    } else {
        Ok(())
    }
}

fn derived_title(text: &str) -> String {
    // A leading newline is an empty first line, never permission to use a
    // later paragraph. RichEdit wraps visually without adding a newline.
    let title: String = text
        .split(['\r', '\n'])
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    if title.is_empty() {
        "無題".into()
    } else {
        title
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
fn next_nonce() -> u64 {
    NONCE.fetch_add(1, Ordering::Relaxed)
}
fn new_id() -> String {
    format!("n{}-{}-{}", now_ms(), std::process::id(), next_nonce())
}
fn err(context: &str, error: impl std::fmt::Display) -> String {
    format!("{context}: {error}")
}

fn reject_link(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("リンク先には保存しません: {}", path.display()))
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(err("保存先の種類を確認できません", e)),
    }
}

fn require_directory(path: &Path) -> Result<(), String> {
    reject_link(path)?;
    if fs::metadata(path)
        .map_err(|e| err("保存先を確認できません", e))?
        .is_dir()
    {
        Ok(())
    } else {
        Err(format!(
            "保存先がフォルダーではありません: {}",
            path.display()
        ))
    }
}

fn make_directory(path: &Path) -> Result<(), String> {
    reject_link(path)?;
    match fs::create_dir(path) {
        Ok(()) => sync_directory(path.parent().unwrap_or(path)),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => require_directory(path),
        Err(e) => Err(err("保存先を作れません", e)),
    }
}

fn read_regular(path: &Path) -> Result<Vec<u8>, String> {
    reject_link(path)?;
    if !fs::metadata(path)
        .map_err(|e| err(&format!("ファイルを確認できません ({})", path.display()), e))?
        .is_file()
    {
        return Err(format!("通常のファイルではありません: {}", path.display()));
    }
    fs::read(path).map_err(|e| err(&format!("ファイルを読めません ({})", path.display()), e))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            err(
                &format!("新しい保存ファイルを作れません ({})", path.display()),
                e,
            )
        })?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| err("保存ファイルを同期できません", e))?;
    drop(file);
    sync_directory(path.parent().ok_or("保存先に親フォルダーがありません")?)
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    reject_link(path)?;
    let parent = path.parent().ok_or("保存先に親フォルダーがありません")?;
    require_directory(parent)?;
    let temp = parent.join(format!(
        ".{}.{}-{}.tmp",
        path.file_name()
            .ok_or("保存名がありません")?
            .to_string_lossy(),
        std::process::id(),
        next_nonce()
    ));
    write_new(&temp, bytes)?;
    let result = replace_file(&temp, path).and_then(|_| sync_directory(parent));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // Same-directory rename, replacing the previous complete file. WRITE_THROUGH
    // asks Windows to complete the move on disk before returning.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0x1 | 0x8) } == 0 {
        Err(err(
            "保存ファイルを置き換えられません",
            std::io::Error::last_os_error(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(source, destination).map_err(|e| err("保存ファイルを置き換えられません", e))
}

#[cfg(windows)]
fn sync_directory(_: &Path) -> Result<(), String> {
    Ok(())
} // MoveFileExW WRITE_THROUGH above.
#[cfg(not(windows))]
fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| err("保存先フォルダーを同期できません", e))
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}

fn decode_hex(value: &str) -> Result<String, String> {
    if value.len() % 2 != 0 || value.len() > 200 * 4 * 2 {
        return Err("一覧の名前の符号化が不正です".into());
    }
    let digit = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    };
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        bytes.push(
            digit(pair[0]).ok_or("一覧の名前の符号化が不正です")? * 16
                + digit(pair[1]).ok_or("一覧の名前の符号化が不正です")?,
        );
    }
    String::from_utf8(bytes).map_err(|_| "一覧の名前が UTF-8 ではありません".into())
}

// A cryptographic revision avoids retaining a duplicate full text for every
// memo. Hashes are only for conflict detection; no security protocol is involved.
fn sha256(bytes: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let blocks = bytes.len() / 64 + if bytes.len() % 64 < 56 { 1 } else { 2 };
    let bit_len = (bytes.len() as u64).wrapping_mul(8).to_be_bytes();
    for block in 0..blocks {
        let mut chunk = [0u8; 64];
        for (offset, value) in chunk.iter_mut().enumerate() {
            let index = block * 64 + offset;
            *value = if index < bytes.len() {
                bytes[index]
            } else if index == bytes.len() {
                0x80
            } else if index >= blocks * 64 - 8 {
                bit_len[index - (blocks * 64 - 8)]
            } else {
                0
            };
        }
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (value, extra) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *value = value.wrapping_add(extra);
        }
    }
    let mut digest = [0u8; 32];
    for (part, value) in digest.chunks_exact_mut(4).zip(state) {
        part.copy_from_slice(&value.to_be_bytes());
    }
    digest
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "minimal-native-store-test-{}-{}-{}",
                std::process::id(),
                now_ms(),
                next_nonce()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn sha256_known_vectors_and_padding_boundaries() {
        assert_eq!(
            encode_hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            encode_hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            encode_hex(&sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            encode_hex(&sha256(&vec![b'a'; 1_000_000])),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn automatic_title_uses_only_the_first_line() {
        for (text, expected) in [
            ("", "無題"),
            ("先頭の入力中", "先頭の入力中"),
            ("一行目\n二行目", "一行目"),
            ("一行目\r\n二行目", "一行目"),
            ("一行目\r二行目", "一行目"),
            ("\n二行目", "無題"),
            ("\r\n二行目", "無題"),
            (" \t\n二行目", "無題"),
            ("  一行目  \n二行目", "一行目"),
        ] {
            assert_eq!(derived_title(text), expected, "{text:?}");
        }
        let long = "長い先頭行📝".repeat(50);
        assert_eq!(derived_title(&format!("{long}\n二行目")), long);
    }

    #[test]
    fn automatic_title_tracks_edits_and_restart_until_manual_rename() {
        let directory = Temp::new();
        let id;
        {
            let mut store = Store::open(directory.0.clone()).unwrap();
            assert_eq!(store.list(false)[0].title, "無題");
            id = store.create().unwrap();
            let title = |store: &Store| {
                store.list(false).into_iter().find(|m| m.id == id).unwrap().title
            };
            assert_eq!(title(&store), "無題");
            store.save(&id, "入力中").unwrap();
            assert_eq!(title(&store), "入力中");
            store.save(&id, "変更した先頭行\n本文").unwrap();
            assert_eq!(title(&store), "変更した先頭行");
            store.save(&id, "\n本文だけ").unwrap();
            assert_eq!(title(&store), "無題");
        }
        {
            let mut store = Store::open(directory.0.clone()).unwrap();
            assert_eq!(store.list(false).iter().find(|m| m.id == id).unwrap().title, "無題");
            store.save(&id, "再開後の先頭行\n本文").unwrap();
        }
        {
            let mut store = Store::open(directory.0.clone()).unwrap();
            assert_eq!(store.list(false).iter().find(|m| m.id == id).unwrap().title, "再開後の先頭行");
            store.rename(&id, "手動の名前").unwrap();
            store.save(&id, "別の先頭行\n本文").unwrap();
            store.save(&id, "").unwrap();
        }
        let store = Store::open(directory.0.clone()).unwrap();
        assert_eq!(store.list(false).iter().find(|m| m.id == id).unwrap().title, "手動の名前");
        assert_eq!(store.text(&id).unwrap(), "");
    }

    #[test]
    fn restart_roundtrip_backup_history_and_manual_title() {
        let directory = Temp::new();
        let id;
        {
            let mut store = Store::open(directory.0.clone()).unwrap();
            id = store.active().unwrap();
            store.save(&id, "\n 最初の行 \r\n二行目\n").unwrap();
            assert_eq!(store.list(false)[0].title, "無題");
            store.rename(&id, "手動の名前").unwrap();
            store.save(&id, "本文が変わっても名前は残る\n").unwrap();
            assert_eq!(store.list(false)[0].title, "手動の名前");
            assert_eq!(
                fs::read_to_string(store.memo_path(&id).with_extension("txt.bak")).unwrap(),
                "\n 最初の行 \r\n二行目\n"
            );
            assert_eq!(
                fs::read_dir(directory.0.join("history").join(&id))
                    .unwrap()
                    .count(),
                1
            );
            store.records[0].checkpoint = Some(Instant::now() - Duration::from_secs(61));
            store.save(&id, "第三版").unwrap();
            assert_eq!(
                fs::read_dir(directory.0.join("history").join(&id))
                    .unwrap()
                    .count(),
                2
            );
        }
        let mut store = Store::open(directory.0.clone()).unwrap();
        assert_eq!(store.text(&id).unwrap(), "第三版");
        assert_eq!(store.list(false)[0].title, "手動の名前");
        assert!(store.rename(&id, "").is_err());
        assert!(store.rename(&id, "a\nb").is_err());
        assert!(store.rename(&id, &"あ".repeat(201)).is_err());
        store.rename(&id, &"あ".repeat(200)).unwrap();
    }

    #[test]
    fn archive_all_restore_and_sort_without_deleting_text() {
        let directory = Temp::new();
        let mut store = Store::open(directory.0.clone()).unwrap();
        let first = store.active().unwrap();
        store.save(&first, "保存を保つ").unwrap();
        let second = store.create().unwrap();
        assert_eq!(store.active().as_deref(), Some(second.as_str()));
        store.archive(&second, true).unwrap();
        assert_eq!(store.active().as_deref(), Some(first.as_str()));
        assert!(store.save(&second, "禁止").is_err());
        store.archive(&first, true).unwrap();
        assert!(store.active().is_none());
        assert!(store.list(false).is_empty());
        drop(store);
        let mut store = Store::open(directory.0.clone()).unwrap();
        assert!(store.active().is_none());
        store.archive(&first, false).unwrap();
        assert_eq!(store.text(&first).unwrap(), "保存を保つ");
        assert_eq!(store.active().as_deref(), Some(first.as_str()));
        store.archive(&second, false).unwrap();
        assert_eq!(store.active().as_deref(), Some(first.as_str()));
    }

    #[test]
    fn lifetime_lock_is_released_on_drop() {
        let directory = Temp::new();
        let store = Store::open(directory.0.clone()).unwrap();
        assert!(Store::open(directory.0.clone()).is_err());
        drop(store);
        assert!(Store::open(directory.0.clone()).is_ok());
    }

    #[test]
    fn outside_text_change_never_overwritten_and_buffer_recoverable() {
        let directory = Temp::new();
        let mut store = Store::open(directory.0.clone()).unwrap();
        let id = store.active().unwrap();
        store.save(&id, "元の本文").unwrap();
        fs::write(store.memo_path(&id), "外部の変更").unwrap();
        assert!(store.save(&id, "アプリの未保存文").is_err());
        assert!(store.text(&id).is_err());
        assert!(store.rename(&id, "上書き不可").is_err());
        let path = store.recover(&id, "アプリの未保存文").unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "アプリの未保存文");
        assert_eq!(
            fs::read_to_string(store.memo_path(&id)).unwrap(),
            "外部の変更"
        );
    }

    #[test]
    fn outside_catalog_change_never_overwritten() {
        let directory = Temp::new();
        let mut store = Store::open(directory.0.clone()).unwrap();
        let id = store.active().unwrap();
        fs::write(directory.0.join("catalog.tsv"), "outside\n").unwrap();
        assert!(store.create().is_err());
        assert!(store.save(&id, "変更しない").is_err());
        assert!(store.activate(&id).is_err());
        assert_eq!(
            fs::read_to_string(directory.0.join("catalog.tsv")).unwrap(),
            "outside\n"
        );
    }

    #[test]
    fn missing_or_invalid_text_and_missing_catalog_do_not_become_blank() {
        let directory = Temp::new();
        let mut store = Store::open(directory.0.clone()).unwrap();
        let id = store.active().unwrap();
        store.save(&id, "消してはいけない").unwrap();
        fs::write(store.memo_path(&id), [0xff, 0xfe]).unwrap();
        assert!(store.text(&id).is_err());
        assert!(store.save(&id, "").is_err());
        drop(store);
        assert!(Store::open(directory.0.clone()).is_err());
        fs::remove_file(directory.0.join("memos").join(format!("{id}.txt"))).unwrap();
        assert!(Store::open(directory.0.clone()).is_err());
        fs::remove_file(directory.0.join("catalog.tsv")).unwrap();
        assert!(Store::open(directory.0.clone()).is_err());
    }

    #[test]
    fn strict_import_format_rejects_traversal_duplicates_and_invalid_active() {
        let invalid = [
            format!("{HEADER}\nactive\t../x\nmemo\t../x\t1\t0\t-\n"),
            format!("{HEADER}\nactive\tCON\nmemo\tCON\t1\t0\t-\n"),
            format!("{HEADER}\nactive\ta\nmemo\ta\t1\t0\t-\nmemo\ta\t2\t0\t-\n"),
            format!("{HEADER}\nactive\t-\nmemo\ta\t1\t0\t-\n"),
            format!("{HEADER}\nactive\ta\nmemo\ta\t1\t1\t-\n"),
            format!("{HEADER}\nactive\t-\nmemo\ta\t1\t1\tff\n"),
        ];
        for raw in invalid {
            assert!(parse_catalog(raw.as_bytes()).is_err(), "accepted {raw}");
        }
        let raw = format!(
            "{HEADER}\nactive\tweb-legacy\nmemo\tweb-legacy\t1234\t0\t{}\n",
            encode_hex("手書き".as_bytes())
        );
        let (active, records) = parse_catalog(raw.as_bytes()).unwrap();
        assert_eq!(active.as_deref(), Some("web-legacy"));
        assert_eq!(records[0].manual_title.as_deref(), Some("手書き"));
    }

    #[test]
    fn failed_replace_preserves_published_text() {
        let directory = Temp::new();
        let mut store = Store::open(directory.0.clone()).unwrap();
        let id = store.active().unwrap();
        store.save(&id, "元の文").unwrap();
        fs::remove_file(store.memo_path(&id).with_extension("txt.bak")).unwrap();
        fs::create_dir(store.memo_path(&id).with_extension("txt.bak")).unwrap();
        assert!(store.save(&id, "保存できない文").is_err());
        assert_eq!(store.text(&id).unwrap(), "元の文");
        assert_eq!(fs::read_to_string(store.memo_path(&id)).unwrap(), "元の文");
    }

    #[test]
    fn large_japanese_text_survives_restart() {
        let directory = Temp::new();
        let mut store = Store::open(directory.0.clone()).unwrap();
        let id = store.active().unwrap();
        let text = "日本語の文章を一人で書く。\n".repeat(2000);
        store.save(&id, &text).unwrap();
        drop(store);
        assert_eq!(
            Store::open(directory.0.clone()).unwrap().text(&id).unwrap(),
            text
        );
    }
}
