// Sentence timing from Whisper token times. Sentences let a line be split between speakers.
// Chunk-local times are seconds into the chunk; meeting times add the chunk's start.

use serde::{Deserialize, Serialize};

/// `[t0, t1, text]` in seconds, stored as the JSON array of the `transcripts.sentences` column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sentence(pub f64, pub f64, pub String);

/// A Whisper token: its raw bytes (may split a UTF-8 character) and centisecond times.
pub struct TimedToken {
    pub bytes: Vec<u8>,
    pub t0: i64,
    pub t1: i64,
}

/// Tolerance in seconds for token times that fall just outside the line's audio.
const TOLERANCE_SECS: f64 = 0.2;

/// Cuts a sentence after a token that ends the accumulated text in `.`, `?`, `!` or `…`.
pub fn build_sentences(tokens: &[TimedToken], pad_secs: f64, audio_secs: f64) -> Vec<Sentence> {
    let mut out = Vec::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut span: Option<(i64, i64)> = None;
    for tok in tokens {
        bytes.extend_from_slice(&tok.bytes);
        span = Some(span.map_or((tok.t0, tok.t1), |(t0, _)| (t0, tok.t1)));
        if ends_sentence(&bytes) {
            push_sentence(&mut out, &bytes, span, pad_secs, audio_secs);
            bytes.clear();
            span = None;
        }
    }
    push_sentence(&mut out, &bytes, span, pad_secs, audio_secs);
    out
}

fn ends_sentence(bytes: &[u8]) -> bool {
    // `trim_ascii_end` needs Rust 1.80; the crate supports 1.77
    let end = bytes.iter().rposition(|b| !b.is_ascii_whitespace()).map_or(0, |i| i + 1);
    let trimmed = &bytes[..end];
    matches!(trimmed.last(), Some(b'.' | b'?' | b'!')) || trimmed.ends_with("…".as_bytes())
}

fn push_sentence(out: &mut Vec<Sentence>, bytes: &[u8], span: Option<(i64, i64)>, pad_secs: f64, audio_secs: f64) {
    let text = String::from_utf8_lossy(bytes).trim().to_string();
    let Some((t0, t1)) = span.filter(|_| !text.is_empty()) else { return };
    let secs = |t: i64| round2((t as f64 / 100.0 - pad_secs).clamp(0.0, audio_secs));
    out.push(Sentence(secs(t0), secs(t1), text));
}

/// Shifts chunk-local sentences to meeting time. None unless they pass `is_valid`.
pub fn to_meeting_time(local: &[Sentence], line_text: &str, start: f64, end: f64) -> Option<Vec<Sentence>> {
    let shifted: Vec<Sentence> = local
        .iter()
        .map(|Sentence(t0, t1, text)| Sentence(round2(start + t0), round2(start + t1), text.clone()))
        .collect();
    if !is_valid(&shifted, line_text, start, end) {
        return None;
    }
    Some(
        shifted
            .into_iter()
            .map(|Sentence(t0, t1, text)| Sentence(t0.max(start).min(end), t1.max(start).min(end), text))
            .collect(),
    )
}

/// Meeting-time sentences must join to the line text, and be ordered, non-overlapping and inside the line.
/// Comparisons are written so that NaN fails them.
pub fn is_valid(s: &[Sentence], line_text: &str, start: f64, end: f64) -> bool {
    let texts: Vec<&str> = s.iter().map(|Sentence(_, _, text)| text.as_str()).collect();
    if collapse(&texts.join(" ")) != collapse(line_text) {
        return false;
    }
    let (lo, hi) = (start - TOLERANCE_SECS, end + TOLERANCE_SECS);
    let mut prev_end = f64::NEG_INFINITY;
    s.iter().all(|Sentence(t0, t1, _)| {
        let ok = *t0 <= *t1 && *t0 >= prev_end && *t0 >= lo && *t1 <= hi;
        prev_end = *t1;
        ok
    })
}

