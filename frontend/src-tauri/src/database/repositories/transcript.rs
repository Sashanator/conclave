use crate::api::TranscriptSegment;
use crate::database::repositories::meeting::TitleOrigin;
use crate::database::repositories::speakers::SpeakersRepository;
use crate::speakers::sentences::{is_valid, Sentence};
use crate::database::repositories::meeting_notes::{
    parse_notes, sanitize_mark, sanitize_notes, MeetingNote, MeetingNotesRepository,
};
use chrono::Utc;
use serde::Serialize;
use std::collections::HashMap;
use sqlx::{Connection, Error as SqlxError, SqliteConnection, SqlitePool};
use tracing::{error, info};
use uuid::Uuid;

pub struct TranscriptsRepository;

/// Inserts transcript lines as given. The caller owns the transaction and any error handling.
async fn insert_segments(conn: &mut SqliteConnection, meeting_id: &str, transcripts: &[TranscriptSegment]) -> Result<(), SqlxError> {
    for segment in transcripts {
        let transcript_id = format!("transcript-{}", Uuid::new_v4());
        // Sentences that do not match the saved line are dropped; the line itself still saves
        let sentences = match (&segment.sentences, segment.audio_start_time, segment.audio_end_time) {
            (Some(s), Some(start), Some(end)) if is_valid(s, &segment.text, start, end) => serde_json::to_string(s).ok(),
            _ => None,
        };
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, mark, speaker_idx, sentences)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&transcript_id)
        .bind(meeting_id)
        .bind(&segment.text)
        .bind(&segment.timestamp)
        .bind(segment.audio_start_time)
        .bind(segment.audio_end_time)
        .bind(segment.duration)
        .bind(sanitize_mark(segment.mark.as_deref()))
        .bind(segment.speaker_idx)
        .bind(sentences)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Moves a part's lines by `offset` seconds: the line times and each sentence's times. Text, marks and speakers stay.
pub fn shift_segments(transcripts: &[TranscriptSegment], offset: f64) -> Vec<TranscriptSegment> {
    transcripts
        .iter()
        .map(|s| TranscriptSegment {
            audio_start_time: s.audio_start_time.map(|t| t + offset),
            audio_end_time: s.audio_end_time.map(|t| t + offset),
            sentences: s.sentences.as_ref().map(|v| v.iter().map(|Sentence(a, b, t)| Sentence(a + offset, b + offset, t.clone())).collect()),
            ..s.clone()
        })
        .collect()
}

