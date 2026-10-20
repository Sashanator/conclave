//! Supertonic 3 text front-end: a port of the reference SDK's `UnicodeProcessor`
//! (supertonic 1.3.1, `core.py`) text preprocessing and unicode indexing.
// The TTS engine that consumes this module lands in a later task.
#![allow(dead_code)]

use regex::Regex;
use unicode_normalization::UnicodeNormalization;

lazy_static::lazy_static! {
    static ref EMOJI: Regex = Regex::new(
        r"[\x{1f600}-\x{1f64f}\x{1f300}-\x{1f5ff}\x{1f680}-\x{1f6ff}\x{1f700}-\x{1f77f}\x{1f780}-\x{1f7ff}\x{1f800}-\x{1f8ff}\x{1f900}-\x{1f9ff}\x{1fa00}-\x{1fa6f}\x{1fa70}-\x{1faff}\x{2600}-\x{26ff}\x{2700}-\x{27bf}\x{1f1e6}-\x{1f1ff}]+"
    ).unwrap();
    static ref SPECIAL: Regex = Regex::new(r"[♥☆♡©\\]").unwrap();
    // Python's str.isspace also covers U+001C..U+001F, which Rust's \s does not.
    static ref WHITESPACE: Regex = Regex::new(r"[\s\x1c-\x1f]+").unwrap();
}

const SYMBOL_REPLACEMENTS: &[(&str, &str)] = &[
    ("\u{2013}", "-"),
    ("\u{2011}", "-"),
    ("\u{2014}", "-"),
    ("\u{00af}", " "),
    ("_", " "),
    ("\u{201c}", "\""),
    ("\u{201d}", "\""),
    ("\u{2018}", "'"),
    ("\u{2019}", "'"),
    ("\u{00b4}", "'"),
    ("`", "'"),
    ("[", " "),
    ("]", " "),
    ("|", " "),
    ("/", " "),
    ("#", " "),
    ("\u{2192}", " "),
    ("\u{2190}", " "),
];

const PUNCTUATION_SPACING: &[(&str, &str)] = &[
    (" ,", ","),
    (" .", "."),
    (" !", "!"),
    (" ?", "?"),
    (" ;", ";"),
    (" :", ":"),
    (" '", "'"),
];

const ENDING_PUNCTUATION: &str = ".!?;:,'\"')]}…。」』】〉》›»";

/// Steps 1-5 of the reference: NFKD, emoji, symbols, decorative symbols, abbreviations.
fn map_chars(text: &str) -> String {
    let mut t: String = text.nfkd().collect();
    t = EMOJI.replace_all(&t, "").into_owned();
    for (old, new) in SYMBOL_REPLACEMENTS {
        t = t.replace(old, new);
    }
    t = SPECIAL.replace_all(&t, "").into_owned();
    t.replace('@', " at ")
        .replace("e.g.,", "for example, ")
        .replace("i.e.,", "that is, ")
}

/// Collapses runs of one quote character (`"`, `'`, backtick) to a single one.
fn collapse_quotes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev = None;
    for c in text.chars() {
        if matches!(c, '"' | '\'' | '`') && prev == Some(c) {
            continue;
        }
        out.push(c);
        prev = Some(c);
    }
    out
}

/// Full reference preprocessing (steps 1-10) wrapped as `<lang>...</lang>`.
pub fn preprocess(text: &str, lang: &str) -> String {
    let mut t = map_chars(text);
    for (pat, rep) in PUNCTUATION_SPACING {
        t = t.replace(pat, rep);
    }
    t = collapse_quotes(&t);
    t = WHITESPACE.replace_all(&t, " ").trim().to_string();
    if !t
        .chars()
        .next_back()
        .is_some_and(|c| ENDING_PUNCTUATION.contains(c))
    {
        t.push('.');
    }
    format!("<{lang}>{t}</{lang}>")
}

/// `unicode_indexer.json`: index = code point, value = token id or -1 (unsupported).
pub struct Indexer(Vec<i64>);

impl Indexer {
    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        let table: Vec<i64> = serde_json::from_slice(bytes)
            .map_err(|e| format!("unicode indexer is malformed: {e}"))?;
        if table.is_empty() {
            return Err("unicode indexer is empty".into());
        }
        Ok(Self(table))
    }

    fn id(&self, c: char) -> Option<i64> {
        self.0.get(c as usize).copied().filter(|&id| id >= 0)
    }

    /// Token ids for each supported char; unsupported or out-of-range chars are dropped.
    pub fn ids(&self, preprocessed: &str) -> Vec<i64> {
        preprocessed.chars().filter_map(|c| self.id(c)).collect()
    }
}

/// Maps the text (steps 1-5), then drops chars the model cannot speak.
/// Mapping runs first so that `@`, dashes and curly quotes survive as their
/// supported replacements. Logs only the number of dropped chars.
pub fn supported_only(text: &str, idx: &Indexer) -> String {
    let mapped = map_chars(text);
    let kept: String = mapped.chars().filter(|&c| idx.id(c).is_some()).collect();
    let dropped = mapped.chars().count() - kept.chars().count();
    if dropped > 0 {
        log::info!("practice tts: dropped {dropped} unsupported chars");
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/practice/supertonic_text.json");

    fn fixture() -> Vec<serde_json::Value> {
        serde_json::from_str(FIXTURE).unwrap()
    }

    #[test]
    fn preprocess_matches_reference() {
        for case in fixture() {
            let text = case["text"].as_str().unwrap();
            let pre = case["pre"].as_str().unwrap();
            assert_eq!(preprocess(text, "en"), pre, "input: {text:?}");
        }
    }

    #[test]
    fn ids_never_contain_minus_one() {
        let mut long = vec![-1i64; 0x1F601];
        long[97] = 5; // a
        long[98] = 6; // b
        let mut short = vec![-1i64; 99];
        short[97] = 5;
        short[98] = 6;
        for idx in [Indexer(long), Indexer(short)] {
            assert_eq!(idx.ids("a😀b"), vec![5, 6]);
        }
    }

    #[test]
    fn supported_only_maps_then_drops() {
        let mut v = vec![0i64; 128];
        v[120] = -1; // 'x' unsupported
        let idx = Indexer(v);
        assert_eq!(supported_only("a@b 😀—x", &idx), "a at b -");
    }

    #[test]
    fn from_json_rejects_empty_and_non_list() {
        assert!(Indexer::from_json(b"[]").is_err());
        assert!(Indexer::from_json(b"{}").is_err());
        assert_eq!(
            Indexer::from_json(b"[1,-1]").unwrap().ids("\u{0}\u{1}"),
            vec![1]
        );
    }

    #[test]
    #[ignore = "needs PRACTICE_MODELS_DIR/unicode_indexer.json"]
    fn ids_match_reference() {
        let dir = std::env::var("PRACTICE_MODELS_DIR").expect("set PRACTICE_MODELS_DIR");
        let bytes = std::fs::read(std::path::Path::new(&dir).join("unicode_indexer.json")).unwrap();
        let idx = Indexer::from_json(&bytes).unwrap();
        for case in fixture() {
            let pre = case["pre"].as_str().unwrap();
            let expected: Vec<i64> = case["ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap())
                .filter(|&id| id != -1)
                .collect();
            assert_eq!(idx.ids(pre), expected, "input: {pre:?}");
        }
    }
}
