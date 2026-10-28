//! User notes for a meeting (spec §4). Stored as JSON in the existing `meeting_notes` table.
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use std::cmp::Ordering;
use tracing::warn;

pub const NOTE_MAX_CHARS: usize = 2000;
pub const MARKS: [&str; 3] = ["important", "action", "followup"];
const NOTE_ID_MAX: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeetingNote {
    pub id: String,
    /// Seconds into the recording; `None` = added after the meeting.
    pub at: Option<f64>,
    pub text: String,
}

fn valid_at(at: f64) -> bool {
    at.is_finite() && at >= 0.0
}

fn valid_id(id: &str) -> bool {
    !id.trim().is_empty() && id.chars().count() <= NOTE_ID_MAX
}

/// For commands after the meeting: reject bad input with a message.
pub fn validate_mark(mark: Option<&str>) -> Result<(), String> {
    match mark {
        Some(m) if !MARKS.contains(&m) => Err(format!("Unknown mark: {m}")),
        _ => Ok(()),
    }
}

/// For commands after the meeting: trims text, rejects anything invalid.
pub fn validate_notes(notes: &[MeetingNote]) -> Result<Vec<MeetingNote>, String> {
    notes
        .iter()
        .map(|n| {
            let text = n.text.trim();
            if text.is_empty() {
                return Err("A note cannot be empty".to_string());
            }
            if text.chars().count() > NOTE_MAX_CHARS {
                return Err(format!("A note can be at most {NOTE_MAX_CHARS} characters"));
            }
            if !valid_id(&n.id) {
                return Err("Invalid note id".to_string());
            }
            if n.at.is_some_and(|a| !valid_at(a)) {
                return Err("Invalid note time".to_string());
            }
            Ok(MeetingNote { id: n.id.clone(), at: n.at, text: text.to_string() })
        })
        .collect()
}

/// For the save at stop: never lose a meeting over a bad note. Drops empty notes,
/// truncates long ones, drops invalid times, replaces invalid ids.
pub fn sanitize_notes(notes: &[MeetingNote]) -> Vec<MeetingNote> {
    notes
        .iter()
        .filter_map(|n| {
            let text = n.text.trim();
            if text.is_empty() {
                return None;
            }
            let id = if valid_id(&n.id) {
                n.id.clone()
            } else {
                format!("note-{}", uuid::Uuid::new_v4())
            };
            Some(MeetingNote {
                id,
                at: n.at.filter(|a| valid_at(*a)),
                text: text.chars().take(NOTE_MAX_CHARS).collect(),
            })
        })
        .collect()
}

/// For the save at stop: an unknown mark is dropped, not an error.
pub fn sanitize_mark(mark: Option<&str>) -> Option<String> {
    mark.filter(|m| MARKS.contains(m)).map(str::to_string)
}

/// By recording time; notes added after the meeting go last. Stable.
pub fn sort_notes(notes: &mut [MeetingNote]) {
    notes.sort_by(|a, b| match (a.at, b.at) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    });
}

/// Decodes notes one by one, so one malformed note never fails the save at stop.
pub fn parse_notes(values: Vec<serde_json::Value>) -> Vec<MeetingNote> {
    values
        .into_iter()
        .filter_map(|v| match serde_json::from_value::<MeetingNote>(v) {
            Ok(n) => Some(n),
            Err(e) => {
                warn!("Dropping a note that could not be read: {e}");
                None
            }
        })
        .collect()
}

/// A mark on a transcript line before re-transcription.
pub struct TimedMark {
    pub start: f64,
    pub end: f64,
    pub mark: String,
}

