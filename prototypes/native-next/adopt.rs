//! One-time browser snapshot adoption. Real browser data is never opened here.
mod store;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use store::{Memo, Store};

fn all(store: &Store) -> Vec<Memo> {
    let mut list = store.list(false);
    list.extend(store.list(true));
    list
}
fn manual(dir: &Path) -> Result<HashMap<String, bool>, String> {
    let raw =
        fs::read_to_string(dir.join("catalog.tsv")).map_err(|_| "Cannot read catalog metadata")?;
    Ok(raw
        .lines()
        .skip(2)
        .filter_map(|line| {
            let p: Vec<_> = line.split('\t').collect();
            (p.len() == 5).then(|| (p[1].to_string(), p[4] != "-"))
        })
        .collect())
}
fn copy_tree(source: &Path, target: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source).map_err(|_| "Cannot inspect backup source")?;
    if metadata.file_type().is_symlink() {
        return Err("Refusing a linked backup source".into());
    }
    fs::create_dir(target).map_err(|_| "Backup destination must be new")?;
    for item in fs::read_dir(source).map_err(|_| "Cannot list backup source")? {
        let item = item.map_err(|_| "Cannot list backup source")?;
        if item.file_name() == ".store.lock" {
            continue;
        }
        let kind = item
            .file_type()
            .map_err(|_| "Cannot inspect backup entry")?;
        let destination = target.join(item.file_name());
        if kind.is_symlink() {
            return Err("Refusing a linked backup entry".into());
        }
        if kind.is_dir() {
            copy_tree(&item.path(), &destination)?
        } else if kind.is_file() {
            fs::copy(item.path(), &destination).map_err(|_| "Cannot copy backup file")?;
            fs::OpenOptions::new()
                .write(true)
                .open(&destination)
                .and_then(|f| f.sync_all())
                .map_err(|_| "Cannot sync backup file")?;
        } else {
            return Err("Unsupported backup entry".into());
        }
    }
    Ok(())
}
fn quote(s: &str) -> String {
    format!(
        "\"{}\"",
        s.chars()
            .map(|c| match c {
                '"' => "\\\"".into(),
                '\\' => "\\\\".into(),
                '\n' => "\\n".into(),
                '\r' => "\\r".into(),
                '\t' => "\\t".into(),
                c if c < ' ' => format!("\\u{:04x}", c as u32),
                c => c.to_string(),
            })
            .collect::<String>()
    )
}

