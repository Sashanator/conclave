use crate::summary::templates::{self, Template};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct TemplateInfo {
    pub id: String,
    pub name: String,
    pub use_when: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TemplateListResponse {
    pub templates: Vec<TemplateInfo>,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn api_list_templates() -> Result<TemplateListResponse, String> {
    let listing = templates::list_templates();
    Ok(TemplateListResponse {
        templates: listing.templates.into_iter().map(|e| TemplateInfo {
            language: e.template.fixed_language().map(str::to_string),
            id: e.id,
            name: e.template.name,
            use_when: e.template.use_when,
        }).collect(),
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
