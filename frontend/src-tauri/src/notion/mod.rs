pub mod client;
pub mod mapping;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    pub id: String,
    pub name: String,
    pub colour: u8,
    pub position: i64,
    pub database_id: String,
    pub data_source_id: Option<String>,
    pub mapping: Mapping,
    #[serde(default)]
    pub last_status: Option<String>,
    #[serde(default)]
    pub last_message: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    pub title_prop: String,
    pub due_prop: Option<String>,
    pub meeting_prop: Option<String>,
    pub fixed: Vec<FixedValue>,
}

/// `value`: a string for select/status, a string array for multi_select, a bool for checkbox.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FixedValue {
    pub prop: String,
    pub kind: String,
    pub value: serde_json::Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DbSchema {
    pub database_id: String,
    pub data_source_id: Option<String>,
    pub title: String,
    pub properties: Vec<PropInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PropInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub options: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DestStatus {
    pub dest_id: String,
    pub state: String,
    pub message: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NotionConfig {
    pub token_last4: Option<String>,
    pub destinations: Vec<Destination>,
    pub category_defaults: Vec<CategoryDefault>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CategoryDefault {
    pub template_key: String,
    pub dest_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SendReport {
    pub sent: u32,
    pub failed: u32,
    pub ambiguous: u32,
}

/// States of a destination that stop sending to it.
pub(crate) const BLOCKING_STATES: [&str; 5] = ["not_shared", "not_found", "token_missing", "token_rejected", "mapping_broken"];
