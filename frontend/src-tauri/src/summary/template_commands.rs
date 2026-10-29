use crate::summary::templates::{self, Template, TemplateEntry};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct TemplateInfo {
    pub id: String,
    pub name: String,
    pub use_when: Option<String>,
    pub language: Option<String>,
    /// True when any title keyword is non-blank (with an empty use_when, "auto-pick off" is then hidden).
    pub has_keywords: bool,
}

#[derive(Debug, Serialize)]
pub struct TemplateListResponse {
    pub templates: Vec<TemplateInfo>,
    pub errors: Vec<String>,
}

fn template_info(entry: TemplateEntry) -> TemplateInfo {
    TemplateInfo {
        language: entry.template.fixed_language().map(str::to_string),
        has_keywords: entry.template.title_keywords.iter().any(|k| !k.trim().is_empty()),
        id: entry.id,
        name: entry.template.name,
        use_when: entry.template.use_when,
    }
}

#[tauri::command]
pub async fn api_list_templates() -> Result<TemplateListResponse, String> {
    let listing = templates::list_templates();
    Ok(TemplateListResponse {
        templates: listing.templates.into_iter().map(template_info).collect(),
        errors: listing.errors,
    })
}

#[tauri::command]
pub async fn api_get_template(template_id: String) -> Result<Template, String> {
    templates::get_template(&template_id)
}

#[tauri::command]
pub async fn api_save_template(template_id: Option<String>, template: Template) -> Result<String, String> {
    templates::save_template(template_id.as_deref(), &template)
}

#[tauri::command]
pub async fn api_delete_template(template_id: String) -> Result<(), String> {
    templates::delete_template(&template_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(use_when: Option<&str>, keywords: &[&str], language: Option<&str>) -> TemplateEntry {
        TemplateEntry {
            id: "t1".into(),
            template: Template {
                name: "Deutsch".into(),
                description: String::new(),
                use_when: use_when.map(String::from),
                title_keywords: keywords.iter().map(|k| k.to_string()).collect(),
                language: language.map(String::from),
                context: None,
                prep: None,
                sections: vec![],
                style: None,
            },
        }
    }

    #[test]
    fn maps_fields_and_reports_only_non_blank_keywords() {
        let info = template_info(entry(Some("German lessons"), &[" ", "Deutsch"], Some("de")));
        assert_eq!((info.id.as_str(), info.name.as_str()), ("t1", "Deutsch"));
        assert_eq!(info.use_when.as_deref(), Some("German lessons"));
        assert_eq!(info.language.as_deref(), Some("de"));
        assert!(info.has_keywords);

        assert!(!template_info(entry(None, &["", "  "], None)).has_keywords);
        assert!(!template_info(entry(None, &[], Some("auto"))).has_keywords);
        assert_eq!(template_info(entry(None, &[], Some("auto"))).language, None);
    }
}