impl TranscriptsRepository {
    /// Saves a new meeting and its associated transcript segments.
    /// This function uses a transaction to ensure that either both the meeting
    /// and all its transcripts are saved, or none of them are.
    pub async fn save_transcript(
        pool: &SqlitePool,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
        notes: &[MeetingNote],
        enqueue_speakers: bool,
        title: &TitleOrigin,
    ) -> Result<String, SqlxError> {
        let meeting_id = format!("meeting-{}", Uuid::new_v4());

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now();

        // 1. Create the new meeting
        let result = sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, folder_path, title_source, calendar_uid, calendar_start) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&meeting_id)
        .bind(meeting_title)
        .bind(now)
        .bind(now)
        .bind(&folder_path)
        .bind(&title.source)
        .bind(&title.calendar_uid)
        .bind(&title.calendar_start)
        .execute(&mut *transaction)
        .await;

        if let Err(e) = result {
            error!("Failed to create meeting: {}", e);
            transaction.rollback().await?;
            return Err(e);
        }

        info!("Successfully created meeting with id: {}", meeting_id);

        // 2. Save each transcript segment with audio timing fields
        if let Err(e) = insert_segments(&mut *transaction, &meeting_id, transcripts).await {
            error!(
                "Failed to save transcript segment for meeting {}: {}",
                meeting_id, e
            );
            transaction.rollback().await?;
            return Err(e);
        }

        info!(
            "Successfully saved {} transcript segments for meeting {}",
            transcripts.len(),
            meeting_id
        );

        // 3. Save the notes in the same transaction; a note never fails the save
        let notes = sanitize_notes(notes);
        if !notes.is_empty() {
            if let Err(e) = MeetingNotesRepository::save(&mut *transaction, &meeting_id, &notes).await {
                error!("Failed to save notes for meeting {}: {}", meeting_id, e);
                transaction.rollback().await?;
                return Err(e);
            }
        }

        // 4. Queue speaker analysis in the same transaction; nothing is queued while the feature is off
        if enqueue_speakers {
            if let Err(e) = SpeakersRepository::enqueue_in_tx(&mut *transaction, &meeting_id).await {
                error!("Failed to queue speaker analysis for meeting {}: {}", meeting_id, e);
                transaction.rollback().await?;
                return Err(e);
            }
        }

        // Commit the transaction
        transaction.commit().await?;

        Ok(meeting_id)
    }

    /// Appends a recording part to an existing meeting: lines shifted by `offset`, new notes
    /// (also shifted) added after the old ones, and speaker analysis re-queued when it is on.
    /// Never commits; the caller owns the transaction and rolls it back on error.
    pub async fn append_in_tx(
        conn: &mut SqliteConnection,
        meeting_id: &str,
        transcripts: &[TranscriptSegment],
        offset: f64,
        notes: &[MeetingNote],
        speakers_active: bool,
    ) -> Result<(), SqlxError> {
        let exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .fetch_optional(&mut *conn)
            .await?;
        if exists.is_none() {
            return Err(SqlxError::RowNotFound);
        }
        insert_segments(&mut *conn, meeting_id, &shift_segments(transcripts, offset)).await?;
        let new_notes = sanitize_notes(notes);
        if !new_notes.is_empty() {
            let mut all = MeetingNotesRepository::get(&mut *conn, meeting_id).await?;
            all.extend(new_notes.into_iter().map(|n| MeetingNote { at: n.at.map(|t| t + offset), ..n }));
            MeetingNotesRepository::save(&mut *conn, meeting_id, &all).await?;
        }
        sqlx::query("UPDATE meetings SET updated_at = ? WHERE id = ?")
            .bind(Utc::now())
            .bind(meeting_id)
            .execute(&mut *conn)
            .await?;
        SpeakersRepository::transcripts_replaced_in_tx(&mut *conn, meeting_id, speakers_active).await?;
        Ok(())
    }

    /// Sets or clears the mark on one transcript line. Returns false if the line does not exist.
    pub async fn set_mark(
        pool: &SqlitePool,
        transcript_id: &str,
        mark: Option<&str>,
    ) -> Result<bool, SqlxError> {
        let r = sqlx::query("UPDATE transcripts SET mark = ? WHERE id = ?")
            .bind(mark)
            .bind(transcript_id)
            .execute(pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }
}

/// One search result. `source` is "title", "transcript", "summary" or "notes".
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub meeting_id: String,
    pub title: String,
    pub source: String,
    pub snippet: String,
    pub transcript_id: Option<String>,
    pub audio_start: Option<f64>,
}

const SEARCH_MEETINGS: usize = 50;
const SEARCH_HITS_PER_MEETING: usize = 3;
const SNIPPET_RADIUS: usize = 100;
const SEARCH_MAX_WORDS: usize = 50;

