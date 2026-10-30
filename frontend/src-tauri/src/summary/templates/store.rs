use super::defaults;
use super::types::Template;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

pub const STANDARD_TEMPLATE_ID: &str = "standard_meeting";
const SEED_MARKER: &str = ".seeded-v1";
const SEED_MARKER_V2: &str = ".seeded-v2";

pub struct TemplateEntry { pub id: String, pub template: Template }
pub struct TemplateListing { pub templates: Vec<TemplateEntry>, pub errors: Vec<String> }

pub fn templates_dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("Conclave").join("templates"))
}

/// Device names Windows 10 reserves in every folder: `con.json` opens the console, not a file.
const WINDOWS_RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul",
    "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9",
    "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

pub fn is_valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        && !WINDOWS_RESERVED.contains(&id)
}

fn checked_path(dir: &Path, id: &str) -> Result<PathBuf, String> {
    if is_valid_id(id) { Ok(dir.join(format!("{id}.json"))) } else { Err(format!("Invalid template id '{id}'")) }
}

fn embedded_standard() -> Template {
    serde_json::from_str(defaults::STANDARD_MEETING).expect("embedded standard_meeting.json is valid")
}

fn parse(json: &str) -> Result<Template, String> {
    let t: Template = serde_json::from_str(json).map_err(|e| format!("not a valid template: {e}"))?;
    t.validate()?;
    Ok(t)
}

pub fn get_template_in(dir: &Path, id: &str) -> Result<Template, String> {
    let path = checked_path(dir, id)?;
    let read = std::fs::read_to_string(&path).map_err(|e| format!("Template '{id}' not found: {e}")).and_then(|j| parse(&j));
    match read {
        Err(e) if id == STANDARD_TEMPLATE_ID => {
            warn!("Using the built-in Standard template: {e}");
            Ok(embedded_standard())
        }
        other => other,
    }
}

pub fn list_templates_in(dir: &Path) -> TemplateListing {
    let mut templates = Vec::new();
    let mut errors = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().to_string();
            let Some(id) = file.strip_suffix(".json") else { continue };
            if !is_valid_id(id) {
                errors.push(format!("{file}: file name is not a valid template id"));
                continue;
            }
            match std::fs::read_to_string(entry.path()).map_err(|e| e.to_string()).and_then(|j| parse(&j)) {
                Ok(template) => templates.push(TemplateEntry { id: id.to_string(), template }),
                Err(e) => {
                    warn!("Skipping template file {file}: {e}");
                    errors.push(format!("{file}: {e}"));
                }
            }
        }
    }
    if !templates.iter().any(|e| e.id == STANDARD_TEMPLATE_ID) {
        templates.push(TemplateEntry { id: STANDARD_TEMPLATE_ID.into(), template: embedded_standard() });
    }
    templates.sort_by_key(|e| e.template.name.to_lowercase());
    TemplateListing { templates, errors }
}

fn write_atomically(path: &Path, contents: &str) -> Result<(), String> {
    let dir = path.parent().ok_or("Template path has no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, contents).map_err(|e| format!("Cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Cannot save {}: {e}", path.display())
    })
}

/// Every `<id>.json` file stem, parseable or not, so a new template never overwrites a file it could not read.
fn taken_ids(dir: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten()
        .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".json").map(str::to_string))
        .collect();
    ids.push(STANDARD_TEMPLATE_ID.into());
    ids
}

pub fn save_template_in(dir: &Path, id: Option<&str>, template: &Template) -> Result<String, String> {
    template.validate()?;
    let id = match id {
        Some(id) => { checked_path(dir, id)?; id.to_string() }
        None => {
            new_template_id(&template.name, &taken_ids(dir))
        }
    };
    let json = serde_json::to_string_pretty(template).map_err(|e| e.to_string())?;
    write_atomically(&checked_path(dir, &id)?, &json)?;
    info!("Saved template '{id}'");
    Ok(id)
}

pub fn delete_template_in(dir: &Path, id: &str) -> Result<(), String> {
    let path = checked_path(dir, id)?;
    if id == STANDARD_TEMPLATE_ID {
        return Err("The Standard template cannot be deleted".into());
    }
    std::fs::remove_file(&path).map_err(|e| format!("Cannot delete template '{id}': {e}"))
}

