//! Pre-upgrade and manual backups of the user's data folder (file copies + a manifest).
use crate::database::manager::DB_FILE_NAME;
use anyhow::{anyhow, bail, Context};
use chrono::{DateTime, Local};
use log::warn;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

mod startup;
pub use startup::*;

pub const LAST_RUN_FILE: &str = "last-run-version";
pub const BACKUPS_DIR: &str = "backups";
pub const MANIFEST_FILE: &str = "backup.json";
pub const KEEP_PRE_UPGRADE: usize = 3;
pub const KEEP_MANUAL: usize = 5;
const PRE_UPGRADE: &str = "pre-upgrade";
const MANUAL: &str = "manual";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub kind: String,
    pub created_at: String,
    pub from_version: Option<String>,
    pub version: String,
    pub size_bytes: u64,
    pub path: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    kind: String,
    created_at: String,
    from_version: Option<String>,
    version: String,
    size_bytes: u64,
    files: Vec<ManifestFile>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct ManifestFile {
    path: String,
    size_bytes: u64,
}

fn valid_version(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-'))
}

/// The file is untrusted: anything not a plain version string is treated as missing.
pub fn read_last_run(data_dir: &Path) -> Option<String> {
    let s = fs::read_to_string(data_dir.join(LAST_RUN_FILE)).ok()?;
    let s = s.trim();
    valid_version(s).then(|| s.to_string())
}

pub fn write_last_run(data_dir: &Path, version: &str) -> std::io::Result<()> {
    fs::write(data_dir.join(LAST_RUN_FILE), version)
}

fn read_manifest(dir: &Path) -> Option<Manifest> {
    let m = fs::symlink_metadata(dir).ok()?;
    if !m.is_dir() {
        return None;
    }
    serde_json::from_slice(&fs::read(dir.join(MANIFEST_FILE)).ok()?).ok()
}

fn created(m: &Manifest) -> Option<DateTime<chrono::FixedOffset>> {
    DateTime::parse_from_rfc3339(&m.created_at).ok()
}

/// Real child dirs of `kind_dir` whose manifest parses with `kind`, newest first.
fn entries(kind_dir: &Path, kind: &str) -> Vec<(PathBuf, Manifest)> {
    let Ok(rd) = fs::read_dir(kind_dir) else { return vec![] };
    let mut v: Vec<_> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let m = read_manifest(&p)?;
            (m.kind == kind && created(&m).is_some()).then_some((p, m))
        })
        .collect();
    v.sort_by(|a, b| (created(&b.1), &b.0).cmp(&(created(&a.1), &a.0)));
    v
}

pub fn list_backups(data_dir: &Path) -> Vec<BackupInfo> {
    let mut v: Vec<_> = [PRE_UPGRADE, MANUAL]
        .iter()
        .flat_map(|k| entries(&data_dir.join(BACKUPS_DIR).join(k), k))
        .collect();
    v.sort_by(|a, b| (created(&b.1), &b.0).cmp(&(created(&a.1), &a.0)));
    v.into_iter()
        .map(|(p, m)| BackupInfo {
            kind: m.kind,
            created_at: m.created_at,
            from_version: m.from_version,
            version: m.version,
            size_bytes: m.size_bytes,
            path: p.to_string_lossy().into_owned(),
        })
        .collect()
}

fn prune(kind_dir: &Path, kind: &str, keep: usize) -> anyhow::Result<()> {
    for (p, _) in entries(kind_dir, kind).into_iter().skip(keep) {
        fs::remove_dir_all(&p).with_context(|| format!("removing old backup {}", p.display()))?;
    }
    Ok(())
}

fn new_backup_dir(kind_dir: &Path, name: &str) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(kind_dir).with_context(|| format!("creating {}", kind_dir.display()))?;
    for n in 1..1000 {
        let p = kind_dir.join(if n == 1 { name.to_string() } else { format!("{name}-{n}") });
        match fs::create_dir(&p) {
            Ok(()) => return Ok(p),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("creating {}", p.display())),
        }
    }
    bail!("no free backup directory name for {name}")
}