/// Re-transcription replaces every line; each old mark moves to the new line that overlaps
/// it most (no overlap: nearest start). If two marks land on one line, the first one wins.
pub fn carry_marks(old: &[TimedMark], new: &[(String, f64, f64)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for m in old {
        let overlap = |s: f64, e: f64| (e.min(m.end) - s.max(m.start)).max(0.0);
        let best = new
            .iter()
            .max_by(|a, b| {
                overlap(a.1, a.2)
                    .partial_cmp(&overlap(b.1, b.2))
                    .unwrap_or(Ordering::Equal)
                    // equal overlap: prefer the nearer start (max_by keeps the later max, so invert)
                    .then_with(|| (b.1 - m.start).abs().partial_cmp(&(a.1 - m.start).abs()).unwrap_or(Ordering::Equal))
            });
        if let Some((id, _, _)) = best {
            if !out.iter().any(|(existing, _)| existing == id) {
                out.push((id.clone(), m.mark.clone()));
            }
        }
    }
    out
}

pub struct MeetingNotesRepository;

impl MeetingNotesRepository {
    pub async fn get<'e, E: sqlx::SqliteExecutor<'e>>(ex: E, meeting_id: &str) -> Result<Vec<MeetingNote>, sqlx::Error> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT notes_json FROM meeting_notes WHERE meeting_id = ?")
                .bind(meeting_id)
                .fetch_optional(ex)
                .await?;
        Ok(match row.and_then(|(json,)| json) {
            // Per note, like the save path: one bad stored entry must not hide the others.
            Some(json) => match serde_json::from_str::<Vec<serde_json::Value>>(&json) {
                Ok(values) => parse_notes(values),
                Err(e) => {
                    warn!("Unreadable notes for meeting {meeting_id}: {e}");
                    Vec::new()
                }
            },
            None => Vec::new(),
        })
    }

    /// Replaces the meeting's notes. Callers validate or sanitize first.
    pub async fn save(conn: &mut SqliteConnection, meeting_id: &str, notes: &[MeetingNote]) -> Result<(), sqlx::Error> {
        let mut sorted = notes.to_vec();
        sort_notes(&mut sorted);
        let json = serde_json::to_string(&sorted).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
        let now = chrono::Utc::now();
        sqlx::query(
            "INSERT INTO meeting_notes (meeting_id, notes_json, created_at, updated_at) VALUES (?, ?, ?, ?)
             ON CONFLICT(meeting_id) DO UPDATE SET notes_json = excluded.notes_json, updated_at = excluded.updated_at",
        )
        .bind(meeting_id)
        .bind(json)
        .bind(now)
        .bind(now)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: &str, at: Option<f64>, text: &str) -> MeetingNote {
        MeetingNote { id: id.into(), at, text: text.into() }
    }

    #[test]
    fn validate_notes_trims_and_rejects_bad_input() {
        let ok = validate_notes(&[note("note-1", Some(3.0), "  ask about Q2  ")]).unwrap();
        assert_eq!(ok[0].text, "ask about Q2");
        assert!(validate_notes(&[note("note-1", None, "   ")]).is_err());
        assert!(validate_notes(&[note("note-1", None, &"x".repeat(2001))]).is_err());
        assert!(validate_notes(&[note("note-1", None, &"x".repeat(2000))]).is_ok());
        assert!(validate_notes(&[note("", None, "a")]).is_err());
        assert!(validate_notes(&[note(&"i".repeat(65), None, "a")]).is_err());
        assert!(validate_notes(&[note("note-1", Some(f64::NAN), "a")]).is_err());
        assert!(validate_notes(&[note("note-1", Some(-1.0), "a")]).is_err());
    }

    #[test]
    fn sanitize_notes_never_fails() {
        let out = sanitize_notes(&[
            note("note-1", Some(f64::NAN), " keep "),
            note("note-2", None, "   "),
            note("", Some(5.0), &"y".repeat(5000)),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], note("note-1", None, "keep"));
        assert_eq!(out[1].text.chars().count(), NOTE_MAX_CHARS);
        assert!(out[1].id.starts_with("note-"));
        assert_eq!(out[1].at, Some(5.0));
    }

    #[test]
    fn marks_are_checked() {
        assert!(validate_mark(None).is_ok());
        for m in MARKS { assert!(validate_mark(Some(m)).is_ok()); }
        assert!(validate_mark(Some("IMPORTANT")).is_err());
        assert_eq!(sanitize_mark(Some("action")), Some("action".into()));
        assert_eq!(sanitize_mark(Some("bogus")), None);
    }

    #[test]
    fn parse_notes_drops_entries_that_do_not_decode() {
        let v = vec![
            serde_json::json!({"id": "n1", "at": 1.0, "text": "ok"}),
            serde_json::json!({"id": "n2", "at": "soon", "text": "bad time"}),
            serde_json::json!({"id": "n3"}),
            serde_json::json!({"id": "n4", "at": null, "text": "later"}),
        ];
        let ids: Vec<_> = parse_notes(v).into_iter().map(|n| n.id).collect();
        assert_eq!(ids, ["n1", "n4"]);
    }

    #[test]
    fn carry_marks_moves_each_mark_to_the_best_overlapping_segment() {
        let old = vec![
            TimedMark { start: 10.0, end: 14.0, mark: "action".into() },
            TimedMark { start: 30.0, end: 31.0, mark: "important".into() },
            TimedMark { start: 100.0, end: 101.0, mark: "followup".into() }, // no overlap: nearest start
        ];
        let new = vec![
            ("a".to_string(), 9.0, 11.0),
            ("b".to_string(), 11.0, 15.0),
            ("c".to_string(), 29.0, 33.0),
            ("d".to_string(), 90.0, 95.0),
        ];
        assert_eq!(
            carry_marks(&old, &new),
            vec![("b".to_string(), "action".to_string()), ("c".to_string(), "important".to_string()), ("d".to_string(), "followup".to_string())]
        );
        assert!(carry_marks(&old, &[]).is_empty());
    }

    #[test]
    fn carry_marks_keeps_the_first_mark_when_two_land_on_one_segment() {
        let old = vec![
            TimedMark { start: 1.0, end: 2.0, mark: "action".into() },
            TimedMark { start: 2.0, end: 3.0, mark: "important".into() },
        ];
        let new = vec![("a".to_string(), 0.0, 5.0)];
        assert_eq!(carry_marks(&old, &new), vec![("a".to_string(), "action".to_string())]);
    }

    #[test]
    fn sort_puts_untimed_notes_last_and_keeps_order() {
        let mut v = vec![note("a", None, "a"), note("b", Some(9.0), "b"), note("c", None, "c"), note("d", Some(2.0), "d")];
        sort_notes(&mut v);
        let ids: Vec<_> = v.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["d", "b", "a", "c"]);
    }

    #[tokio::test]
    async fn get_skips_a_bad_stored_note_and_keeps_the_good_one() {
        let pool = crate::database::repositories::test_pool().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 't', '2026-10-15T00:00:00Z', '2026-10-15T00:00:00Z')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO meeting_notes (meeting_id, notes_json, created_at, updated_at) VALUES ('m1', ?, '2026-10-15T00:00:00Z', '2026-10-15T00:00:00Z')")
            .bind(r#"[{"id":"n1","at":"soon","text":"bad"},{"id":"n2","at":1.0,"text":"good"}]"#)
            .execute(&pool).await.unwrap();
        assert_eq!(MeetingNotesRepository::get(&pool, "m1").await.unwrap(), vec![note("n2", Some(1.0), "good")]);
    }

    #[tokio::test]
    async fn save_replaces_and_get_reads_back() {
        let pool = crate::database::repositories::test_pool().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 't', '2026-10-15T00:00:00Z', '2026-10-15T00:00:00Z')")
            .execute(&pool).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        MeetingNotesRepository::save(&mut conn, "m1", &[note("n1", None, "later"), note("n2", Some(1.0), "first")]).await.unwrap();
        MeetingNotesRepository::save(&mut conn, "m1", &[note("n3", Some(2.0), "only")]).await.unwrap();
        drop(conn);
        assert_eq!(MeetingNotesRepository::get(&pool, "m1").await.unwrap(), vec![note("n3", Some(2.0), "only")]);
        assert!(MeetingNotesRepository::get(&pool, "missing").await.unwrap().is_empty());
    }
}
