use super::store::{
    checked_path, embedded_standard, is_valid_id, parse, with_dir, STANDARD_TEMPLATE_ID,
};
use super::Template;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use tracing::warn;

pub(crate) const HISTORY_DIR: &str = ".history";
pub(crate) const HISTORY_KEEP: usize = 20;

#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HistorySource {
    #[default]
    Manual,
    Ai,
    Restore,
    Delete,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SaveOrigin {
    pub source: HistorySource,
    #[serde(default)]
    pub note: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub version: String,
    pub saved_at: String,
    pub source: HistorySource,
    pub note: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeletedTemplate {
    pub id: String,
    pub name: String,
    pub deleted_at: String,
    pub version: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryMeta {
    source: HistorySource,
    #[serde(default)]
    note: String,
    #[serde(default)]
    saved_at: String,
}

pub(crate) fn is_valid_version(v: &str) -> bool {
    let b = v.as_bytes();
    if b.len() < 19
        || b[8] != b'T'
        || b[18] != b'Z'
        || !b[..8].iter().all(u8::is_ascii_digit)
        || !b[9..18].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    b.len() == 19
        || ((21..=23).contains(&b.len()) && b[19] == b'-' && b[20..].iter().all(u8::is_ascii_digit))
}

/// Only called for validated version names.
fn version_key(version: &str) -> (&str, u16) {
    (
        &version[..19],
        version.get(20..).and_then(|s| s.parse().ok()).unwrap_or(0),
    )
}

fn versions(folder: &Path) -> io::Result<Vec<String>> {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let mut result = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Some(stem) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| is_valid_version(s))
        {
            if entry.file_type()?.is_file() {
                result.push(stem.to_string());
            }
        }
    }
    result.sort_by(|a, b| version_key(b).cmp(&version_key(a)));
    Ok(result)
}

pub(super) fn copy_to_history(dir: &Path, id: &str, origin: &SaveOrigin) -> Result<(), String> {
    archive(dir, id, origin, false, Utc::now())
}

pub(super) fn move_to_history(dir: &Path, id: &str) -> Result<(), String> {
    archive(
        dir,
        id,
        &SaveOrigin {
            source: HistorySource::Delete,
            note: String::new(),
        },
        true,
        Utc::now(),
    )
}

fn archive(
    dir: &Path,
    id: &str,
    origin: &SaveOrigin,
    moving: bool,
    now: DateTime<Utc>,
) -> Result<(), String> {
    let source = checked_path(dir, id)?;
    let keep = || -> io::Result<()> {
        let embedded = match fs::metadata(&source) {
            Ok(_) => None,
            Err(e) if e.kind() == io::ErrorKind::NotFound && !moving => {
                if id != STANDARD_TEMPLATE_ID {
                    return Ok(());
                }
                Some(serde_json::to_vec_pretty(&embedded_standard())?)
            }
            Err(e) => return Err(e),
        };
        let folder = dir.join(HISTORY_DIR).join(id);
        fs::create_dir_all(&folder)?;
        let base = now.format("%Y%m%dT%H%M%S%3fZ").to_string();
        let meta = serde_json::to_vec_pretty(&HistoryMeta {
            source: origin.source,
            note: origin.note.clone(),
            saved_at: now.to_rfc3339(),
        })?;
        for suffix in 0..=999 {
            let version = if suffix == 0 {
                base.clone()
            } else {
                format!("{base}-{suffix}")
            };
            let meta_path = folder.join(format!("{version}.meta.json"));
            let mut file = match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&meta_path)
            {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            };
            let saved = file.write_all(&meta);
            drop(file);
            let destination = folder.join(format!("{version}.json"));
            let saved = saved.and_then(|()| {
                if let Some(json) = &embedded {
                    fs::write(&destination, json)
                } else if moving {
                    fs::rename(&source, &destination)
                } else {
                    fs::copy(&source, &destination).map(|_| ())
                }
            });
            if let Err(e) = saved {
                if fs::remove_file(&meta_path).is_err() {
                    warn!("Cannot remove failed history metadata for '{id}' version '{version}'");
                }
                return Err(e);
            }
            if prune(&folder).is_err() {
                warn!("Cannot prune template history for '{id}'");
            }
            return Ok(());
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "All history version suffixes are taken",
        ))
    };
    keep().map_err(|e| format!("Cannot keep an undo copy of '{id}': {e}"))
}