/// Runs each seed batch once, in order. A batch writes only the files that are missing.
pub fn seed_templates_in(dir: &Path) -> Result<bool, String> {
    let v1 = seed_batch_once(dir, SEED_MARKER, &defaults::SEED_TEMPLATES)?;
    let v2 = seed_batch_once(dir, SEED_MARKER_V2, &defaults::SEED_TEMPLATES_V2)?;
    Ok(v1 || v2)
}

fn seed_batch_once(dir: &Path, marker_name: &str, seeds: &[(&str, &str)]) -> Result<bool, String> {
    let marker = dir.join(marker_name);
    if marker.exists() {
        return Ok(false);
    }
    for (id, json) in seeds {
        let path = checked_path(dir, id)?;
        if !path.exists() {
            write_atomically(&path, json)?;
        }
    }
    write_atomically(&marker, "seeded by Conclave; delete to re-add missing default templates\n")?;
    info!("Seeded default templates ({marker_name}) into {}", dir.display());
    Ok(true)
}

/// Lower-case ASCII id from a display name; Cyrillic is transliterated.
pub fn new_template_id(name: &str, existing: &[String]) -> String {
    let mut base = String::new();
    for c in name.trim().to_lowercase().chars() {
        let mapped: &str = match c {
            'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'д' => "d", 'е' | 'ё' | 'э' => "e",
            'ж' => "zh", 'з' => "z", 'и' | 'й' | 'ы' => "i", 'к' => "k", 'л' => "l", 'м' => "m",
            'н' => "n", 'о' => "o", 'п' => "p", 'р' => "r", 'с' => "s", 'т' => "t", 'у' => "u",
            'ф' => "f", 'х' => "h", 'ц' => "ts", 'ч' => "ch", 'ш' => "sh", 'щ' => "sch",
            'ю' => "yu", 'я' => "ya", 'ъ' | 'ь' => "",
            'ä' => "ae", 'ö' => "oe", 'ü' => "ue", 'ß' => "ss",
            c if c.is_ascii_alphanumeric() || c == '-' => { base.push(c); continue; }
            _ => "_",
        };
        base.push_str(mapped);
    }
    let mut base: String = base.split('_').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("_");
    base.truncate(56);
    let base = base.trim_end_matches(['_', '-']).to_string();
    let base = if base.is_empty() { "template".to_string() } else { base };
    let base = if WINDOWS_RESERVED.contains(&base.as_str()) { format!("{base}_template") } else { base };
    if !existing.contains(&base) {
        return base;
    }
    (2..).map(|n| format!("{base}_{n}")).find(|c| !existing.contains(c)).unwrap()
}

fn with_dir<T>(f: impl FnOnce(&Path) -> Result<T, String>) -> Result<T, String> {
    f(&templates_dir().ok_or("Cannot find the app data folder")?)
}

