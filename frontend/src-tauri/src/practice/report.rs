use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::PRON_MAX_HINTS;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum ItemKind {
    Grammar,
    Unnatural,
    Clarity,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReportItem {
    pub kind: ItemKind,
    pub turn: i64,
    pub quote: String,
    pub better: String,
    pub note: String,
    #[serde(default)]
    pub echoed: bool,
    /// A short lowercase key for the kind of mistake, grammar and unnatural items only.
    #[serde(default)]
    pub pattern: Option<String>,
}

/// A word the local pronunciation check may have got wrong. Times are relative to the start of the turn's audio.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PronHint {
    pub turn: i64,
    pub word: String,
    pub start_secs: f64,
    pub end_secs: f64,
    pub score: f32,
    /// The phone with the worst score in the word, e.g. "TH".
    pub phone: String,
}

/// Keeps the worst-scoring hint for each word (case-insensitive), lowest score first, at most `PRON_MAX_HINTS`.
pub fn select_hints(all: Vec<PronHint>) -> Vec<PronHint> {
    let mut best: BTreeMap<String, PronHint> = BTreeMap::new();
    for hint in all {
        let key = hint.word.to_lowercase();
        if best.get(&key).is_none_or(|kept| hint.score < kept.score) {
            best.insert(key, hint);
        }
    }
    let mut out: Vec<PronHint> = best.into_values().collect();
    out.sort_by(|a, b| a.score.total_cmp(&b.score));
    out.truncate(PRON_MAX_HINTS);
    out
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Fluency {
    pub fillers: BTreeMap<String, u32>,
    pub filler_total: u32,
    pub words: u32,
    pub speech_secs: f64,
    pub wpm: f64,
    pub long_pauses: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Report {
    pub version: u32, // 2; version 1 reports read back with empty pronunciation and no note
    pub fluency: Fluency,
    pub items: Vec<ReportItem>,
    pub dropped_items: u32,
    #[serde(default)]
    pub pronunciation: Vec<PronHint>,
    #[serde(default)]
    pub pronunciation_note: Option<String>,
}

pub const FILLERS: [&str; 11] = [
    "um",
    "uh",
    "er",
    "erm",
    "hmm",
    "like",
    "you know",
    "so basically",
    "i mean",
    "kind of",
    "sort of",
];

/// Lowercased word tokens (letters, digits, apostrophes) with the text that separates each from the previous one.
fn tokens(text: &str) -> Vec<(String, &str)> {
    let mut out = Vec::new();
    let mut prev_end = 0;
    let mut start: Option<usize> = None;
    for (i, c) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        let is_word = c.is_alphanumeric() || c == '\'' || c == '\u{2019}';
        match (is_word, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((
                    text[s..i].to_lowercase().replace('\u{2019}', "'"),
                    &text[prev_end..s],
                ));
                prev_end = i;
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// Whether filler phrase `f` starts at token `i`, applying the "like" and "kind of" rules.
fn filler_at(toks: &[(String, &str)], i: usize, f: &str) -> bool {
    let words: Vec<&str> = f.split(' ').collect();
    let end = i + words.len();
    if end > toks.len() || !words.iter().zip(&toks[i..end]).all(|(w, tok)| tok.0 == *w) {
        return false;
    }
    match f {
        "like" => {
            toks[i].1.contains(',') || toks.get(end).is_some_and(|(_, sep)| sep.contains(','))
        }
        "kind of" | "sort of" => !toks
            .get(end)
            .is_some_and(|(t, _)| matches!(t.as_str(), "a" | "an" | "the")),
        _ => true,
    }
}

pub fn count_fillers(text: &str) -> BTreeMap<String, u32> {
    let toks = tokens(text);
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < toks.len() {
        match FILLERS.iter().find(|f| filler_at(&toks, i, f)) {
            Some(f) => {
                *out.entry(f.to_string()).or_insert(0) += 1;
                i += f.split(' ').count();
            }
            None => i += 1,
        }
    }
    out
}

pub fn word_count(text: &str) -> u32 {
    tokens(text).len() as u32
}

pub fn wpm(words: u32, speech_secs: f64) -> f64 {
    if speech_secs < 1.0 {
        0.0
    } else {
        words as f64 * 60.0 / speech_secs
    }
}

pub fn long_pauses(segments: &[(f64, f64)], min_gap: f64) -> u32 {
    segments
        .windows(2)
        .filter(|w| w[1].0 - w[0].1 >= min_gap)
        .count() as u32
}

/// Whisper-style loops: one word six times in a row, or a 3-word sequence four times back to back.
pub fn looks_looped(text: &str) -> bool {
    let words: Vec<String> = tokens(text).into_iter().map(|(t, _)| t).collect();
    let mut run = 0;
    let mut prev: Option<&String> = None;
    for w in &words {
        run = if prev == Some(w) { run + 1 } else { 1 };
        if run >= 6 {
            return true;
        }
        prev = Some(w);
    }
    // Consecutive only: a long answer may reuse a phrase in separate places.
    (0..words.len().saturating_sub(2)).any(|i| {
        let seq = &words[i..i + 3];
        let repeats = (1..)
            .take_while(|k| words.get(i + 3 * k..i + 3 * k + 3) == Some(seq))
            .count();
        repeats + 1 >= 4
    })
}

/// Lowercase, letters/digits/apostrophes only, outer apostrophes trimmed, single spaces.
pub fn normalise(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '\u{2019}' => '\'',
            c if c.is_alphanumeric() || c == '\'' => c,
            _ => ' ',
        })
        .collect();
    cleaned
        .to_lowercase()
        .split_whitespace()
        .map(|w| w.trim_matches('\''))
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Drops any item whose quote is not a run of whole words in the learner's own turn.
pub fn validate_items(
    items: Vec<ReportItem>,
    learner_turns: &BTreeMap<i64, String>,
) -> (Vec<ReportItem>, u32) {
    let mut kept = Vec::new();
    let mut dropped = 0;
    for item in items {
        let grounded = learner_turns.get(&item.turn).is_some_and(|turn| {
            let quote = normalise(&item.quote);
            !quote.is_empty() && format!(" {} ", normalise(turn)).contains(&format!(" {} ", quote))
        });
        if grounded {
            kept.push(item);
        } else {
            dropped += 1;
        }
    }
    (kept, dropped)
}

pub fn schema() -> Value {
    let item = |grammar: bool, keyed: bool| {
        let mut props = json!({
            "turn": {"type": "integer"},
            "quote": {"type": "string"},
            "better": {"type": "string"},
            "note": {"type": "string"}
        });
        let mut required = vec!["turn", "quote", "better", "note"];
        if grammar {
            props["echoed"] = json!({"type": "boolean"});
            required.push("echoed");
        }
        if keyed {
            props["pattern"] = json!({"type": "string"});
            required.push("pattern");
        }
        json!({"type": "object", "additionalProperties": false, "required": required, "properties": props})
    };
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["grammar", "unnatural", "clarity"],
        "properties": {
            "grammar": {"type": "array", "items": item(true, true)},
            "unnatural": {"type": "array", "items": item(false, true)},
            "clarity": {"type": "array", "items": item(false, false)}
        }
    })
}

#[derive(Deserialize)]
struct RawItem {
    turn: i64,
    quote: String,
    better: String,
    note: String,
    #[serde(default)]
    echoed: bool,
    #[serde(default)]
    pattern: Option<String>,
}

/// A pattern key: normalised, 2 to 5 words and at most 60 characters. Anything else is no key.
pub fn pattern_key(raw: &str) -> Option<String> {
    let key = normalise(raw);
    let words = key.split_whitespace().count();
    ((2..=5).contains(&words) && key.chars().count() <= 60).then_some(key)
}

pub fn items_from_json(v: &Value) -> Result<Vec<ReportItem>, String> {
    let mut out = Vec::new();
    for (key, kind) in [
        ("grammar", ItemKind::Grammar),
        ("unnatural", ItemKind::Unnatural),
        ("clarity", ItemKind::Clarity),
    ] {
        let arr = v
            .get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| format!("report is missing the {key} array"))?;
        for item in arr {
            let raw = RawItem::deserialize(item).map_err(|e| format!("bad {key} item: {e}"))?;
            let pattern = raw.pattern.and_then(|p| pattern_key(&p));
            out.push(ReportItem {
                kind: kind.clone(),
                turn: raw.turn,
                quote: raw.quote,
                better: raw.better,
                note: raw.note,
                echoed: key == "grammar" && raw.echoed,
                pattern: pattern.filter(|_| key != "clarity"),
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fillers_count_whole_words_only() {
        assert!(count_fillers("umbrella").is_empty());
        assert!(count_fillers("I like it").is_empty());
        assert_eq!(count_fillers("so, like, maybe").get("like"), Some(&1));
        assert_eq!(count_fillers("like, I went").get("like"), Some(&1));
        let m = count_fillers("Um, I went, uh, home. Um.");
        assert_eq!(m.get("um"), Some(&2));
        assert_eq!(m.get("uh"), Some(&1));
    }

    #[test]
    fn kind_of_before_article_is_not_a_filler() {
        assert!(count_fillers("it was kind of a mess").is_empty());
        assert!(count_fillers("it was sort of the best").is_empty());
        assert_eq!(
            count_fillers("it was kind of good").get("kind of"),
            Some(&1)
        );
        assert_eq!(count_fillers("sort of fine").get("sort of"), Some(&1));
    }

    #[test]
    fn wpm_and_pauses() {
        assert_eq!(wpm(120, 60.0), 120.0);
        assert_eq!(wpm(10, 0.5), 0.0);
        assert_eq!(long_pauses(&[(0.0, 2.0), (3.5, 5.0), (5.2, 6.0)], 1.0), 1);
        assert_eq!(long_pauses(&[(0.0, 2.0)], 1.0), 0);
        // a gap exactly equal to min_gap counts
        assert_eq!(long_pauses(&[(0.0, 1.0), (2.0, 3.0)], 1.0), 1);
    }

    #[test]
    fn loop_detector() {
        assert!(looks_looped("the the the the the the"));
        assert!(!looks_looped(
            "I think we should ship it today, and then review the design next week"
        ));
    }

    #[test]
    fn invented_quotes_are_dropped() {
        let mut turns = BTreeMap::new();
        turns.insert(2, "I think it\u{2019}s a good idea, honestly.".to_string());
        let item = |turn: i64, quote: &str| ReportItem {
            kind: ItemKind::Clarity,
            turn,
            quote: quote.to_string(),
            better: String::new(),
            note: String::new(),
            echoed: false,
            pattern: None,
        };
        let items = vec![
            item(2, "good idea"),
            item(2, "I THINK it's a good idea honestly"),
            item(2, "we should go"),
            item(1, "good idea"),
        ];
        let (kept, dropped) = validate_items(items, &turns);
        assert_eq!(kept.len(), 2);
        assert_eq!(dropped, 2);
    }

    #[test]
    fn items_from_json_round_trip() {
        let v = json!({
            "grammar": [{"turn": 1, "quote": "I has", "better": "I have", "note": "agreement", "echoed": true}],
            "unnatural": [{"turn": 3, "quote": "make a decision", "better": "decide", "note": "wordy"}],
            "clarity": [{"turn": 5, "quote": "it", "better": "the plan", "note": "vague"}]
        });
        let items = items_from_json(&v).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].kind, ItemKind::Grammar);
        assert!(items[0].echoed);
        assert_eq!(items[1].kind, ItemKind::Unnatural);
        assert!(!items[1].echoed);
        assert_eq!(items[2].kind, ItemKind::Clarity);
        assert!(!items[2].echoed);
        assert!(items_from_json(&json!({"grammar": [], "unnatural": []})).is_err());
    }

    #[test]
    fn schema_requires_echoed_only_on_grammar() {
        let s = schema();
        assert_eq!(s["required"], json!(["grammar", "unnatural", "clarity"]));
        let g = &s["properties"]["grammar"]["items"];
        assert!(g["required"].as_array().unwrap().contains(&json!("echoed")));
        let u = &s["properties"]["unnatural"]["items"];
        assert!(!u["properties"].as_object().unwrap().contains_key("echoed"));
    }

    #[test]
    fn quote_must_match_whole_words() {
        let mut turns = BTreeMap::new();
        turns.insert(1, "He walked to work.".to_string());
        let item = |quote: &str| ReportItem {
            kind: ItemKind::Clarity,
            turn: 1,
            quote: quote.to_string(),
            better: String::new(),
            note: String::new(),
            echoed: false,
            pattern: None,
        };
        let items = vec![item("he walk"), item("He walked to work"), item("to work")];
        let (kept, dropped) = validate_items(items, &turns);
        assert_eq!(dropped, 1);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].quote, "He walked to work");
    }

    #[test]
    fn looped_only_when_consecutive() {
        assert!(looks_looped(
            "and then we and then we and then we and then we"
        ));
        assert!(!looks_looped("and then we and then we and then we"));
        assert!(!looks_looped(
            "It was a lot of work, then a lot of stress, later a lot of time, and in the end a lot of fun."
        ));
    }

    #[test]
    fn normalise_trims_outer_apostrophes() {
        assert_eq!(normalise("'good idea'"), "good idea");
        assert_eq!(normalise("Don\u{2019}t stop"), "don't stop");
    }

    #[test]
    fn v1_report_json_still_parses() {
        let v1 = r#"{"version":1,"fluency":{"fillers":{},"filler_total":0,"words":0,"speech_secs":0.0,"wpm":0.0,"long_pauses":0},"items":[{"kind":"Grammar","turn":1,"quote":"I has","better":"I have","note":"agreement","echoed":true}],"dropped_items":0}"#;
        let report: Report = serde_json::from_str(v1).unwrap();
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].pattern, None);
        assert!(report.pronunciation.is_empty());
        assert_eq!(report.pronunciation_note, None);
    }

    #[test]
    fn select_hints_dedupes_sorts_and_caps() {
        let hint = |turn: i64, word: &str, score: f32| PronHint {
            turn,
            word: word.to_string(),
            start_secs: 0.0,
            end_secs: 0.5,
            score,
            phone: "TH".to_string(),
        };
        let all = vec![
            hint(1, "Think", -8.0),
            hint(2, "think", -9.5), // the same word: the lower score is kept
            hint(3, "lunch", -7.0),
            hint(4, "worked", -6.8),
            hint(5, "sixth", -7.2),
            hint(6, "seventh", -7.5),
            hint(7, "eighth", -6.9),
            hint(8, "ninth", -6.6),
            hint(9, "tenth", -7.1),
        ];
        let words: Vec<String> = select_hints(all).into_iter().map(|h| h.word).collect();
        assert_eq!(words, ["think", "seventh", "sixth", "tenth", "lunch", "eighth"]);
    }

    #[test]
    fn pattern_keys_are_normalised_and_bounded() {
        assert_eq!(pattern_key("Third-Person  S!").as_deref(), Some("third person s"));
        assert_eq!(pattern_key("  make a decision ").as_deref(), Some("make a decision"));
        assert_eq!(pattern_key("tense"), None, "one word");
        assert_eq!(pattern_key("one two three four five six"), None, "six words");
        assert_eq!(pattern_key("!!"), None, "empty after normalising");
        let sixty = format!("{} {}", "a".repeat(29), "b".repeat(30));
        assert_eq!(pattern_key(&sixty).map(|k| k.chars().count()), Some(60));
        let too_long = format!("{} {}", "a".repeat(30), "b".repeat(30));
        assert_eq!(pattern_key(&too_long), None, "61 characters");
    }

    #[test]
    fn items_from_json_bounds_pattern_keys() {
        let item = |pattern: &str| json!({"turn": 1, "quote": "I has", "better": "I have", "note": "n", "echoed": false, "pattern": pattern});
        let v = json!({
            "grammar": [item("  Subject-Verb Agreement "), item("!!"), item("agreement")],
            "unnatural": [{"turn": 1, "quote": "I has", "better": "I have", "note": "n", "pattern": "one two three four five six"}],
            "clarity": [{"turn": 1, "quote": "I has", "better": "I have", "note": "n", "pattern": "third person s"}]
        });
        let items = items_from_json(&v).unwrap();
        let patterns: Vec<Option<&str>> = items.iter().map(|i| i.pattern.as_deref()).collect();
        assert_eq!(patterns, [Some("subject verb agreement"), None, None, None, None]);
    }

    #[test]
    fn schema_requires_pattern_on_grammar_and_unnatural() {
        let s = schema();
        for key in ["grammar", "unnatural"] {
            let item = &s["properties"][key]["items"];
            assert!(item["required"].as_array().unwrap().contains(&json!("pattern")), "{key}");
            assert_eq!(item["properties"]["pattern"]["type"], json!("string"), "{key}");
        }
        let clarity = &s["properties"]["clarity"]["items"];
        assert!(!clarity["properties"].as_object().unwrap().contains_key("pattern"));
    }
}
