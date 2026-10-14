use crate::summary::templates::TemplateEntry;
use serde::{Deserialize, Serialize};

const AUTO_PICK_TRANSCRIPT_CHARS: usize = 4_000;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TemplateSource { Manual, Keyword, Auto, Fallback }

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Choice { Decided(String, TemplateSource), AskModel }

/// Only a choice the user or a rule made is kept; a Fallback (model failed or cancelled) is re-decided next time.
pub(crate) fn choice_to_store(source: TemplateSource, cancelled: bool) -> bool {
    !cancelled && source != TemplateSource::Fallback
}

fn exists(id: &str, templates: &[TemplateEntry]) -> bool {
    templates.iter().any(|e| e.id == id)
}

/// Spec §5.1 steps 1-2 (manual, then the choice stored on the meeting, then title keywords); AskModel means step 3.
pub(crate) fn choose_without_model(
    manual: Option<&str>,
    stored: Option<(String, TemplateSource)>,
    title: &str,
    templates: &[TemplateEntry],
) -> Choice {
    if let Some(id) = manual.filter(|id| exists(id, templates)) {
        return Choice::Decided(id.to_string(), TemplateSource::Manual);
    }
    if let Some((id, source)) = stored.filter(|(id, _)| exists(id, templates)) {
        return Choice::Decided(id, source);
    }
    match templates.iter().find(|e| e.template.matches_title(title)) {
        Some(e) => Choice::Decided(e.id.clone(), TemplateSource::Keyword),
        None => Choice::AskModel,
    }
}

fn candidates(templates: &[TemplateEntry]) -> impl Iterator<Item = (&TemplateEntry, &str)> {
    templates.iter().filter_map(|e| {
        e.template.use_when.as_deref().map(str::trim).filter(|w| !w.is_empty()).map(|w| (e, w))
    })
}

pub(crate) fn auto_pick_prompts(transcript: &str, templates: &[TemplateEntry]) -> (String, String) {
    let system = "You choose which note template fits a meeting. Reply with exactly one id from the list and nothing else. \
                  If none fits well, reply with standard_meeting."
        .to_string();
    let list: String = candidates(templates)
        .map(|(e, use_when)| format!("- {}: {} — use when: {}\n", e.id, e.template.name, use_when))
        .collect();
    let start: String = transcript.chars().take(AUTO_PICK_TRANSCRIPT_CHARS).collect();
    let user = format!("Templates:\n{list}\n<transcript_start>\n{start}\n</transcript_start>\n\nAnswer with one id.");
    (system, user)
}

pub(crate) fn parse_auto_pick_answer(answer: &str, templates: &[TemplateEntry]) -> Option<String> {
    let id = answer.trim().trim_matches(|c| c == '`' || c == '"' || c == '\'').trim().trim_end_matches('.');
    candidates(templates).find(|(e, _)| e.id == id).map(|(e, _)| e.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::templates::{Template, TemplateEntry};

    fn entry(id: &str, name: &str, keywords: &[&str], use_when: Option<&str>) -> TemplateEntry {
        let mut t: Template = serde_json::from_str(&format!(r#"{{"name":"{name}","sections":[{{"title":"A","instruction":"B"}}]}}"#)).unwrap();
        t.title_keywords = keywords.iter().map(|k| k.to_string()).collect();
        t.use_when = use_when.map(String::from);
        TemplateEntry { id: id.into(), template: t }
    }

    fn all() -> Vec<TemplateEntry> {
        vec![
            entry("one_two_one_rasulov", "1-2-1 Расулов", &["1-2-1"], Some("1-on-1 with my HoD")),
            entry("german_lessons", "German Lessons", &["Deutsch"], Some("German lesson")),
            entry("no_hint", "No hint", &[], None),
            entry("standard_meeting", "Standard Meeting Notes", &[], Some("General meeting")),
        ]
    }

    #[test]
    fn manual_beats_stored_beats_keyword_beats_model() {
        let stored = Some(("german_lessons".to_string(), TemplateSource::Auto));
        assert_eq!(choose_without_model(Some("no_hint"), stored.clone(), "1-2-1 x", &all()),
                   Choice::Decided("no_hint".into(), TemplateSource::Manual));
        assert_eq!(choose_without_model(None, stored, "1-2-1 x", &all()),
                   Choice::Decided("german_lessons".into(), TemplateSource::Auto));
        assert_eq!(choose_without_model(None, None, "Meeting 1-2-1", &all()),
                   Choice::Decided("one_two_one_rasulov".into(), TemplateSource::Keyword));
        assert_eq!(choose_without_model(None, None, "Zoom Meeting 14_10_26", &all()), Choice::AskModel);
    }

    #[test]
    fn a_deleted_or_unknown_template_is_ignored_not_fatal() {
        let gone = Some(("deleted_one".to_string(), TemplateSource::Manual));
        assert_eq!(choose_without_model(Some("also_gone"), gone, "Zoom Meeting", &all()), Choice::AskModel);
    }

    #[test]
    fn auto_pick_lists_only_templates_with_use_when_and_trims_the_transcript() {
        let long = "я".repeat(10_000);
        let (system, user) = auto_pick_prompts(&long, &all());
        assert!(system.contains("one id"));
        assert!(user.contains("one_two_one_rasulov") && user.contains("1-on-1 with my HoD"));
        assert!(!user.contains("no_hint"));
        assert!(user.chars().filter(|c| *c == 'я').count() <= 4_000);
    }

    #[test]
    fn only_real_choices_of_an_uncancelled_run_are_stored() {
        assert!(choice_to_store(TemplateSource::Manual, false));
        assert!(choice_to_store(TemplateSource::Keyword, false));
        assert!(choice_to_store(TemplateSource::Auto, false));
        assert!(!choice_to_store(TemplateSource::Fallback, false));
        assert!(!choice_to_store(TemplateSource::Manual, true));
        assert!(!choice_to_store(TemplateSource::Auto, true));
    }

    #[test]
    fn answers_are_parsed_strictly() {
        let t = all();
        for ok in ["german_lessons", " german_lessons\n", "`german_lessons`", "\"german_lessons\"", "german_lessons."] {
            assert_eq!(parse_auto_pick_answer(ok, &t).as_deref(), Some("german_lessons"), "{ok:?}");
        }
        for bad in ["", "German Lessons", "Template: german_lessons", "no_hint", "german_lessons or standard_meeting"] {
            assert_eq!(parse_auto_pick_answer(bad, &t), None, "{bad:?}");
        }
    }
}
