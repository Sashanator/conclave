use serde::{Deserialize, Serialize};

pub mod chat;
pub mod store;
pub mod upcoming;

pub(crate) const MAX_MATERIAL_CHARS: usize = 40_000;
pub(crate) const MAX_PREP_ITEMS: usize = 50;
pub(crate) const MAX_ITEM_CHARS: usize = 500;
pub(crate) const KINDS: [&str; 4] = ["question", "clarify", "check", "other"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrepItem {
    pub id: String,
    pub text: String,
    pub kind: String,
    pub ticked: bool,
    #[serde(default)]
    pub by_user: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: String,
    pub text: String,
    pub at: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub list_not_updated: bool,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProposedItem {
    pub text: String,
    #[serde(default)]
    pub kind: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingPrep {
    pub id: i64,
    pub event_uid: String,
    pub event_start: String,
    pub event_title: String,
    pub template_id: Option<String>,
    pub material: String,
    pub chat: Vec<ChatMessage>,
    pub items: Vec<PrepItem>,
    pub approved_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub(crate) fn count_label(count: usize, limit: usize) -> String {
    fn grouped(n: usize) -> String {
        let digits = n.to_string();
        let mut result = String::new();
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                result.push(',');
            }
            result.push(ch);
        }
        result
    }
    format!("{} / {} characters", grouped(count), grouped(limit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_label_groups_thousands() {
        assert_eq!(count_label(41_300, 40_000), "41,300 / 40,000 characters");
        assert_eq!(count_label(5, 999), "5 / 999 characters");
    }
}