pub fn list_templates() -> TemplateListing {
    match templates_dir() {
        Some(dir) => list_templates_in(&dir),
        None => TemplateListing {
            templates: vec![TemplateEntry { id: STANDARD_TEMPLATE_ID.into(), template: embedded_standard() }],
            errors: vec!["Cannot find the app data folder".into()],
        },
    }
}
pub fn get_template(id: &str) -> Result<Template, String> {
    match templates_dir() {
        Some(dir) => get_template_in(&dir, id),
        None if id == STANDARD_TEMPLATE_ID => Ok(embedded_standard()),
        None => Err("Cannot find the app data folder".into()),
    }
}
pub fn save_template(id: Option<&str>, t: &Template) -> Result<String, String> { with_dir(|d| save_template_in(d, id, t)) }
pub fn delete_template(id: &str) -> Result<(), String> { with_dir(|d| delete_template_in(d, id)) }
pub fn seed_templates() -> Result<bool, String> { with_dir(seed_templates_in) }

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn t(name: &str) -> Template {
        serde_json::from_str(&format!(
            r#"{{"name":"{name}","sections":[{{"title":"A","instruction":"B"}}]}}"#
        )).unwrap()
    }

    #[test]
    fn seeds_are_valid_and_match_the_spec_shape() {
        let expected = [
            ("standard_meeting", None, 4), ("german_lessons", Some("ru"), 4),
            ("one_two_one_rasulov", Some("ru"), 7), ("rasulov_lpr", Some("ru"), 6),
            ("portfolio_meetings", Some("ru"), 5), ("scrum_of_scrums", Some("ru"), 6),
            ("delivery_management", Some("ru"), 6), ("one_to_one", Some("ru"), 6),
        ];
        assert_eq!(defaults::SEED_TEMPLATES.len(), expected.len());
        for (id, lang, sections) in expected {
            let json = defaults::SEED_TEMPLATES.iter().find(|(i, _)| *i == id).unwrap().1;
            assert!(!json.contains("без указания говорящих"), "{id}");
            assert!(!json.contains("no speaker labels"), "{id}");
            let t: Template = serde_json::from_str(json).unwrap();
            t.validate().unwrap();
            assert_eq!(t.fixed_language(), lang, "{id}");
            assert_eq!(t.sections.len(), sections, "{id}");
            assert!(t.use_when.is_some(), "{id} needs use_when");
        }
    }

    #[test]
    fn seeding_writes_everything_once_and_never_overwrites() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("one_to_one.json"), serde_json::to_string(&t("Mine")).unwrap()).unwrap();
        assert!(seed_templates_in(dir.path()).unwrap());
        assert_eq!(get_template_in(dir.path(), "one_to_one").unwrap().name, "Mine");
        assert!(dir.path().join(".seeded-v1").exists());
        delete_template_in(dir.path(), "german_lessons").unwrap();
        assert!(!seed_templates_in(dir.path()).unwrap(), "must not seed twice");
        assert!(get_template_in(dir.path(), "german_lessons").is_err(), "a deleted seed stays deleted");
    }

    #[test]
    fn seeding_v2_adds_interview_to_an_old_install() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(".seeded-v1"), "old").unwrap();
        let mine = serde_json::to_string(&t("Mine")).unwrap();
        std::fs::write(dir.path().join("one_to_one.json"), &mine).unwrap();
        assert!(seed_templates_in(dir.path()).unwrap());
        assert!(dir.path().join("interview.json").exists());
        assert_eq!(std::fs::read_to_string(dir.path().join("one_to_one.json")).unwrap(), mine);
        assert!(dir.path().join(".seeded-v2").exists());
        delete_template_in(dir.path(), "interview").unwrap();
        assert!(!seed_templates_in(dir.path()).unwrap(), "must not seed v2 twice");
        assert!(get_template_in(dir.path(), "interview").is_err(), "a deleted seed stays deleted");
    }

    #[test]
    fn interview_seed_is_valid() {
        let json = defaults::SEED_TEMPLATES_V2[0].1;
        assert!(!json.contains("without speaker labels"));
        let t: Template = serde_json::from_str(json).unwrap();
        t.validate().unwrap();
        assert_eq!(t.sections.len(), 5);
        assert!(t.prep.is_some());
        assert_eq!(t.fixed_language(), None);
        assert!(t.matches_title("Техническое собеседование"));
    }

    #[test]
    fn ids_from_outside_are_checked_before_any_path_is_built() {
        let dir = tempdir().unwrap();
        for bad in ["../x", "a/b", "a\\b", "C:x", "", "UPPER", "a b", &"x".repeat(65)] {
            assert!(!is_valid_id(bad), "{bad}");
            assert!(get_template_in(dir.path(), bad).is_err());
            assert!(save_template_in(dir.path(), Some(bad), &t("X")).is_err());
            assert!(delete_template_in(dir.path(), bad).is_err());
        }
        assert!(is_valid_id("one_two_one_rasulov") && is_valid_id("a-1"));
    }

    #[test]
    fn windows_device_names_are_never_ids() {
        let dir = tempdir().unwrap();
        for reserved in ["con", "prn", "aux", "nul", "com1", "com9", "lpt1", "lpt9"] {
            assert!(!is_valid_id(reserved), "{reserved}");
            assert!(get_template_in(dir.path(), reserved).is_err());
        }
        assert!(is_valid_id("console") && is_valid_id("com10") && is_valid_id("nul_day"));
        std::fs::write(dir.path().join("con.json"), serde_json::to_string(&t("Con")).unwrap()).unwrap();
        let listing = list_templates_in(dir.path());
        assert!(listing.templates.iter().all(|e| e.id != "con"));
        assert!(listing.errors.iter().any(|e| e.starts_with("con.json")));
    }

    #[test]
    fn new_ids_transliterate_and_avoid_collisions() {
        assert_eq!(new_template_id("Расулов ЛПР", &[]), "rasulov_lpr");
        assert_eq!(new_template_id("1-2-1 Расулов", &[]), "1-2-1_rasulov");
        assert_eq!(new_template_id("Scrum of Scrums", &["scrum_of_scrums".into()]), "scrum_of_scrums_2");
        assert_eq!(new_template_id("!!!", &[]), "template");
        assert!(is_valid_id(&new_template_id(&"Длинное название ".repeat(10), &[])));
        assert_eq!(new_template_id("Con", &[]), "con_template");
        assert_eq!(new_template_id("COM1", &[]), "com1_template");
        assert_eq!(new_template_id("console", &[]), "console");
    }

    #[test]
    fn save_validates_writes_atomically_and_keeps_the_id_on_rename() {
        let dir = tempdir().unwrap();
        let id = save_template_in(dir.path(), None, &t("Weekly Sync")).unwrap();
        assert_eq!(id, "weekly_sync");
        let renamed = save_template_in(dir.path(), Some(&id), &t("Weekly Sync v2")).unwrap();
        assert_eq!(renamed, id);
        assert_eq!(get_template_in(dir.path(), &id).unwrap().name, "Weekly Sync v2");
        let invalid = Template { sections: vec![], ..t("Bad") };
        assert!(save_template_in(dir.path(), Some(&id), &invalid).is_err());
        assert_eq!(get_template_in(dir.path(), &id).unwrap().name, "Weekly Sync v2", "invalid save leaves the file alone");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn standard_cannot_be_deleted_and_falls_back_to_the_embedded_copy() {
        let dir = tempdir().unwrap();
        assert!(delete_template_in(dir.path(), STANDARD_TEMPLATE_ID).is_err());
        assert_eq!(get_template_in(dir.path(), STANDARD_TEMPLATE_ID).unwrap().name, "Standard Meeting Notes");
        std::fs::write(dir.path().join("standard_meeting.json"), "{ broken").unwrap();
        assert_eq!(get_template_in(dir.path(), STANDARD_TEMPLATE_ID).unwrap().name, "Standard Meeting Notes");
    }

    #[test]
    fn new_ids_never_take_a_file_that_cannot_be_read() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("weekly_sync.json"), "{ broken").unwrap();
        let id = save_template_in(dir.path(), None, &t("Weekly Sync")).unwrap();
        assert_eq!(id, "weekly_sync_2");
        assert_eq!(std::fs::read_to_string(dir.path().join("weekly_sync.json")).unwrap(), "{ broken");
        assert_eq!(save_template_in(dir.path(), None, &t("Standard Meeting")).unwrap(), "standard_meeting_2");
    }

    #[test]
    fn listing_skips_bad_files_reports_them_and_always_has_standard() {
        let dir = tempdir().unwrap();
        save_template_in(dir.path(), None, &t("Zeta")).unwrap();
        save_template_in(dir.path(), None, &t("alpha")).unwrap();
        std::fs::write(dir.path().join("broken.json"), "{").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "ignored").unwrap();
        let listing = list_templates_in(dir.path());
        let names: Vec<_> = listing.templates.iter().map(|e| e.template.name.as_str()).collect();
        assert_eq!(names, ["alpha", "Standard Meeting Notes", "Zeta"]);
        assert_eq!(listing.errors.len(), 1);
        assert!(listing.errors[0].contains("broken.json"));
    }
}