fn adopt(
    source_dir: &Path,
    target_dir: &Path,
    backup_dir: &Path,
    report: &Path,
) -> Result<String, String> {
    let source_path = source_dir
        .canonicalize()
        .map_err(|_| "Source snapshot is missing")?;
    let target_path = target_dir
        .canonicalize()
        .map_err(|_| "Native target is missing")?;
    if source_path.starts_with(&target_path) || target_path.starts_with(&source_path) {
        return Err("Source and target must be separate directories".into());
    }
    let backup_parent = backup_dir
        .parent()
        .ok_or("Missing backup parent")?
        .canonicalize()
        .map_err(|_| "Backup parent is missing")?;
    let report_parent = report
        .parent()
        .ok_or("Missing report parent")?
        .canonicalize()
        .map_err(|_| "Report parent is missing")?;
    if backup_parent.starts_with(&target_path)
        || backup_parent.starts_with(&source_path)
        || report_parent.starts_with(&target_path)
        || report_parent.starts_with(&source_path)
    {
        return Err("Recovery files must be outside both stores".into());
    }
    if report.exists() {
        return Err(
            "Adoption report already exists; refusing to repeat a one-time migration".into(),
        );
    }
    if !source_dir.join("catalog.tsv").is_file() || !target_dir.join("catalog.tsv").is_file() {
        return Err("Both stores must already exist".into());
    }
    let source = Store::open(source_dir.to_path_buf())?;
    let mut target = Store::open(target_dir.to_path_buf())?;
    let sources = all(&source);
    let targets = all(&target);
    let source_manual = manual(source_dir)?;
    let target_manual = manual(target_dir)?;
    // Pre-read all text before changing anything. Store reads verify revisions.
    let source_text: HashMap<_, _> = sources
        .iter()
        .map(|m| Ok((m.id.clone(), source.text(&m.id)?)))
        .collect::<Result<_, String>>()?;
    let target_text: HashMap<_, _> = targets
        .iter()
        .map(|m| Ok((m.id.clone(), target.text(&m.id)?)))
        .collect::<Result<_, String>>()?;
    copy_tree(target_dir, backup_dir)?;
    let mut mapping = Vec::new();
    let (mut kept, mut imported, mut conflicts) = (0, 0, 0);
    for memo in &sources {
        let existing = targets.iter().find(|m| m.id == memo.id);
        let identical = existing.is_some_and(|m| {
            m.title == memo.title
                && m.archived == memo.archived
                && source_manual.get(&memo.id) == target_manual.get(&memo.id)
                && target_text.get(&memo.id) == source_text.get(&memo.id)
        });
        let destination = if identical {
            kept += 1;
            memo.id.clone()
        } else {
            let id = target.create()?;
            target.save(&id, &source_text[&memo.id])?;
            if source_manual[&memo.id] {
                target.rename(&id, &memo.title)?;
            }
            if memo.archived {
                target.archive(&id, true)?;
            }
            if let Some(previous) = existing {
                // Preserve the former comparison variant, including its text.
                if previous.archived {
                    target.archive(&previous.id, false)?;
                }
                let suffix = "（移行前の比較版）";
                let prefix: String = previous
                    .title
                    .chars()
                    .take(200 - suffix.chars().count())
                    .collect();
                target.rename(&previous.id, &format!("{prefix}{suffix}"))?;
                target.archive(&previous.id, true)?;
                conflicts += 1;
            }
            imported += 1;
            id
        };
        mapping.push((memo.id.clone(), destination));
    }
    if let Some(active) = source.active() {
        let destination = &mapping
            .iter()
            .find(|(id, _)| id == &active)
            .ok_or("Active mapping missing")?
            .1;
        target.activate(destination)?;
    }
    let final_count = all(&target).len();
    let mappings = mapping
        .iter()
        .map(|(a, b)| format!("{}:{}", quote(a), quote(b)))
        .collect::<Vec<_>>()
        .join(",");
    let json=format!("{{\"version\":1,\"completed\":true,\"sourceCount\":{},\"beforeCount\":{},\"afterCount\":{},\"kept\":{},\"imported\":{},\"conflicts\":{},\"active\":{},\"mapping\":{{{}}}}}",sources.len(),targets.len(),final_count,kept,imported,conflicts,target.active().map(|id|quote(&id)).unwrap_or("null".into()),mappings);
    // The report doubles as the one-time completion guard. Partial failures keep
    // the full pre-adoption backup for explicit recovery, never silent rollback.
    use std::io::Write;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report)
        .map_err(|_| "Cannot create adoption report")?;
    output
        .write_all(json.as_bytes())
        .and_then(|_| output.sync_all())
        .map_err(|_| "Cannot save adoption report")?;
    Ok(json)
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let option = |name: &str| {
        args.iter()
            .position(|arg| arg == name)
            .and_then(|at| args.get(at + 1))
            .map(PathBuf::from)
            .ok_or_else(|| format!("Missing {name}"))
    };
    let result = (|| {
        adopt(
            &option("--source-dir")?,
            &option("--target-dir")?,
            &option("--backup-dir")?,
            &option("--report-file")?,
        )
    })();
    match result {
        Ok(report) => println!("{report}"),
        Err(error) => {
            eprintln!("Adoption stopped: {error}");
            std::process::exit(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn folder() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "native-adopt-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }
    fn fixture(path: &Path, rows: &[(&str, &str, bool)], active: Option<&str>) {
        fs::create_dir(path).unwrap();
        fs::create_dir(path.join("memos")).unwrap();
        let mut catalog = format!(
            "minimal-editor-native-v1\nactive\t{}\n",
            active.unwrap_or("-")
        );
        for (id, text, archived) in rows {
            fs::write(path.join("memos").join(format!("{id}.txt")), text).unwrap();
            catalog.push_str(&format!(
                "memo\t{id}\t1\t{}\t-\n",
                if *archived { 1 } else { 0 }
            ));
        }
        fs::write(path.join("catalog.tsv"), catalog).unwrap();
    }
    #[test]
    fn conflict_keeps_both_and_selects_browser_variant() {
        let dir = folder();
        let (source, target) = (dir.join("source"), dir.join("target"));
        fixture(
            &source,
            &[
                ("same", "same", false),
                ("diff", "browser draft", false),
                ("new", "new draft", true),
            ],
            Some("diff"),
        );
        fixture(
            &target,
            &[
                ("same", "same", false),
                ("diff", "native draft", false),
                ("extra", "native only", false),
            ],
            Some("diff"),
        );
        let before = fs::read(target.join("catalog.tsv")).unwrap();
        let report = dir.join("report.json");
        let result = adopt(&source, &target, &dir.join("backup"), &report).unwrap();
        assert!(result.contains("\"afterCount\":5"));
        assert!(result.contains("\"kept\":1"));
        assert!(result.contains("\"conflicts\":1"));
        assert!(!result.contains("draft"));
        let store = Store::open(target.clone()).unwrap();
        let records = all(&store);
        let old = records.iter().find(|m| m.id == "diff").unwrap();
        assert!(old.archived);
        assert!(old.title.ends_with("（移行前の比較版）"));
        assert_eq!(store.text("diff").unwrap(), "native draft");
        assert_eq!(
            store.text(&store.active().unwrap()).unwrap(),
            "browser draft"
        );
        assert_eq!(store.text("extra").unwrap(), "native only");
        assert!(records
            .iter()
            .any(|m| m.archived && store.text(&m.id).unwrap() == "new draft"));
        assert_eq!(fs::read(dir.join("backup/catalog.tsv")).unwrap(), before);
        drop(store);
        assert!(adopt(&source, &target, &dir.join("second-backup"), &report).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn metadata_conflict_and_all_archived() {
        let dir = folder();
        let (source, target) = (dir.join("source"), dir.join("target"));
        fixture(&source, &[("a", "same text", true)], None);
        fixture(&target, &[("a", "same text", false)], Some("a"));
        adopt(
            &source,
            &target,
            &dir.join("backup"),
            &dir.join("report.json"),
        )
        .unwrap();
        let store = Store::open(target).unwrap();
        assert_eq!(all(&store).len(), 2);
        assert_eq!(store.active(), None);
        assert_eq!(store.list(true).len(), 2);
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn locked_store_refuses_before_backup_or_mutation() {
        let dir = folder();
        let (source, target) = (dir.join("source"), dir.join("target"));
        fixture(&source, &[("a", "source", false)], Some("a"));
        fixture(&target, &[("a", "target", false)], Some("a"));
        let store = Store::open(target.clone()).unwrap();
        assert!(adopt(
            &source,
            &target,
            &dir.join("backup"),
            &dir.join("report.json")
        )
        .is_err());
        assert!(!dir.join("backup").exists());
        assert_eq!(store.text("a").unwrap(), "target");
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn manual_title_metadata_difference_preserves_both_variants() {
        let dir = folder();
        let (source, target) = (dir.join("source"), dir.join("target"));
        fixture(&source, &[("a", "same text", false)], Some("a"));
        fixture(&target, &[("a", "same text", false)], Some("a"));
        let raw = fs::read_to_string(source.join("catalog.tsv"))
            .unwrap()
            .replace("\t0\t-\n", "\t0\t73616d652074657874\n");
        fs::write(source.join("catalog.tsv"), raw).unwrap();
        let result = adopt(
            &source,
            &target,
            &dir.join("backup"),
            &dir.join("report.json"),
        )
        .unwrap();
        assert!(result.contains("\"conflicts\":1"));
        let store = Store::open(target.clone()).unwrap();
        assert_eq!(store.list(true).len(), 1);
        assert_eq!(store.list(false).len(), 1);
        assert!(manual(&target).unwrap()[&store.active().unwrap()]);
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }
}
