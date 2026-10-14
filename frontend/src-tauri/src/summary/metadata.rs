use anyhow::{bail, Context, Result};
use once_cell::sync::Lazy;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::processor::language_name_from_code;
use super::template_choice::TemplateSource;

const SUMMARY_LANGUAGE_FIELD: &str = "summary_language";
const DETECTED_SUMMARY_LANGUAGE_FIELD: &str = "detected_summary_language";
const TEMPLATE_FIELD: &str = "summary_template";
const TEMPLATE_SOURCE_FIELD: &str = "summary_template_source";
const METADATA_FILE: &str = "metadata.json";
const METADATA_TEMP_FILE_PREFIX: &str = ".metadata.json.";
static METADATA_WRITE_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub(crate) fn read_summary_language_from_metadata(folder: &Path) -> Result<Option<String>> {
    read_language_field_from_metadata(folder, SUMMARY_LANGUAGE_FIELD)
}

pub(crate) fn read_detected_summary_language_from_metadata(
    folder: &Path,
) -> Result<Option<String>> {
    read_language_field_from_metadata(folder, DETECTED_SUMMARY_LANGUAGE_FIELD)
}

pub(crate) fn write_summary_language_to_metadata(
    folder: &Path,
    summary_language: Option<&str>,
) -> Result<()> {
    write_language_field_to_metadata(folder, SUMMARY_LANGUAGE_FIELD, summary_language)
}

pub(crate) fn write_detected_summary_language_to_metadata(
    folder: &Path,
    summary_language: Option<&str>,
) -> Result<()> {
    write_language_field_to_metadata(folder, DETECTED_SUMMARY_LANGUAGE_FIELD, summary_language)
}

/// The template chosen for the meeting; None unless both fields are present and the source parses.
pub(crate) fn read_template_choice_from_metadata(
    folder: &Path,
) -> Result<Option<(String, TemplateSource)>> {
    let Some(value) = read_metadata_value(folder)? else {
        return Ok(None);
    };
    let id = value.get(TEMPLATE_FIELD).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let source = value
        .get(TEMPLATE_SOURCE_FIELD)
        .and_then(|s| serde_json::from_value::<TemplateSource>(s.clone()).ok());
    Ok(id.zip(source).map(|(id, source)| (id.to_string(), source)))
}

pub(crate) fn write_template_choice_to_metadata(
    folder: &Path,
    choice: Option<(&str, TemplateSource)>,
) -> Result<()> {
    let (id, source) = match choice {
        Some((id, source)) => (Some(Value::String(id.to_string())), Some(serde_json::json!(source))),
        None => (None, None),
    };
    write_metadata_fields(folder, &[(TEMPLATE_FIELD, id), (TEMPLATE_SOURCE_FIELD, source)])
}

fn read_metadata_value(folder: &Path) -> Result<Option<Value>> {
    let metadata_path = metadata_path(folder);
    if !metadata_path.exists() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(&metadata_path)
        .with_context(|| format!("Failed to read {}", metadata_path.display()))?;
    parse_metadata_json(&raw).map(Some)
}

fn read_language_field_from_metadata(folder: &Path, field: &str) -> Result<Option<String>> {
    let Some(value) = read_metadata_value(folder)? else {
        return Ok(None);
    };

    let Some(language) = value.get(field) else {
        return Ok(None);
    };

    match language.as_str().map(str::trim).filter(|s| !s.is_empty()) {
        Some(code) => Ok(Some(normalise_supported_summary_language(code)?)),
        None => Ok(None),
    }
}

fn write_language_field_to_metadata(
    folder: &Path,
    field: &str,
    summary_language: Option<&str>,
) -> Result<()> {
    let normalised = summary_language.map(normalise_supported_summary_language).transpose()?;
    write_metadata_fields(folder, &[(field, normalised.map(Value::String))])
}