fn copy_one(src: &Path, dest_root: &Path, rel: &str, out: &mut Vec<ManifestFile>) -> anyhow::Result<()> {
    let dest = dest_root.join(rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let size = fs::copy(src, &dest).with_context(|| format!("copying {}", src.display()))?;
    out.push(ManifestFile { path: rel.to_string(), size_bytes: size });
    Ok(())
}

fn copy_tree(src: &Path, dest_root: &Path, rel: &str, out: &mut Vec<ManifestFile>) -> anyhow::Result<()> {
    for e in fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let e = e?;
        let ft = e.file_type()?;
        let r = format!("{rel}/{}", e.file_name().to_string_lossy());
        if ft.is_dir() {
            copy_tree(&e.path(), dest_root, &r, out)?;
        } else if ft.is_file() {
            copy_one(&e.path(), dest_root, &r, out)?;
        } // symlinks are skipped
    }
    Ok(())
}

/// Top-level `*.json` of the data dir plus the templates tree, into `dest`.
fn copy_user_files(data_dir: &Path, templates: Option<&Path>, dest: &Path) -> anyhow::Result<Vec<ManifestFile>> {
    let mut out = Vec::new();
    for e in fs::read_dir(data_dir).with_context(|| format!("reading {}", data_dir.display()))? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if e.file_type()?.is_file() && name.to_lowercase().ends_with(".json") {
            copy_one(&e.path(), dest, &name, &mut out)?;
        }
    }
    if let Some(t) = templates.filter(|t| t.is_dir()) {
        copy_tree(t, dest, "templates", &mut out)?;
    }
    Ok(out)
}

fn copy_all(data_dir: &Path, templates: Option<&Path>, dest: &Path) -> anyhow::Result<Vec<ManifestFile>> {
    let mut files = Vec::new();
    for suffix in ["", "-wal", "-shm"] {
        let name = format!("{DB_FILE_NAME}{suffix}");
        let src = data_dir.join(&name);
        if suffix.is_empty() || src.exists() {
            copy_one(&src, dest, &name, &mut files)?;
        }
    }
    files.extend(copy_user_files(data_dir, templates, dest)?);
    Ok(files)
}