/// Parses the webview's JSON. Any malformed or non-finite entry rejects the whole payload.
pub fn parse_json(raw: &str) -> Option<Vec<Sentence>> {
    let sentences: Vec<Sentence> = serde_json::from_str(raw).ok()?;
    sentences.iter().all(|Sentence(t0, t1, _)| t0.is_finite() && t1.is_finite()).then_some(sentences)
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(s: &str, t0: i64, t1: i64) -> TimedToken {
        TimedToken { bytes: s.as_bytes().to_vec(), t0, t1 }
    }

    #[test]
    fn sentences_cut_after_terminal_punctuation() {
        let t = [tok(" Hello", 0, 40), tok(" there.", 40, 90), tok(" Yes?", 120, 160), tok(" ok", 170, 200)];
        assert_eq!(build_sentences(&t, 0.0, 2.0), vec![
            Sentence(0.0, 0.9, "Hello there.".into()), Sentence(1.2, 1.6, "Yes?".into()), Sentence(1.7, 2.0, "ok".into())]);
    }

    #[test]
    fn sentences_join_cyrillic_bytes_split_across_tokens() {
        let b = "Привет.".as_bytes();
        let t = [TimedToken { bytes: b[..3].to_vec(), t0: 0, t1: 10 }, TimedToken { bytes: b[3..].to_vec(), t0: 10, t1: 50 }];
        assert_eq!(build_sentences(&t, 0.0, 1.0), vec![Sentence(0.0, 0.5, "Привет.".into())]);
    }

    #[test]
    fn sentences_remove_padding_and_clamp() {
        // 0.3 s of padding before the real audio of 0.5 s
        let t = [tok(" Да.", 20, 90)];
        assert_eq!(build_sentences(&t, 0.3, 0.5), vec![Sentence(0.0, 0.5, "Да.".into())]);
    }

    #[test]
    fn sentences_ellipsis_cuts() {
        let t = [tok(" Ну…", 0, 50), tok(" Потом.", 60, 100)];
        assert_eq!(build_sentences(&t, 0.0, 1.0), vec![
            Sentence(0.0, 0.5, "Ну…".into()), Sentence(0.6, 1.0, "Потом.".into())]);
    }

    #[test]
    fn sentences_ellipsis_split_across_tokens_cuts() {
        let e = "…".as_bytes();
        let t = [
            tok(" Ну", 0, 50),
            TimedToken { bytes: e[..2].to_vec(), t0: 50, t1: 55 },
            TimedToken { bytes: e[2..].to_vec(), t0: 55, t1: 60 },
            tok(" Потом.", 60, 100),
        ];
        assert_eq!(build_sentences(&t, 0.0, 1.0), vec![
            Sentence(0.0, 0.6, "Ну…".into()), Sentence(0.6, 1.0, "Потом.".into())]);
    }

    #[test]
    fn sentences_to_meeting_time_offsets_and_checks_text() {
        let local = vec![Sentence(0.0, 0.9, "Hello there.".into()), Sentence(1.2, 1.6, "Yes?".into())];
        assert_eq!(to_meeting_time(&local, "Hello there. Yes?", 100.0, 102.0).unwrap()[1], Sentence(101.2, 101.6, "Yes?".into()));
        assert!(to_meeting_time(&local, "Hello there. Yes? Yes?", 100.0, 102.0).is_none()); // text was cleaned/changed
    }

    #[test]
    fn sentences_reject_unordered_or_out_of_range() {
        let bad = vec![Sentence(1.0, 0.5, "a.".into())];
        assert!(to_meeting_time(&bad, "a.", 0.0, 2.0).is_none());
        let late = vec![Sentence(0.0, 5.0, "a.".into())];
        assert!(to_meeting_time(&late, "a.", 0.0, 2.0).is_none()); // beyond end + 0.2 s tolerance
    }

    #[test]
    fn sentences_reject_overlap() {
        let s = vec![Sentence(0.0, 1.0, "a.".into()), Sentence(0.8, 1.5, "b.".into())];
        assert!(!is_valid(&s, "a. b.", 0.0, 2.0));
        let ok = vec![Sentence(0.0, 1.0, "a.".into()), Sentence(1.0, 1.5, "b.".into())];
        assert!(is_valid(&ok, "a. b.", 0.0, 2.0));
    }

    #[test]
    fn sentences_parse_json_rejects_garbage() {
        assert_eq!(parse_json(r#"[[1.0,2.0,"a."]]"#).unwrap().len(), 1);
        for raw in [r#"[[1.0,"x","a"]]"#, r#"{"a":1}"#, r#"[[NaN,1,"a"]]"#, "not json"] { assert!(parse_json(raw).is_none()); }
    }
}