/// Sets each field to its value, or removes it for None, in one locked read-modify-write.
fn write_metadata_fields(folder: &Path, fields: &[(&str, Option<Value>)]) -> Result<()> {
    let _guard = METADATA_WRITE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let metadata_path = metadata_path(folder);
    let temp_path = metadata_temp_path(folder);

    let mut value = read_metadata_value(folder)?.unwrap_or_else(|| Value::Object(serde_json::Map::new()));

    if !value.is_object() {
        bail!("Failed to parse metadata.json: root value must be a JSON object");
    }

    let object = value.as_object_mut().expect("metadata value checked as object");
    for (field, field_value) in fields {
        match field_value {
            Some(v) => {
                object.insert(field.to_string(), v.clone());
            }
            None => {
                object.remove(*field);
            }
        }
    }

    let json_string = serde_json::to_string_pretty(&value)
        .context("Failed to serialize metadata.json")?;
    std::fs::write(&temp_path, json_string)
        .with_context(|| format!("Failed to write {}", temp_path.display()))?;
    std::fs::rename(&temp_path, &metadata_path).with_context(|| {
        format!(
            "Failed to replace {} with {}",
            metadata_path.display(),
            temp_path.display()
        )
    })?;

    Ok(())
}

fn metadata_path(folder: &Path) -> PathBuf {
    folder.join(METADATA_FILE)
}

fn metadata_temp_path(folder: &Path) -> PathBuf {
    folder.join(format!(
        "{}{}.tmp",
        METADATA_TEMP_FILE_PREFIX,
        uuid::Uuid::new_v4()
    ))
}

fn parse_metadata_json(raw: &str) -> Result<Value> {
    serde_json::from_str(raw).context("Failed to parse metadata.json")
}