fn prune(folder: &Path) -> io::Result<()> {
    for version in versions(folder)?.into_iter().skip(HISTORY_KEEP) {
        fs::remove_file(folder.join(format!("{version}.json")))?;
        match fs::remove_file(folder.join(format!("{version}.meta.json"))) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

pub fn list_history_in(dir: &Path, id: &str) -> Result<Vec<HistoryEntry>, String> {
    checked_path(dir, id)?;
    let folder = dir.join(HISTORY_DIR).join(id);
    versions(&folder)
        .map_err(|e| format!("Cannot list history for '{id}': {e}"))?
        .into_iter()
        .map(|version| {
            let meta = fs::read(folder.join(format!("{version}.meta.json")))
                .ok()
                .and_then(|json| serde_json::from_slice::<HistoryMeta>(&json).ok())
                .unwrap_or(HistoryMeta {
                    source: HistorySource::Manual,
                    note: String::new(),
                    saved_at: String::new(),
                });
            Ok(HistoryEntry {
                version,
                saved_at: meta.saved_at,
                source: meta.source,
                note: meta.note,
            })
        })
        .collect()
}

pub fn get_version_in(dir: &Path, id: &str, version: &str) -> Result<Template, String> {
    checked_path(dir, id)?;
    if !is_valid_version(version) {
        return Err("Invalid version".into());
    }
    let path = dir
        .join(HISTORY_DIR)
        .join(id)
        .join(format!("{version}.json"));
    let json = fs::read_to_string(path)
        .map_err(|e| format!("Cannot read history for '{id}' version '{version}': {e}"))?;
    parse(&json)
}

pub fn list_deleted_in(dir: &Path) -> Vec<DeletedTemplate> {
    let mut deleted = Vec::new();
    let entries = match fs::read_dir(dir.join(HISTORY_DIR)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return deleted,
        Err(_) => {
            warn!("Cannot list deleted template history");
            return deleted;
        }
    };
    for entry in entries {
        let Ok(entry) = entry else {
            warn!("Cannot read a deleted template history entry");
            continue;
        };
        let id = entry.file_name().to_string_lossy().to_string();
        if !is_valid_id(&id) {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            warn!("Cannot inspect template history for '{id}'");
            continue;
        };
        if !kind.is_dir() {
            continue;
        }
        let path = checked_path(dir, &id).expect("validated template id");
        match fs::metadata(path) {
            Ok(_) => continue,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => {
                warn!("Cannot inspect deleted template '{id}'");
                continue;
            }
        }
        let history = match list_history_in(dir, &id) {
            Ok(history) => history,
            Err(_) => {
                warn!("Cannot list template history for '{id}'");
                continue;
            }
        };
        let Some(newest) = history.into_iter().next() else {
            continue;
        };
        if newest.source != HistorySource::Delete {
            continue;
        }
        let name = get_version_in(dir, &id, &newest.version)
            .map(|t| t.name)
            .unwrap_or_else(|_| id.clone());
        deleted.push(DeletedTemplate {
            id,
            name,
            deleted_at: newest.saved_at,
            version: newest.version,
        });
    }
    deleted.sort_by(|a, b| version_key(&b.version).cmp(&version_key(&a.version)));
    deleted
}

pub fn list_history(id: &str) -> Result<Vec<HistoryEntry>, String> {
    with_dir(|d| list_history_in(d, id))
}
pub fn get_version(id: &str, version: &str) -> Result<Template, String> {
    with_dir(|d| get_version_in(d, id, version))
}
pub fn list_deleted() -> Result<Vec<DeletedTemplate>, String> {
    with_dir(|d| Ok(list_deleted_in(d)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::templates::{
        delete_template_in, get_template_in, list_templates_in, save_template_in,
        STANDARD_TEMPLATE_ID,
    };
    use tempfile::tempdir;

    fn t(name: &str) -> Template {
        serde_json::from_str(&format!(
            r#"{{"name":"{name}","sections":[{{"title":"A","instruction":"B"}}]}}"#
        ))
        .unwrap()
    }
    fn ai(note: &str) -> SaveOrigin {
        SaveOrigin {
            source: HistorySource::Ai,
            note: note.into(),
        }
    }

    #[test]
    fn save_copies_the_previous_version() {
        let d = tempdir().unwrap();
        let d = d.path();
        let id = save_template_in(d, None, &t("A"), &SaveOrigin::default()).unwrap();
        save_template_in(d, Some(&id), &t("B"), &ai("add risks")).unwrap();
        let h = list_history_in(d, &id).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].source, HistorySource::Ai);
        assert_eq!(h[0].note, "add risks");
        assert!(is_valid_version(&h[0].version));
        assert_eq!(get_version_in(d, &id, &h[0].version).unwrap().name, "A");
        assert_eq!(get_template_in(d, &id).unwrap().name, "B");
    }
    #[test]
    fn first_save_makes_no_history() {
        let d = tempdir().unwrap();
        let d = d.path();
        save_template_in(d, None, &t("A"), &SaveOrigin::default()).unwrap();
        assert!(!d.join(".history").exists());
    }
    #[test]
    fn delete_moves_into_history_and_lists_as_deleted() {
        let d = tempdir().unwrap();
        let d = d.path();
        let id = save_template_in(d, None, &t("Weekly Sync"), &SaveOrigin::default()).unwrap();
        delete_template_in(d, &id).unwrap();
        assert!(!d.join("weekly_sync.json").exists());
        let h = list_history_in(d, &id).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].source, HistorySource::Delete);
        assert!(!h[0].saved_at.is_empty());
        assert_eq!(
            list_deleted_in(d),
            vec![DeletedTemplate {
                id,
                name: "Weekly Sync".into(),
                version: h[0].version.clone(),
                deleted_at: h[0].saved_at.clone()
            }]
        );
    }
    #[test]
    fn restore_of_a_deleted_id_uses_save_with_the_id() {
        let d = tempdir().unwrap();
        let d = d.path();
        let id = save_template_in(d, None, &t("Weekly Sync"), &SaveOrigin::default()).unwrap();
        delete_template_in(d, &id).unwrap();
        let h = list_history_in(d, &id).unwrap();
        let old = get_version_in(d, &id, &h[0].version).unwrap();
        assert_eq!(
            save_template_in(
                d,
                Some(&id),
                &old,
                &SaveOrigin {
                    source: HistorySource::Restore,
                    note: "".into()
                }
            ),
            Ok(id.clone())
        );
        assert!(list_deleted_in(d).is_empty());
        let h = list_history_in(d, &id).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].source, HistorySource::Delete);
    }
    #[test]
    fn new_template_never_reuses_a_deleted_id() {
        let d = tempdir().unwrap();
        let d = d.path();
        let id = save_template_in(d, None, &t("Weekly Sync"), &SaveOrigin::default()).unwrap();
        delete_template_in(d, &id).unwrap();
        assert_eq!(
            save_template_in(d, None, &t("Weekly Sync"), &SaveOrigin::default()),
            Ok("weekly_sync_2".into())
        );
    }
    #[test]
    fn failed_history_copy_blocks_the_save() {
        let d = tempdir().unwrap();
        let d = d.path();
        let id = save_template_in(d, None, &t("A"), &SaveOrigin::default()).unwrap();
        std::fs::write(d.join(".history"), "").unwrap();
        assert!(
            save_template_in(d, Some(&id), &t("B"), &SaveOrigin::default())
                .unwrap_err()
                .contains("undo copy")
        );
        assert_eq!(get_template_in(d, &id).unwrap().name, "A");
        assert!(delete_template_in(d, &id).is_err());
        assert!(d.join(format!("{id}.json")).exists());
    }
    #[test]
    fn pruning_keeps_the_newest_20() {
        let d = tempdir().unwrap();
        let d = d.path();
        let id = save_template_in(d, None, &t("v0"), &SaveOrigin::default()).unwrap();
        for i in 1..=21 {
            save_template_in(d, Some(&id), &t(&format!("v{i}")), &SaveOrigin::default()).unwrap();
        }
        let h = list_history_in(d, &id).unwrap();
        assert_eq!(h.len(), 20);
        assert_eq!(get_version_in(d, &id, &h[0].version).unwrap().name, "v20");
        assert_eq!(get_version_in(d, &id, &h[19].version).unwrap().name, "v1");
        assert_eq!(
            std::fs::read_dir(d.join(".history").join(&id))
                .unwrap()
                .count(),
            40
        );
    }
    #[test]
    fn same_millisecond_versions_get_suffixes() {
        let d = tempdir().unwrap();
        let d = d.path();
        let id = save_template_in(d, None, &t("A"), &SaveOrigin::default()).unwrap();
        let now: DateTime<Utc> = "2026-10-30T14:22:33.123Z".parse().unwrap();
        for _ in 0..3 {
            archive(d, &id, &SaveOrigin::default(), false, now).unwrap();
        }
        let base = "20261030T142233123Z";
        let folder = d.join(".history").join(&id);
        for suffix in ["", "-1", "-2"] {
            assert!(folder.join(format!("{base}{suffix}.json")).exists());
            assert!(folder.join(format!("{base}{suffix}.meta.json")).exists());
        }
        assert_eq!(
            list_history_in(d, &id)
                .unwrap()
                .iter()
                .map(|e| e.version.clone())
                .collect::<Vec<_>>(),
            [format!("{base}-2"), format!("{base}-1"), base.into()]
        );
        std::fs::copy(
            folder.join(format!("{base}.json")),
            folder.join(format!("{base}-10.json")),
        )
        .unwrap();
        let h = list_history_in(d, &id).unwrap();
        assert_eq!(h[0].version, format!("{base}-10"));
        assert_eq!(h[1].version, format!("{base}-2"));
    }
    #[test]
    fn listing_skips_history() {
        let d = tempdir().unwrap();
        let d = d.path();
        let id = save_template_in(d, None, &t("Gone"), &SaveOrigin::default()).unwrap();
        delete_template_in(d, &id).unwrap();
        save_template_in(d, None, &t("Live"), &SaveOrigin::default()).unwrap();
        let l = list_templates_in(d);
        assert!(l.errors.is_empty());
        assert_eq!(
            l.templates
                .iter()
                .map(|e| e.template.name.as_str())
                .collect::<Vec<_>>(),
            ["Live", "Standard Meeting Notes"]
        );
    }
    #[test]
    fn versions_from_outside_are_checked() {
        let d = tempdir().unwrap();
        let d = d.path();
        for good in ["20261030T142233123Z", "20261030T142233123Z-2"] {
            assert!(is_valid_version(good), "{good}");
        }
        for bad in [
            "",
            "../x",
            "20261030T142233123Z/..",
            "20261030T142233123",
            "2026103aT142233123Z",
            "20261030T142233123Z-",
            "20261030T142233123Z-1a",
            "20261030T142233123Z-1234",
        ] {
            assert!(!is_valid_version(bad), "{bad}");
        }
        assert_eq!(
            get_version_in(d, "a", "../a"),
            Err("Invalid version".into())
        );
        assert!(list_history_in(d, "../x").is_err());
        assert!(get_version_in(d, "../x", "20261030T142233123Z").is_err());
    }
    #[test]
    fn missing_or_broken_metadata_uses_manual_defaults() {
        let dir = tempdir().unwrap();
        let d = dir.path();
        let id = save_template_in(d, None, &t("A"), &SaveOrigin::default()).unwrap();
        save_template_in(d, Some(&id), &t("B"), &ai("change")).unwrap();
        let version = list_history_in(d, &id).unwrap()[0].version.clone();
        let meta = d
            .join(HISTORY_DIR)
            .join(&id)
            .join(format!("{version}.meta.json"));
        for broken in [false, true] {
            if broken {
                fs::write(&meta, "{").unwrap();
            } else {
                fs::remove_file(&meta).unwrap();
            }
            assert_eq!(
                list_history_in(d, &id).unwrap(),
                vec![HistoryEntry {
                    version: version.clone(),
                    source: HistorySource::Manual,
                    note: String::new(),
                    saved_at: String::new(),
                }]
            );
        }
        let folder = meta.parent().unwrap();
        fs::write(folder.join("bad.json"), "{}").unwrap();
        fs::create_dir(folder.join("20261030T000000000Z.json")).unwrap();
        assert_eq!(list_history_in(d, &id).unwrap().len(), 1);
    }

    #[test]
    fn failed_archive_removes_the_claim_and_preserves_the_live_file() {
        let dir = tempdir().unwrap();
        let d = dir.path();
        let id = save_template_in(d, None, &t("A"), &SaveOrigin::default()).unwrap();
        let now: DateTime<Utc> = "2026-10-30T14:22:33.123Z".parse().unwrap();
        let folder = d.join(HISTORY_DIR).join(&id);
        fs::create_dir_all(folder.join("20261030T142233123Z.json")).unwrap();
        for moving in [false, true] {
            assert!(archive(d, &id, &SaveOrigin::default(), moving, now)
                .unwrap_err()
                .contains("undo copy"));
            assert!(!folder.join("20261030T142233123Z.meta.json").exists());
            assert_eq!(get_template_in(d, &id).unwrap().name, "A");
        }
    }

    #[test]
    fn deleted_listing_uses_newest_delete_and_falls_back_to_id() {
        let dir = tempdir().unwrap();
        let d = dir.path();
        let now: DateTime<Utc> = "2026-10-30T14:22:33.123Z".parse().unwrap();
        for (id, seconds) in [("a", 0), ("b", 1)] {
            save_template_in(d, Some(id), &t(id), &SaveOrigin::default()).unwrap();
            archive(
                d,
                id,
                &SaveOrigin {
                    source: HistorySource::Delete,
                    note: String::new(),
                },
                true,
                now + chrono::Duration::seconds(seconds),
            )
            .unwrap();
        }
        let deleted = list_deleted_in(d);
        assert_eq!(
            deleted.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["b", "a"]
        );
        fs::write(
            d.join(HISTORY_DIR)
                .join("b")
                .join(format!("{}.json", deleted[0].version)),
            "{",
        )
        .unwrap();
        assert_eq!(list_deleted_in(d)[0].name, "b");
        assert!(list_history_in(d, "missing").unwrap().is_empty());
        assert!(delete_template_in(d, "missing").is_err());
    }

    #[test]
    fn standard_special_cases() {
        let d = tempdir().unwrap();
        let d = d.path();
        assert!(delete_template_in(d, STANDARD_TEMPLATE_ID).is_err());
        assert!(!d.join(".history").exists());
        save_template_in(
            d,
            Some(STANDARD_TEMPLATE_ID),
            &t("Mine"),
            &SaveOrigin::default(),
        )
        .unwrap();
        let h = list_history_in(d, STANDARD_TEMPLATE_ID).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(
            get_version_in(d, STANDARD_TEMPLATE_ID, &h[0].version)
                .unwrap()
                .name,
            "Standard Meeting Notes"
        );
    }
}