/// Case forms SQLite LIKE can match. LIKE folds only ASCII case, so each Cyrillic form is listed:
/// as typed, lowercase, first letter uppercased, all uppercase.
/// shortcut: a word with mixed case inside it (ЗаДачА) misses in SQL, though the Rust check would
/// match it; upgrade with a case-folded column when that search matters.
pub(crate) fn query_variants(q: &str) -> Vec<String> {
    let lower = q.to_lowercase();
    let mut chars = lower.chars();
    let mut first_upper = String::new();
    if let Some(c) = chars.next() {
        first_upper.extend(c.to_uppercase());
        first_upper.push_str(chars.as_str());
    }
    let mut out: Vec<String> = Vec::new();
    for v in [q.to_string(), lower, first_upper, q.to_uppercase()] {
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

fn like_pattern(variant: &str) -> String {
    let escaped = variant.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("%{escaped}%")
}

/// Words joined by single spaces, so the Rust phrase check ignores extra spaces the user typed.
fn normalize_query(query: &str) -> String {
    query.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// SQL prefilter: every word of the query must match in one of its case forms. Returns the
/// WHERE clause for `column` and the patterns to bind, in order. The Rust check confirms the phrase.
/// Only the first 50 words are filtered: more clauses overflow SQLite's expression depth limit,
/// and the phrase check still sees the whole query.
fn like_filter(column: &str, query: &str) -> (String, Vec<String>) {
    let mut clauses = Vec::new();
    let mut binds = Vec::new();
    for word in query.split_whitespace().take(SEARCH_MAX_WORDS) {
        let forms = query_variants(word);
        let one = format!("{column} LIKE ? ESCAPE '\\'");
        clauses.push(format!("({})", vec![one; forms.len()].join(" OR ")));
        binds.extend(forms.iter().map(|f| like_pattern(f)));
    }
    (clauses.join(" AND "), binds)
}

/// Text around the first match, cut by chars so multi-byte text never splits.
/// `needle_lower` must already be lowercase.
pub(crate) fn snippet_around(text: &str, needle_lower: &str, radius_chars: usize) -> Option<String> {
    let lower = text.to_lowercase();
    let byte = lower.find(needle_lower)?;
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    // Lowercasing can change the char count; clamping keeps the slice in range
    let centre = lower[..byte].chars().count().min(total);
    let needle_len = needle_lower.chars().count();
    let start = centre.saturating_sub(radius_chars);
    let end = (centre + needle_len + radius_chars).min(total);
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&chars[start..end]);
    if end < total {
        out.push('…');
    }
    Some(out)
}

/// One matching source item, before capping and snippeting. `source` is "title", "transcript", "summary" or "notes".
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SourceMatch {
    pub meeting_id: String,
    pub title: String,
    pub created_at: String,
    pub source: &'static str,
    /// The full matched text: a transcript line, the title, the summary markdown or one note.
    pub text: String,
    pub transcript_id: Option<String>,
    pub audio_start: Option<f64>,
}

/// Every title, summary, note and transcript line that contains the query. Uncapped and unsnipped.
/// Order: transcript lines by time, then title, summary and notes.
pub(crate) async fn source_matches(pool: &SqlitePool, query: &str) -> Result<Vec<SourceMatch>, SqlxError> {
    let query = normalize_query(query);
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let needle = query.to_lowercase();
    let mut out = Vec::new();

    let (clause, binds) = like_filter("t.transcript", &query);
    let sql = format!(
        "SELECT m.id, m.title, m.created_at, t.id, t.transcript, t.audio_start_time FROM transcripts t
         JOIN meetings m ON m.id = t.meeting_id
         WHERE {clause} ORDER BY t.audio_start_time"
    );
    let mut q = sqlx::query_as::<_, (String, String, String, String, String, Option<f64>)>(&sql);
    for p in &binds {
        q = q.bind(p);
    }
    for (meeting_id, title, created_at, transcript_id, text, audio_start) in q.fetch_all(pool).await? {
        if text.to_lowercase().contains(&needle) {
            out.push(SourceMatch { meeting_id, title, created_at, source: "transcript", text, transcript_id: Some(transcript_id), audio_start });
        }
    }

    let (clause, binds) = like_filter("title", &query);
    let sql = format!("SELECT id, title, created_at FROM meetings WHERE {clause}");
    let mut q = sqlx::query_as::<_, (String, String, String)>(&sql);
    for p in &binds {
        q = q.bind(p);
    }
    for (meeting_id, title, created_at) in q.fetch_all(pool).await? {
        if title.to_lowercase().contains(&needle) {
            out.push(SourceMatch { meeting_id, title: title.clone(), created_at, source: "title", text: title, transcript_id: None, audio_start: None });
        }
    }

    let markdown = "json_extract(s.result, '$.markdown')";
    let (clause, binds) = like_filter(markdown, &query);
    let sql = format!(
        "SELECT m.id, m.title, m.created_at, {markdown} FROM summary_processes s
         JOIN meetings m ON m.id = s.meeting_id
         WHERE s.status = 'completed' AND {clause}"
    );
    let mut q = sqlx::query_as::<_, (String, String, String, Option<String>)>(&sql);
    for p in &binds {
        q = q.bind(p);
    }
    for (meeting_id, title, created_at, text) in q.fetch_all(pool).await? {
        if let Some(text) = text.filter(|t| t.to_lowercase().contains(&needle)) {
            out.push(SourceMatch { meeting_id, title, created_at, source: "summary", text, transcript_id: None, audio_start: None });
        }
    }

    // Notes are checked in Rust: the JSON text escapes quotes and control characters, so LIKE on it would miss matches
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT m.id, m.title, m.created_at, n.notes_json FROM meeting_notes n JOIN meetings m ON m.id = n.meeting_id",
    )
    .fetch_all(pool)
    .await?;
    for (meeting_id, title, created_at, json) in rows {
        let Ok(values) = serde_json::from_str::<Vec<serde_json::Value>>(&json) else { continue };
        for note in parse_notes(values) {
            if note.text.to_lowercase().contains(&needle) {
                out.push(SourceMatch {
                    meeting_id: meeting_id.clone(), title: title.clone(), created_at: created_at.clone(),
                    source: "notes", text: note.text, transcript_id: None, audio_start: None,
                });
            }
        }
    }
    Ok(out)
}

/// Searches titles, summaries, notes and transcripts for the newest 50 meetings, at most 3 hits each.
pub async fn search_all(pool: &SqlitePool, query: &str) -> Result<Vec<SearchHit>, SqlxError> {
    let needle = normalize_query(query).to_lowercase();

    let mut created: HashMap<String, String> = HashMap::new();
    let mut by_meeting: HashMap<String, Vec<SourceMatch>> = HashMap::new();
    for m in source_matches(pool, query).await? {
        created.entry(m.meeting_id.clone()).or_insert_with(|| m.created_at.clone());
        by_meeting.entry(m.meeting_id.clone()).or_default().push(m);
    }

    // Newest meetings first; ties broken by id so the order is stable
    let mut ordered: Vec<(String, String)> = created.into_iter().collect();
    ordered.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ordered.truncate(SEARCH_MEETINGS);

    let mut out = Vec::new();
    for (meeting_id, _) in ordered {
        // Each meeting's matches arrive transcripts first (by time), then title, summary, notes
        let Some(list) = by_meeting.remove(&meeting_id) else { continue };
        let hits = list.into_iter().filter_map(|m| {
            let snippet = snippet_around(&m.text, &needle, SNIPPET_RADIUS)?;
            Some(SearchHit {
                meeting_id: m.meeting_id, title: m.title, source: m.source.into(), snippet,
                transcript_id: m.transcript_id, audio_start: m.audio_start,
            })
        });
        out.extend(hits.take(SEARCH_HITS_PER_MEETING));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::meeting_notes::{MeetingNote, MeetingNotesRepository};

    fn seg(text: &str, start: f64, mark: Option<&str>) -> TranscriptSegment {
        TranscriptSegment {
            id: "x".into(), text: text.into(), timestamp: "t".into(),
            audio_start_time: Some(start), audio_end_time: Some(start + 1.0), duration: Some(1.0),
            mark: mark.map(str::to_string), sentences: None, speaker_idx: None,
        }
    }

    #[tokio::test]
    async fn save_transcript_stores_sentences_and_self_label() {
        let pool = crate::database::repositories::test_pool().await;
        let mut line = seg("Hi.", 0.0, None);
        line.sentences = Some(vec![Sentence(0.0, 1.0, "Hi.".into())]);
        line.speaker_idx = Some(0);
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[line], None, &[], false, &TitleOrigin::default()).await.unwrap();
        let (idx, sentences): (Option<i64>, Option<String>) = sqlx::query_as("SELECT speaker_idx, sentences FROM transcripts WHERE meeting_id = ?")
            .bind(&id).fetch_one(&pool).await.unwrap();
        assert_eq!(idx, Some(0));
        assert_eq!(sentences.as_deref(), Some(r#"[[0.0,1.0,"Hi."]]"#));
    }

    #[tokio::test]
    async fn save_transcript_drops_bad_sentences_and_speaker_idx() {
        let pool = crate::database::repositories::test_pool().await;
        // Malformed sentences and an out-of-range speaker from the webview are dropped while parsing
        let malformed: TranscriptSegment = serde_json::from_value(serde_json::json!({
            "id": "x", "text": "a", "timestamp": "t", "audio_start_time": 0.0, "audio_end_time": 1.0, "duration": 1.0,
            "sentences": [[1, "x", "a"]], "speaker_idx": 7
        })).unwrap();
        // Well-formed sentences whose text does not match the line are dropped by save_transcript
        let mut mismatched = seg("a", 2.0, None);
        mismatched.sentences = Some(vec![Sentence(2.0, 3.0, "Other.".into())]);
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[malformed, mismatched], None, &[], false, &TitleOrigin::default()).await.unwrap();
        let rows: Vec<(Option<i64>, Option<String>)> = sqlx::query_as("SELECT speaker_idx, sentences FROM transcripts WHERE meeting_id = ?")
            .bind(&id).fetch_all(&pool).await.unwrap();
        assert_eq!(rows, vec![(None, None), (None, None)]);
    }

    #[tokio::test]
    async fn save_transcript_stores_marks_and_notes() {
        let pool = crate::database::repositories::test_pool().await;
        let notes = vec![MeetingNote { id: "note-1".into(), at: Some(1.5), text: "ask".into() }];
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[seg("a", 0.0, Some("action")), seg("b", 2.0, None)], None, &notes, false, &TitleOrigin::default()).await.unwrap();
        let marks: Vec<(Option<String>,)> = sqlx::query_as("SELECT mark FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time")
            .bind(&id).fetch_all(&pool).await.unwrap();
        assert_eq!(marks, vec![(Some("action".into()),), (None,)]);
        assert_eq!(MeetingNotesRepository::get(&pool, &id).await.unwrap(), notes);
    }

    #[tokio::test]
    async fn save_transcript_sanitizes_bad_notes_and_marks() {
        let pool = crate::database::repositories::test_pool().await;
        let notes = vec![
            MeetingNote { id: "note-1".into(), at: Some(f64::NAN), text: "  ".into() },
            MeetingNote { id: "note-2".into(), at: None, text: "z".repeat(5000) },
        ];
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[seg("a", 0.0, Some("nonsense"))], None, &notes, false, &TitleOrigin::default()).await.unwrap();
        let (mark,): (Option<String>,) = sqlx::query_as("SELECT mark FROM transcripts WHERE meeting_id = ?").bind(&id).fetch_one(&pool).await.unwrap();
        assert_eq!(mark, None);
        let saved = MeetingNotesRepository::get(&pool, &id).await.unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].text.chars().count(), 2000);
    }

    #[tokio::test]
    async fn set_mark_and_keep_audio_round_trip() {
        let pool = crate::database::repositories::test_pool().await;
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[seg("a", 0.0, None)], None, &[], false, &TitleOrigin::default()).await.unwrap();
        let (tid,): (String,) = sqlx::query_as("SELECT id FROM transcripts WHERE meeting_id = ?").bind(&id).fetch_one(&pool).await.unwrap();
        assert!(TranscriptsRepository::set_mark(&pool, &tid, Some("important")).await.unwrap());
        assert!(!TranscriptsRepository::set_mark(&pool, "nope", Some("important")).await.unwrap());
        let (m,): (Option<String>,) = sqlx::query_as("SELECT mark FROM transcripts WHERE id = ?").bind(&tid).fetch_one(&pool).await.unwrap();
        assert_eq!(m.as_deref(), Some("important"));
        assert!(crate::database::repositories::meeting::MeetingsRepository::set_keep_audio(&pool, &id, true).await.unwrap());
        let meta = crate::database::repositories::meeting::MeetingsRepository::get_meeting_metadata(&pool, &id).await.unwrap().unwrap();
        assert!(meta.keep_audio);
    }

    #[tokio::test]
    async fn delete_meeting_removes_notes() {
        let pool = crate::database::repositories::test_pool().await;
        let notes = vec![MeetingNote { id: "note-1".into(), at: None, text: "x".into() }];
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[seg("a", 0.0, None)], None, &notes, false, &TitleOrigin::default()).await.unwrap();
        assert!(crate::database::repositories::meeting::MeetingsRepository::delete_meeting(&pool, &id).await.unwrap());
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meeting_notes").fetch_one(&pool).await.unwrap();
        assert_eq!(n, 0);
    }

    async fn meeting(pool: &SqlitePool, title: &str, lines: &[&str], notes: &[&str], summary: Option<&str>) -> String {
        let segs: Vec<TranscriptSegment> = lines.iter().enumerate().map(|(i, t)| seg(t, i as f64, None)).collect();
        let notes: Vec<MeetingNote> = notes.iter().enumerate()
            .map(|(i, t)| MeetingNote { id: format!("note-{i}"), at: None, text: t.to_string() }).collect();
        let id = TranscriptsRepository::save_transcript(pool, title, &segs, None, &notes, false, &TitleOrigin::default()).await.unwrap();
        if let Some(md) = summary {
            let result = serde_json::json!({ "markdown": md }).to_string();
            sqlx::query("INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, result) VALUES (?, 'completed', '2026-10-22T00:00:00Z', '2026-10-22T00:00:00Z', ?)")
                .bind(&id).bind(result).execute(pool).await.unwrap();
        }
        id
    }

    #[tokio::test]
    async fn search_folds_cyrillic_case() {
        let pool = crate::database::repositories::test_pool().await;
        meeting(&pool, "Standup", &["Задача на неделю"], &[], None).await;
        let hits = search_all(&pool, "задача").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, "transcript");
        assert!(hits[0].transcript_id.is_some());

        // A name capitalised after the first word matches only with each word in its own case forms
        meeting(&pool, "Созвон", &["Встреча с Иваном"], &[], None).await;
        let hits = search_all(&pool, "встреча с иваном").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Созвон");
    }

    #[tokio::test]
    async fn search_finds_summary_and_notes() {
        let pool = crate::database::repositories::test_pool().await;
        meeting(&pool, "Квартал", &["Обсуждали план"], &["проверить бюджет"], Some("Главное: бюджет на квартал")).await;
        let hits = search_all(&pool, "бюджет").await.unwrap();
        let sources: Vec<&str> = hits.iter().map(|h| h.source.as_str()).collect();
        assert_eq!(sources, vec!["summary", "notes"]);
    }

    #[test]
    fn snippet_is_char_safe() {
        let at_start = format!("Задача {}", "я".repeat(494));
        let at_end = format!("{}Задача", "я".repeat(494));
        for line in [&at_start, &at_end] {
            let snippet = snippet_around(line, "задача", 100).unwrap();
            assert!(snippet.to_lowercase().contains("задача"));
        }
        assert_eq!(snippet_around("без совпадений", "задача", 100), None);
    }

    #[tokio::test]
    async fn like_wildcards_escaped() {
        let pool = crate::database::repositories::test_pool().await;
        meeting(&pool, "A", &["Цена 500 рублей"], &[], None).await;
        let m2 = meeting(&pool, "B", &["Скидка 50% сегодня"], &[], None).await;
        let hits = search_all(&pool, "50%").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].meeting_id, m2);
    }

    #[tokio::test]
    async fn three_hits_per_meeting_max() {
        let pool = crate::database::repositories::test_pool().await;
        meeting(&pool, "бюджет", &["бюджет один", "бюджет два", "бюджет три", "бюджет четыре"], &["бюджет"], Some("бюджет")).await;
        let hits = search_all(&pool, "бюджет").await.unwrap();
        // Transcripts come first, by time, then title, summary and notes; the cap keeps three
        let sources: Vec<&str> = hits.iter().map(|h| h.source.as_str()).collect();
        assert_eq!(sources, vec!["transcript", "transcript", "transcript"]);
        let times: Vec<Option<f64>> = hits.iter().map(|h| h.audio_start).collect();
        assert_eq!(times, vec![Some(0.0), Some(1.0), Some(2.0)]);
    }

    #[tokio::test]
    async fn source_matches_is_uncapped() {
        let pool = crate::database::repositories::test_pool().await;
        meeting(&pool, "бюджет", &["бюджет один", "бюджет два", "бюджет три", "бюджет четыре"], &["бюджет"], Some("бюджет")).await;
        let found = source_matches(&pool, "бюджет").await.unwrap();
        let sources: Vec<&str> = found.iter().map(|m| m.source).collect();
        assert_eq!(sources, vec!["transcript", "transcript", "transcript", "transcript", "title", "summary", "notes"]);
    }

    #[tokio::test]
    async fn empty_query_returns_nothing() {
        let pool = crate::database::repositories::test_pool().await;
        meeting(&pool, "M", &["что-то"], &[], None).await;
        assert!(search_all(&pool, "   ").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn search_ignores_extra_spaces_in_query() {
        let pool = crate::database::repositories::test_pool().await;
        meeting(&pool, "План", &["Обсуждаем проект план на квартал"], &[], None).await;
        assert_eq!(search_all(&pool, "проект  план").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn transcripts_are_indexed_by_meeting() {
        // Every meeting page, search and delete reads transcripts by meeting_id
        let pool = crate::database::repositories::test_pool().await;
        let cols: Vec<(String,)> = sqlx::query_as("SELECT name FROM pragma_index_info('idx_transcripts_meeting') ORDER BY seqno")
            .fetch_all(&pool).await.unwrap();
        let cols: Vec<String> = cols.into_iter().map(|c| c.0).collect();
        assert_eq!(cols, vec!["meeting_id", "audio_start_time", "id"]);
    }

    #[tokio::test]
    async fn search_caps_words_in_prefilter() {
        let pool = crate::database::repositories::test_pool().await;
        let long = vec!["слово"; 2000].join(" ");
        assert!(search_all(&pool, &long).await.is_ok());
    }

    #[tokio::test]
    async fn append_shifts_lines_and_sentences_after_existing() {
        let pool = crate::database::repositories::test_pool().await;
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[seg("old", 0.0, None)], None, &[], false, &TitleOrigin::default()).await.unwrap();
        let new_line = TranscriptSegment {
            audio_end_time: Some(2.0), sentences: Some(vec![Sentence(0.5, 1.5, "new".into())]),
            ..seg("new", 0.0, None)
        };
        let mut tx = pool.begin().await.unwrap();
        TranscriptsRepository::append_in_tx(&mut *tx, &id, &[new_line], 100.0, &[], false).await.unwrap();
        tx.commit().await.unwrap();
        let rows: Vec<(String, Option<f64>, Option<f64>, Option<String>)> = sqlx::query_as(
            "SELECT transcript, audio_start_time, audio_end_time, sentences FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time",
        ).bind(&id).fetch_all(&pool).await.unwrap();
        assert_eq!(rows.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), vec!["old", "new"]);
        assert_eq!((rows[1].1, rows[1].2), (Some(100.0), Some(102.0)));
        assert_eq!(rows[1].3.as_deref(), Some(r#"[[100.5,101.5,"new"]]"#));
    }

    #[tokio::test]
    async fn append_keeps_old_notes_and_marks_and_shifts_new_notes() {
        let pool = crate::database::repositories::test_pool().await;
        let old_notes = vec![MeetingNote { id: "n1".into(), at: Some(5.0), text: "old".into() }];
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[seg("old", 0.0, Some("important"))], None, &old_notes, false, &TitleOrigin::default()).await.unwrap();
        let new_notes = vec![
            MeetingNote { id: "n2".into(), at: Some(3.0), text: "new".into() },
            MeetingNote { id: "n3".into(), at: None, text: "later".into() },
        ];
        let mut tx = pool.begin().await.unwrap();
        TranscriptsRepository::append_in_tx(&mut *tx, &id, &[seg("new", 0.0, None)], 100.0, &new_notes, false).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(MeetingNotesRepository::get(&pool, &id).await.unwrap(), vec![
            MeetingNote { id: "n1".into(), at: Some(5.0), text: "old".into() },
            MeetingNote { id: "n2".into(), at: Some(103.0), text: "new".into() },
            MeetingNote { id: "n3".into(), at: None, text: "later".into() },
        ]);
        let marks: Vec<(Option<String>,)> = sqlx::query_as("SELECT mark FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time")
            .bind(&id).fetch_all(&pool).await.unwrap();
        assert_eq!(marks, vec![(Some("important".into()),), (None,)]);
    }

    #[tokio::test]
    async fn append_to_missing_meeting_fails_and_writes_nothing() {
        let pool = crate::database::repositories::test_pool().await;
        let note = MeetingNote { id: "n1".into(), at: Some(1.0), text: "x".into() };
        let mut tx = pool.begin().await.unwrap();
        let result = TranscriptsRepository::append_in_tx(&mut *tx, "meeting-nope", &[seg("a", 0.0, None)], 10.0, &[note], true).await;
        assert!(result.is_err());
        tx.rollback().await.unwrap();
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM transcripts").fetch_one(&pool).await.unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn append_requeues_speakers_when_active() {
        let pool = crate::database::repositories::test_pool().await;
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[seg("old", 0.0, None)], None, &[], true, &TitleOrigin::default()).await.unwrap();
        sqlx::query("UPDATE speaker_jobs SET status = 'done' WHERE meeting_id = ?").bind(&id).execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        TranscriptsRepository::append_in_tx(&mut *tx, &id, &[seg("new", 0.0, None)], 100.0, &[], true).await.unwrap();
        tx.commit().await.unwrap();
        let (status,): (String,) = sqlx::query_as("SELECT status FROM speaker_jobs WHERE meeting_id = ?").bind(&id).fetch_one(&pool).await.unwrap();
        assert_eq!(status, "pending");
    }

    #[test]
    fn shift_segments_moves_only_times() {
        let line = TranscriptSegment {
            id: "x".into(), text: "a".into(), timestamp: "t".into(),
            audio_start_time: Some(1.0), audio_end_time: Some(2.0), duration: Some(1.0),
            mark: Some("action".into()), sentences: Some(vec![Sentence(1.0, 2.0, "a".into())]), speaker_idx: Some(2),
        };
        let untimed = TranscriptSegment {
            id: "y".into(), text: "b".into(), timestamp: "t".into(),
            audio_start_time: None, audio_end_time: None, duration: None,
            mark: None, sentences: None, speaker_idx: None,
        };
        let out = shift_segments(&[line, untimed], 10.0);
        assert_eq!(out[0].text, "a");
        assert_eq!(out[0].mark.as_deref(), Some("action"));
        assert_eq!(out[0].speaker_idx, Some(2));
        assert_eq!(out[0].duration, Some(1.0));
        assert_eq!((out[0].audio_start_time, out[0].audio_end_time), (Some(11.0), Some(12.0)));
        assert_eq!(out[0].sentences, Some(vec![Sentence(11.0, 12.0, "a".into())]));
        assert_eq!(out[1].text, "b");
        assert_eq!((out[1].audio_start_time, out[1].audio_end_time, out[1].sentences.clone()), (None, None, None));
    }
}