fn normalise_supported_summary_language(raw: &str) -> Result<String> {
    let code = raw.trim().to_ascii_lowercase().replace('_', "-");
    if code.is_empty() {
        bail!("Unsupported summary language: empty");
    }

    if language_name_from_code(&code).is_none() {
        bail!("Unsupported summary language: {}", raw);
    }

    Ok(match code.as_str() {
        "zh-tw" => code,
        "zh-cn" => "zh".to_string(),
        other => other.split('-').next().unwrap_or(other).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summary_language_missing_field_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("metadata.json"),
            serde_json::to_string_pretty(&json!({
                "version": "1.0",
                "meeting_id": "meeting-123"
            }))
            .unwrap(),
        )
        .unwrap();

        assert_eq!(read_summary_language_from_metadata(dir.path()).unwrap(), None);
    }

    #[test]
    fn summary_language_write_preserves_existing_metadata_fields() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("metadata.json"),
            serde_json::to_string_pretty(&json!({
                "version": "1.0",
                "meeting_id": "meeting-123",
                "meeting_name": "Design Review"
            }))
            .unwrap(),
        )
        .unwrap();

        write_summary_language_to_metadata(dir.path(), Some("fr")).unwrap();

        let raw = std::fs::read_to_string(dir.path().join("metadata.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["version"], "1.0");
        assert_eq!(parsed["meeting_id"], "meeting-123");
        assert_eq!(parsed["meeting_name"], "Design Review");
        assert_eq!(parsed["summary_language"], "fr");
    }

    #[test]
    fn detected_summary_language_is_stored_separately_from_user_override() {
        let dir = tempfile::tempdir().unwrap();

        write_detected_summary_language_to_metadata(dir.path(), Some("es")).unwrap();
        write_summary_language_to_metadata(dir.path(), Some("fr")).unwrap();

        assert_eq!(
            read_detected_summary_language_from_metadata(dir.path()).unwrap(),
            Some("es".to_string())
        );
        assert_eq!(
            read_summary_language_from_metadata(dir.path()).unwrap(),
            Some("fr".to_string())
        );

        write_summary_language_to_metadata(dir.path(), None).unwrap();

        assert_eq!(
            read_detected_summary_language_from_metadata(dir.path()).unwrap(),
            Some("es".to_string())
        );
        assert_eq!(read_summary_language_from_metadata(dir.path()).unwrap(), None);
    }

    #[test]
    fn concurrent_summary_language_writes_preserve_both_fields() {
        for _ in 0..50 {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(
                dir.path().join("metadata.json"),
                serde_json::to_string_pretty(&json!({
                    "version": "1.0",
                    "meeting_id": "meeting-123",
                    "meeting_name": "Design Review",
                    "padding": "x".repeat(8192)
                }))
                .unwrap(),
            )
            .unwrap();

            let summary_dir = dir.path().to_path_buf();
            let detected_dir = dir.path().to_path_buf();

            let summary_thread = std::thread::spawn(move || {
                write_summary_language_to_metadata(&summary_dir, Some("fr")).unwrap();
            });
            let detected_thread = std::thread::spawn(move || {
                write_detected_summary_language_to_metadata(&detected_dir, Some("es")).unwrap();
            });

            summary_thread.join().unwrap();
            detected_thread.join().unwrap();

            let raw = std::fs::read_to_string(dir.path().join("metadata.json")).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(parsed["summary_language"], "fr");
            assert_eq!(parsed["detected_summary_language"], "es");
            assert_eq!(parsed["meeting_name"], "Design Review");
        }
    }

    #[test]
    fn summary_language_temp_path_is_unique_per_write() {
        let dir = tempfile::tempdir().unwrap();

        let first = metadata_temp_path(dir.path());
        let second = metadata_temp_path(dir.path());

        assert_ne!(first, second);
        assert_eq!(first.parent(), Some(dir.path()));
        assert_eq!(second.parent(), Some(dir.path()));
    }

    #[test]
    fn summary_language_null_removes_existing_field() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("metadata.json"),
            serde_json::to_string_pretty(&json!({
                "version": "1.0",
                "summary_language": "de"
            }))
            .unwrap(),
        )
        .unwrap();

        write_summary_language_to_metadata(dir.path(), None).unwrap();

        let raw = std::fs::read_to_string(dir.path().join("metadata.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(parsed.get("summary_language").is_none());
    }

    #[test]
    fn summary_language_rejects_unsupported_code() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("metadata.json"), "{}").unwrap();

        let err = write_summary_language_to_metadata(dir.path(), Some("xx")).unwrap_err();

        assert!(err.to_string().contains("Unsupported summary language"));
    }

    #[test]
    fn summary_language_malformed_metadata_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("metadata.json"), "{").unwrap();

        let err = read_summary_language_from_metadata(dir.path()).unwrap_err();

        assert!(err.to_string().contains("Failed to parse metadata.json"));
    }

    #[test]
    fn template_choice_round_trips_and_keeps_summary_language() {
        let dir = tempfile::tempdir().unwrap();
        write_summary_language_to_metadata(dir.path(), Some("fr")).unwrap();

        write_template_choice_to_metadata(dir.path(), Some(("german_lessons", TemplateSource::Auto))).unwrap();

        assert_eq!(
            read_template_choice_from_metadata(dir.path()).unwrap(),
            Some(("german_lessons".to_string(), TemplateSource::Auto))
        );
        assert_eq!(read_summary_language_from_metadata(dir.path()).unwrap(), Some("fr".to_string()));
    }

    #[test]
    fn template_choice_none_removes_both_fields() {
        let dir = tempfile::tempdir().unwrap();
        write_template_choice_to_metadata(dir.path(), Some(("standard_meeting", TemplateSource::Manual))).unwrap();

        write_template_choice_to_metadata(dir.path(), None).unwrap();

        let raw = std::fs::read_to_string(dir.path().join("metadata.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(parsed.get("summary_template").is_none());
        assert!(parsed.get("summary_template_source").is_none());
        assert_eq!(read_template_choice_from_metadata(dir.path()).unwrap(), None);
    }

    #[test]
    fn template_choice_with_unknown_source_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("metadata.json"),
            json!({ "summary_template": "standard_meeting", "summary_template_source": "weird" }).to_string(),
        )
        .unwrap();

        assert_eq!(read_template_choice_from_metadata(dir.path()).unwrap(), None);
    }
}
