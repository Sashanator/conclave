//! CMUdict parser and word tokeniser for the local pronunciation hints.
//! Phone ids follow the order of the HuBERT phoneme model's output head (ids 0..=38).

use std::collections::HashMap;

/// ARPAbet phones (stress-less CMUdict set), in model id order.
pub const PHONES: [&str; 39] = [
    "AA", "AE", "AH", "AO", "AW", "AY", "B", "CH", "D", "DH", "EH", "ER", "EY", "F", "G", "HH",
    "IH", "IY", "JH", "K", "L", "M", "N", "NG", "OW", "OY", "P", "R", "S", "SH", "T", "TH", "UH",
    "UW", "V", "W", "Y", "Z", "ZH",
];

/// Maps an ARPAbet phone, with or without a stress digit, to its model id.
pub fn phone_id(arpabet: &str) -> Option<u8> {
    let base = arpabet.trim_end_matches(|c: char| c.is_ascii_digit());
    PHONES.iter().position(|p| *p == base).map(|i| i as u8)
}

/// CMUdict pronunciations keyed by lowercase word. Each word keeps all its variants, in file order.
pub struct PronDict {
    map: HashMap<String, Vec<Vec<u8>>>,
}

impl PronDict {
    /// Lines are `WORD PHONES…` or `WORD(n) PHONES…`. `#` starts a comment, and `;` lines are CMUdict headers.
    /// A line with a phone outside the model set is skipped.
    pub fn parse(text: &str) -> PronDict {
        let mut map: HashMap<String, Vec<Vec<u8>>> = HashMap::new();
        for line in text.lines().filter(|l| !l.starts_with(';')) {
            let line = line.split('#').next().unwrap_or_default();
            let mut parts = line.split_whitespace();
            let Some(head) = parts.next() else { continue };
            let word = head.split('(').next().unwrap_or_default().to_lowercase();
            let phones: Option<Vec<u8>> = parts.map(phone_id).collect();
            if let Some(p) = phones.filter(|p| !p.is_empty()) {
                map.entry(word).or_default().push(p);
            }
        }
        PronDict { map }
    }

    pub fn variants(&self, word: &str) -> Option<&[Vec<u8>]> {
        self.map.get(&word.to_lowercase()).map(Vec::as_slice)
    }
}

/// Splits a transcript into lowercase words for dictionary lookup. Surrounding punctuation is dropped,
/// and internal apostrophes (including a curly one) are kept as ASCII `'`.
pub fn words(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || matches!(c, '-' | '\u{2013}' | '\u{2014}'))
        .map(|t| {
            t.trim_matches(|c: char| !c.is_alphanumeric())
                .replace('\u{2019}', "'")
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "READ  R IY1 D
read(2)  R EH1 D
# a comment line
THINK  TH IH1 NG K   # trailing comment
DON'T  D OW1 N T
BOGUS  XX IY1 D
";

    #[test]
    fn parses_variants_and_strips_stress_and_comments() {
        let dict = PronDict::parse(SAMPLE);
        assert_eq!(
            dict.variants("read"),
            Some(&[vec![27, 17, 8], vec![27, 10, 8]][..])
        );
        assert_eq!(dict.variants("think"), Some(&[vec![31, 16, 23, 19]][..]));
        assert_eq!(dict.variants("don't"), Some(&[vec![8, 24, 22, 30]][..]));
        assert_eq!(dict.variants("READ"), dict.variants("read"));
        assert_eq!(dict.variants("bogus"), None);
    }

    #[test]
    fn phone_ids_follow_the_model_order() {
        assert_eq!(phone_id("AA"), Some(0));
        assert_eq!(phone_id("ZH"), Some(38));
        assert_eq!(phone_id("AH0"), Some(2));
        assert_eq!(phone_id("AH1"), Some(2));
        assert_eq!(phone_id("XX"), None);
        for (i, p) in PHONES.iter().enumerate() {
            assert_eq!(phone_id(p), Some(i as u8));
        }
    }

    #[test]
    fn words_keep_contractions_and_strip_punctuation() {
        assert_eq!(
            words("Don't, I'm (sure) \u{2014} 2026!"),
            vec!["don't", "i'm", "sure", "2026"]
        );
        assert_eq!(
            words("well-known \u{2019}tis"),
            vec!["well", "known", "tis"]
        );
        assert_eq!(words("don\u{2019}t"), vec!["don't"]);
    }

    #[test]
    fn unknown_word_returns_none() {
        let dict = PronDict::parse(SAMPLE);
        assert_eq!(dict.variants("weber"), None);
    }
}
