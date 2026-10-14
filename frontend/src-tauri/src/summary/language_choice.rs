use super::processor::language_name_from_code;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LanguageChoice {
    pub code: String,
    pub name: &'static str,
    pub fallback: bool,
}

/// Spec §5.2: the meeting's own choice, then the template's fixed language, then the detected transcript
/// language; English only as a flagged last resort.
pub(crate) fn resolve_summary_language(
    per_meeting: Option<&str>,
    template: Option<&str>,
    detected: Option<&str>,
) -> LanguageChoice {
    [per_meeting, template, detected]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|c| !c.is_empty() && !c.eq_ignore_ascii_case("auto"))
        .find_map(|c| language_name_from_code(c).map(|name| LanguageChoice { code: c.to_lowercase(), name, fallback: false }))
        .unwrap_or(LanguageChoice { code: "en".into(), name: "English", fallback: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_is_meeting_then_template_then_detected() {
        assert_eq!(resolve_summary_language(Some("de"), Some("ru"), Some("en")).code, "de");
        assert_eq!(resolve_summary_language(None, Some("ru"), Some("en")).code, "ru");
        let detected = resolve_summary_language(None, None, Some("ru"));
        assert_eq!((detected.code.as_str(), detected.name, detected.fallback), ("ru", "Russian", false));
    }

    #[test]
    fn unsupported_or_blank_values_are_skipped() {
        assert_eq!(resolve_summary_language(Some("xx"), Some(" "), Some("ru")).code, "ru");
        assert_eq!(resolve_summary_language(Some(""), Some("auto"), Some("de")).code, "de");
    }

    #[test]
    fn nothing_known_falls_back_to_english_flagged() {
        let c = resolve_summary_language(None, None, None);
        assert_eq!((c.code.as_str(), c.fallback), ("en", true));
        assert!(!resolve_summary_language(Some("en"), None, None).fallback, "a chosen English is not a fallback");
    }
}