/// Backs up once per `last -> current` change. `Ok(None)` means nothing was needed.
pub fn pre_upgrade_backup(data_dir: &Path, templates: Option<&Path>, current: &str) -> anyhow::Result<Option<BackupInfo>> {
    if !data_dir.join(DB_FILE_NAME).exists() {
        return Ok(None);
    }
    if !valid_version(current) {
        bail!("invalid app version {current:?}");
    }
    let last = read_last_run(data_dir);
    if last.as_deref() == Some(current) {
        return Ok(None);
    }
    let kind_dir = data_dir.join(BACKUPS_DIR).join(PRE_UPGRADE);
    if entries(&kind_dir, PRE_UPGRADE).iter().any(|(_, m)| m.from_version == last && m.version == current) {
        return Ok(None);
    }
    let now = Local::now();
    let name = format!(
        "{}_{}-to-v{current}",
        now.format("%Y-%m-%d_%H%M"),
        last.as_ref().map_or("unknown".to_string(), |l| format!("v{l}"))
    );
    let dir = new_backup_dir(&kind_dir, &name)?;
    let result = (|| -> anyhow::Result<BackupInfo> {
        let files = copy_all(data_dir, templates, &dir)?;
        let m = Manifest {
            kind: PRE_UPGRADE.into(),
            created_at: now.to_rfc3339(),
            from_version: last.clone(),
            version: current.into(),
            size_bytes: files.iter().map(|f| f.size_bytes).sum(),
            files,
        };
        fs::write(dir.join(MANIFEST_FILE), serde_json::to_vec_pretty(&m)?).context("writing backup manifest")?;
        Ok(BackupInfo {
            kind: m.kind,
            created_at: m.created_at,
            from_version: m.from_version,
            version: m.version,
            size_bytes: m.size_bytes,
            path: dir.to_string_lossy().into_owned(),
        })
    })();
    match result {
        Ok(info) => {
            if let Err(e) = prune(&kind_dir, PRE_UPGRADE, KEEP_PRE_UPGRADE) {
                warn!("backup: prune failed: {e:#}");
            }
            Ok(Some(info))
        }
        Err(e) => {
            if let Err(re) = fs::remove_dir_all(&dir) {
                warn!("backup: could not remove partial {}: {re}", dir.display());
            }
            Err(anyhow!("pre-upgrade backup failed: {e:#}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn w(p: &Path, s: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    }
    fn count(d: &Path) -> usize {
        fs::read_dir(d).map(|r| r.count()).unwrap_or(0)
    }
    fn manifest(kind_dir: &Path, name: &str, kind: &str, at: &str) {
        let m = Manifest { kind: kind.into(), created_at: at.into(), from_version: None, version: "1".into(), size_bytes: 0, files: vec![] };
        w(&kind_dir.join(name).join(MANIFEST_FILE), &serde_json::to_string(&m).unwrap());
    }

    #[test]
    fn last_run_missing_is_none() {
        assert_eq!(read_last_run(tempdir().unwrap().path()), None);
    }
    #[test]
    fn last_run_round_trips() {
        let d = tempdir().unwrap();
        write_last_run(d.path(), "1.2.0").unwrap();
        assert_eq!(read_last_run(d.path()).as_deref(), Some("1.2.0"));
        w(&d.path().join(LAST_RUN_FILE), "1.2.0\n");
        assert_eq!(read_last_run(d.path()).as_deref(), Some("1.2.0"));
    }
    #[test]
    fn last_run_rejects_path_like_content() {
        let d = tempdir().unwrap();
        for bad in ["../../x", "", &"a".repeat(100)] {
            w(&d.path().join(LAST_RUN_FILE), bad);
            assert_eq!(read_last_run(d.path()), None);
        }
    }
    #[test]
    fn fresh_install_takes_no_backup() {
        let d = tempdir().unwrap();
        write_last_run(d.path(), "1.0.0").unwrap();
        assert!(pre_upgrade_backup(d.path(), None, "1.1.0").unwrap().is_none());
        assert!(!d.path().join(BACKUPS_DIR).exists());
    }
    #[test]
    fn same_version_takes_no_backup() {
        let d = tempdir().unwrap();
        w(&d.path().join(DB_FILE_NAME), "db");
        write_last_run(d.path(), "1.1.0").unwrap();
        assert!(pre_upgrade_backup(d.path(), None, "1.1.0").unwrap().is_none());
        assert!(!d.path().join(BACKUPS_DIR).exists());
    }
    #[test]
    fn db_without_last_run_counts_as_change() {
        let d = tempdir().unwrap();
        w(&d.path().join(DB_FILE_NAME), "db");
        let i = pre_upgrade_backup(d.path(), None, "1.1.0").unwrap().unwrap();
        assert_eq!(i.from_version, None);
        assert!(i.path.ends_with("_unknown-to-v1.1.0"), "{}", i.path);
    }
    #[test]
    fn new_version_copies_db_wal_shm_json_and_templates() {
        let d = tempdir().unwrap();
        let t = tempdir().unwrap();
        let p = d.path();
        for (n, c) in [(DB_FILE_NAME.to_string(), "db"), (format!("{DB_FILE_NAME}-wal"), "wal"), (format!("{DB_FILE_NAME}-shm"), "shm"),
            ("a.json".into(), "aa"), ("B.JSON".into(), "bbb"), ("notes.txt".into(), "n"), ("models/m.json".into(), "m"), ("backups/old.json".into(), "o")] {
            w(&p.join(n), c);
        }
        for (n, c) in [("t.json", "t"), (".seeded-v1", "s"), ("history/t/1.json", "h")] {
            w(&t.path().join(n), c);
        }
        write_last_run(p, "1.0.0").unwrap();
        let i = pre_upgrade_backup(p, Some(t.path()), "1.1.0").unwrap().unwrap();
        let b = PathBuf::from(&i.path);
        let m: Manifest = serde_json::from_slice(&fs::read(b.join(MANIFEST_FILE)).unwrap()).unwrap();
        assert_eq!(m.files.len(), 8);
        assert_eq!(m.size_bytes, m.files.iter().map(|f| f.size_bytes).sum::<u64>());
        assert_eq!(i.size_bytes, m.size_bytes);
        assert!(DateTime::parse_from_rfc3339(&m.created_at).is_ok());
        for f in &m.files {
            assert!(b.join(&f.path).is_file(), "{}", f.path);
        }
        assert_eq!(fs::read(b.join("B.JSON")).unwrap(), b"bbb");
        assert_eq!(fs::read(b.join("templates/history/t/1.json")).unwrap(), b"h");
        assert_eq!(fs::read(b.join("templates/.seeded-v1")).unwrap(), b"s");
        assert!(!b.join("notes.txt").exists() && !b.join("m.json").exists() && !b.join("models").exists());
    }
    #[test]
    fn missing_wal_and_templates_are_fine() {
        let d = tempdir().unwrap();
        w(&d.path().join(DB_FILE_NAME), "db");
        let i = pre_upgrade_backup(d.path(), Some(&d.path().join("nope")), "1.1.0").unwrap().unwrap();
        let m: Manifest = serde_json::from_slice(&fs::read(Path::new(&i.path).join(MANIFEST_FILE)).unwrap()).unwrap();
        assert_eq!(m.files.len(), 1);
    }
    #[test]
    fn once_per_version_pair() {
        let d = tempdir().unwrap();
        w(&d.path().join(DB_FILE_NAME), "db");
        write_last_run(d.path(), "1.0.0").unwrap();
        let kd = d.path().join(BACKUPS_DIR).join(PRE_UPGRADE);
        assert!(pre_upgrade_backup(d.path(), None, "1.1.0").unwrap().is_some());
        assert!(pre_upgrade_backup(d.path(), None, "1.1.0").unwrap().is_none());
        assert_eq!(count(&kd), 1);
        assert!(pre_upgrade_backup(d.path(), None, "1.2.0").unwrap().is_some());
        assert_eq!(count(&kd), 2);
    }
    #[test]
    fn failure_returns_err_and_leaves_no_partial_dir() {
        let d = tempdir().unwrap();
        w(&d.path().join(DB_FILE_NAME), "db");
        w(&d.path().join(BACKUPS_DIR).join(PRE_UPGRADE), "file");
        assert!(pre_upgrade_backup(d.path(), None, "1.1.0").is_err());

        let d = tempdir().unwrap();
        w(&d.path().join(DB_FILE_NAME), "db");
        fs::create_dir(d.path().join(format!("{DB_FILE_NAME}-wal"))).unwrap();
        assert!(pre_upgrade_backup(d.path(), None, "1.1.0").is_err());
        assert_eq!(count(&d.path().join(BACKUPS_DIR).join(PRE_UPGRADE)), 0);
    }
    #[test]
    fn prune_keeps_newest_n_and_ignores_strangers() {
        let d = tempdir().unwrap();
        let kd = d.path().join("manual");
        for i in 1..=7 {
            manifest(&kd, &format!("m{i}"), MANUAL, &format!("2026-01-0{i}T10:00:00+00:00"));
        }
        manifest(&kd, "other", PRE_UPGRADE, "2020-01-01T10:00:00+00:00");
        fs::create_dir_all(kd.join("nomanifest")).unwrap();
        w(&kd.join("plain.txt"), "x");
        prune(&kd, MANUAL, KEEP_MANUAL).unwrap();
        for i in 3..=7 {
            assert!(kd.join(format!("m{i}")).exists());
        }
        assert!(!kd.join("m1").exists() && !kd.join("m2").exists());
        assert!(kd.join("other").exists() && kd.join("nomanifest").exists() && kd.join("plain.txt").exists());
    }
    #[test]
    fn pre_upgrade_retention_keeps_three() {
        let d = tempdir().unwrap();
        w(&d.path().join(DB_FILE_NAME), "db");
        let kd = d.path().join(BACKUPS_DIR).join(PRE_UPGRADE);
        for i in 1..=4 {
            manifest(&kd, &format!("old{i}"), PRE_UPGRADE, &format!("2026-01-0{i}T10:00:00+00:00"));
        }
        pre_upgrade_backup(d.path(), None, "1.1.0").unwrap().unwrap();
        assert_eq!(count(&kd), 3);
        assert!(!kd.join("old1").exists() && !kd.join("old2").exists());
    }
    #[test]
    fn list_backups_newest_first_both_kinds() {
        let d = tempdir().unwrap();
        let b = d.path().join(BACKUPS_DIR);
        manifest(&b.join(PRE_UPGRADE), "a", PRE_UPGRADE, "2026-01-01T10:00:00+00:00");
        manifest(&b.join(MANUAL), "b", MANUAL, "2026-01-03T10:00:00+00:00");
        manifest(&b.join(MANUAL), "c", MANUAL, "2026-01-02T10:00:00+00:00");
        w(&b.join(MANUAL).join("bad").join(MANIFEST_FILE), "{not json");
        let l = list_backups(d.path());
        let kinds: Vec<_> = l.iter().map(|i| (i.kind.as_str(), i.created_at.as_str())).collect();
        assert_eq!(kinds, [(MANUAL, "2026-01-03T10:00:00+00:00"), (MANUAL, "2026-01-02T10:00:00+00:00"), (PRE_UPGRADE, "2026-01-01T10:00:00+00:00")]);
    }
}
