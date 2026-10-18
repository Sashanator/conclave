//! Speaker identification storage: people, voiceprints, meeting speakers, analysis jobs and the
//! token-checked publish. Every write that ends a job checks its run token, so an overtaken job
//! (re-transcribe, re-identify, meeting deleted) publishes nothing.

use crate::speakers::assign::{KnownPerson, LineOut, ResolvedSpeaker};
use chrono::{SecondsFormat, Utc};
use sqlx::{Error as SqlxError, SqliteConnection, SqlitePool};
use std::collections::{BTreeMap, HashSet};
use tracing::warn;
use uuid::Uuid;

const DIM: usize = 192;
const PRINTS_PER_PERSON: i64 = 20;
const LEARN_MIN_SECONDS: f64 = 20.0;
const NAME_MAX_CHARS: usize = 80;
const SELF_ID: &str = "self";
const RUNNING: &str = "Speaker analysis is running";

pub struct SpeakersRepository;

#[derive(Debug, Clone, PartialEq)]
pub struct JobRow {
    pub meeting_id: String,
    pub status: String,
    pub reason: Option<String>,
    pub backend: Option<String>,
    pub run_token: String,
    pub audio_seconds: Option<f64>,
    pub run_seconds: Option<f64>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SpeakerView {
    pub idx: i64,
    pub name: String,
    pub person_id: Option<String>,
    pub is_self: bool,
    pub seconds: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PersonView {
    pub id: String,
    pub name: String,
    pub is_self: bool,
    pub meetings: i64,
    pub last_heard: Option<String>,
}

#[derive(Debug, Clone)]
pub enum NameTarget {
    Person(String),
    New(String),
}

pub struct Publish {
    pub speakers: Vec<ResolvedSpeaker>,
    pub lines: Vec<LineOut>,
    pub self_print: Option<(Vec<f32>, f64)>,
    pub backend: String,
    pub audio_seconds: f64,
    pub run_seconds: f64,
}

impl SpeakersRepository {
    /// Queues the meeting's analysis under a new run token, inside the caller's transaction.
    pub async fn enqueue_in_tx(conn: &mut SqliteConnection, meeting_id: &str) -> Result<String, SqlxError> {
        let token = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO speaker_jobs (meeting_id, status, reason, backend, run_token, audio_seconds, run_seconds, updated_at)
             VALUES (?, 'pending', NULL, NULL, ?, NULL, NULL, ?)
             ON CONFLICT(meeting_id) DO UPDATE SET status = 'pending', reason = NULL, backend = NULL,
               run_token = excluded.run_token, audio_seconds = NULL, run_seconds = NULL, updated_at = excluded.updated_at",
        )
        .bind(meeting_id)
        .bind(&token)
        .bind(now())
        .execute(conn)
        .await?;
        Ok(token)
    }

    pub async fn enqueue(pool: &SqlitePool, meeting_id: &str) -> Result<String, SqlxError> {
        let mut tx = pool.begin().await?;
        let token = Self::enqueue_in_tx(&mut *tx, meeting_id).await?;
        tx.commit().await?;
        Ok(token)
    }

    /// Called by every transaction that replaces a meeting's transcripts. A feature-off replace
    /// rotates the token, so a run still in flight can never publish against the replaced lines.
    /// Returns whether a job row changed (queued, or skipped), so callers know to report it.
    pub async fn transcripts_replaced_in_tx(conn: &mut SqliteConnection, meeting_id: &str, active: bool) -> Result<bool, SqlxError> {
        if active {
            Self::enqueue_in_tx(&mut *conn, meeting_id).await?;
            return Ok(true);
        }
        let r = sqlx::query(
            "UPDATE speaker_jobs SET status = 'skipped', reason = 'feature_off', run_token = ?, updated_at = ?
             WHERE meeting_id = ?",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(now())
        .bind(meeting_id)
        .execute(&mut *conn)
        .await?;
        Ok(r.rows_affected() > 0)
    }

    /// At start-up, pending and running jobs wait again under new tokens, so a run from before a crash cannot publish.
    pub async fn recover_jobs(pool: &SqlitePool) -> Result<u64, SqlxError> {
        let mut tx = pool.begin().await?;
        let ids: Vec<(String,)> = sqlx::query_as("SELECT meeting_id FROM speaker_jobs WHERE status IN ('pending', 'running')")
            .fetch_all(&mut *tx)
            .await?;
        for (id,) in &ids {
            sqlx::query("UPDATE speaker_jobs SET status = 'pending', run_token = ?, updated_at = ? WHERE meeting_id = ?")
                .bind(Uuid::new_v4().to_string())
                .bind(now())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(ids.len() as u64)
    }

    /// The oldest pending job becomes running. Returns its meeting id and run token.
    pub async fn claim_next(pool: &SqlitePool) -> Result<Option<(String, String)>, SqlxError> {
        sqlx::query_as::<_, (String, String)>(
            "UPDATE speaker_jobs SET status = 'running', updated_at = ?
             WHERE meeting_id = (SELECT meeting_id FROM speaker_jobs WHERE status = 'pending' ORDER BY updated_at, meeting_id LIMIT 1)
               AND status = 'pending'
             RETURNING meeting_id, run_token",
        )
        .bind(now())
        .fetch_optional(pool)
        .await
    }

    /// Ends a run that will not publish. False if the run was overtaken, so nothing is written.
    pub async fn finish_without_publish(
        pool: &SqlitePool,
        meeting_id: &str,
        token: &str,
        status: &str,
        reason: Option<&str>,
        backend: Option<&str>,
    ) -> Result<bool, SqlxError> {
        if status != "failed" && status != "skipped" {
            return Err(SqlxError::Protocol(format!("a job cannot finish as {status} without publishing")));
        }
        let r = sqlx::query(
            "UPDATE speaker_jobs SET status = ?, reason = ?, backend = ?, updated_at = ?
             WHERE meeting_id = ? AND run_token = ? AND status = 'running'",
        )
        .bind(status)
        .bind(reason)
        .bind(backend)
        .bind(now())
        .bind(meeting_id)
        .bind(token)
        .execute(pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Publishes a finished run in one transaction. False, with nothing written, if the run was overtaken.
    pub async fn publish(pool: &SqlitePool, meeting_id: &str, token: &str, p: &Publish) -> Result<bool, SqlxError> {
        let mut tx = pool.begin().await?;
        let done = sqlx::query(
            "UPDATE speaker_jobs SET status = 'done', backend = ?, audio_seconds = ?, run_seconds = ?, reason = NULL, updated_at = ?
             WHERE meeting_id = ? AND run_token = ? AND status = 'running'",
        )
        .bind(&p.backend)
        .bind(p.audio_seconds)
        .bind(p.run_seconds)
        .bind(now())
        .bind(meeting_id)
        .bind(token)
        .execute(&mut *tx)
        .await?;
        if done.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }

        // People whose voice this meeting taught before a re-run. They keep learning from it under the new index.
        let learned: HashSet<String> = sqlx::query_as::<_, (String,)>("SELECT DISTINCT person_id FROM voiceprints WHERE source_meeting_id = ?")
            .bind(meeting_id)
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .map(|r| r.0)
            .collect();
        // Prints learned from this meeting go; the kept ones below are written back under the new indexes.
        sqlx::query("DELETE FROM voiceprints WHERE source_meeting_id = ?")
            .bind(meeting_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id = ?")
            .bind(meeting_id)
            .execute(&mut *tx)
            .await?;
        for s in &p.speakers {
            let centroid = s.centroid.as_deref().map(encode).transpose()?;
            sqlx::query(
                // A person deleted or merged mid-run becomes NULL ("Speaker n") instead of failing the foreign key
                "INSERT INTO meeting_speakers (meeting_id, idx, person_id, name, centroid, seconds)
                 VALUES (?, ?, (SELECT id FROM people WHERE id = ?), NULL, ?, ?)",
            )
            .bind(meeting_id)
            .bind(s.idx)
            .bind(s.person_id.as_deref())
            .bind(centroid.as_deref())
            .bind(s.seconds)
            .execute(&mut *tx)
            .await?;

            // A missing person inserts nothing (the subquery finds no row), like the meeting_speakers insert above
            if let (Some(person), Some(vector)) = (s.person_id.as_deref(), centroid.as_deref()) {
                if person != SELF_ID && learned.contains(person) && s.seconds >= LEARN_MIN_SECONDS {
                    sqlx::query(
                        "INSERT INTO voiceprints (id, person_id, source_meeting_id, source_idx, vector, seconds, created_at)
                         SELECT ?, id, ?, ?, ?, ?, ? FROM people WHERE id = ?",
                    )
                    .bind(format!("voiceprint-{}", Uuid::new_v4()))
                    .bind(meeting_id)
                    .bind(s.idx)
                    .bind(vector)
                    .bind(s.seconds)
                    .bind(now())
                    .bind(person)
                    .execute(&mut *tx)
                    .await?;
                    prune(&mut *tx, person).await?;
                }
            }
        }

        // Lines left out of this publish keep no label: a label from the previous run may name a different speaker now.
        sqlx::query("UPDATE transcripts SET speaker_idx = NULL WHERE meeting_id = ?")
            .bind(meeting_id)
            .execute(&mut *tx)
            .await?;

        for l in &p.lines {
            let sentences = l.sentences.as_ref().and_then(|s| serde_json::to_string(s).ok());
            let duration = l.end - l.start;
            // A line or source row that is missing fails the publish, which rolls everything back.
            let done = match &l.id {
                Some(id) => {
                    sqlx::query(
                        "UPDATE transcripts SET transcript = ?, audio_start_time = ?, audio_end_time = ?, duration = ?,
                           sentences = ?, speaker_idx = ?
                         WHERE id = ? AND meeting_id = ?",
                    )
                    .bind(&l.text)
                    .bind(l.start)
                    .bind(l.end)
                    .bind(duration)
                    .bind(sentences)
                    .bind(l.speaker_idx)
                    .bind(id)
                    .bind(meeting_id)
                    .execute(&mut *tx)
                    .await?
                }
                None => {
                    sqlx::query(
                        "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, mark, speaker_idx, sentences)
                         SELECT ?, meeting_id, ?, timestamp, ?, ?, ?, NULL, ?, ? FROM transcripts WHERE id = ? AND meeting_id = ?",
                    )
                    .bind(format!("transcript-{}", Uuid::new_v4()))
                    .bind(&l.text)
                    .bind(l.start)
                    .bind(l.end)
                    .bind(duration)
                    .bind(l.speaker_idx)
                    .bind(sentences)
                    .bind(&l.source_id)
                    .bind(meeting_id)
                    .execute(&mut *tx)
                    .await?
                }
            };
            if done.rows_affected() == 0 {
                return Err(SqlxError::RowNotFound);
            }
        }

        if let Some((v, seconds)) = &p.self_print {
            upsert_print(&mut *tx, SELF_ID, meeting_id, 0, v, *seconds).await?;
            prune(&mut *tx, SELF_ID).await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    pub async fn get_job(pool: &SqlitePool, meeting_id: &str) -> Result<Option<JobRow>, SqlxError> {
        let row: Option<JobTuple> = sqlx::query_as(
            "SELECT meeting_id, status, reason, backend, run_token, audio_seconds, run_seconds, updated_at
             FROM speaker_jobs WHERE meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(job_of))
    }

    pub async fn last_done_job(pool: &SqlitePool) -> Result<Option<JobRow>, SqlxError> {
        let row: Option<JobTuple> = sqlx::query_as(
            "SELECT meeting_id, status, reason, backend, run_token, audio_seconds, run_seconds, updated_at
             FROM speaker_jobs WHERE status = 'done' ORDER BY updated_at DESC LIMIT 1",
        )
        .fetch_optional(pool)
        .await?;
        Ok(row.map(job_of))
    }

    pub async fn meeting_speakers(pool: &SqlitePool, meeting_id: &str) -> Result<Vec<SpeakerView>, SqlxError> {
        let rows: Vec<(i64, String, Option<String>, i64, f64)> = sqlx::query_as(
            "SELECT ms.idx, COALESCE(p.name, ms.name, 'Speaker ' || ms.idx), ms.person_id, COALESCE(p.is_self, 0), ms.seconds
             FROM meeting_speakers ms LEFT JOIN people p ON p.id = ms.person_id
             WHERE ms.meeting_id = ? ORDER BY ms.idx",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(idx, name, person_id, is_self, seconds)| SpeakerView { idx, name, person_id, is_self: is_self != 0, seconds })
            .collect())
    }

    /// Self's prints, and every other saved person with at least one usable print.
    pub async fn known_people(pool: &SqlitePool) -> Result<(Vec<Vec<f32>>, Vec<KnownPerson>), SqlxError> {
        let rows: Vec<(String, i64, Vec<u8>)> = sqlx::query_as(
            "SELECT v.person_id, p.is_self, v.vector FROM voiceprints v JOIN people p ON p.id = v.person_id
             ORDER BY v.created_at, v.rowid",
        )
        .fetch_all(pool)
        .await?;
        let mut self_prints = Vec::new();
        let mut people: BTreeMap<String, Vec<Vec<f32>>> = BTreeMap::new();
        for (person_id, is_self, bytes) in rows {
            let Some(v) = decode(&bytes) else { continue };
            if is_self != 0 {
                self_prints.push(v);
            } else {
                people.entry(person_id).or_default().push(v);
            }
        }
        let known = people.into_iter().map(|(person_id, prints)| KnownPerson { person_id, prints }).collect();
        Ok((self_prints, known))
    }

    /// Names a speaker and returns the person's id. Learns a voiceprint if the speaker has enough speech.
    pub async fn name_speaker(pool: &SqlitePool, meeting_id: &str, idx: i64, target: NameTarget) -> Result<String, String> {
        if idx == 0 {
            return Err("You are speaker 0 and can't be named".into());
        }
        let mut tx = pool.begin().await.map_err(db)?;
        ensure_idle(&mut *tx, meeting_id).await?;
        let person_id = match target {
            NameTarget::New(name) => {
                let name = clean_name(&name)?;
                let id = format!("person-{}", Uuid::new_v4());
                sqlx::query("INSERT INTO people (id, name, is_self, created_at) VALUES (?, ?, 0, ?)")
                    .bind(&id)
                    .bind(&name)
                    .bind(now())
                    .execute(&mut *tx)
                    .await
                    .map_err(db)?;
                id
            }
            NameTarget::Person(id) => {
                let is_self: Option<(i64,)> = sqlx::query_as("SELECT is_self FROM people WHERE id = ?")
                    .bind(&id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(db)?;
                match is_self {
                    None => return Err("Person not found".into()),
                    Some((1,)) => return Err("You can't be named here".into()),
                    Some(_) => id,
                }
            }
        };
        let row: Option<(Option<Vec<u8>>, f64)> =
            sqlx::query_as("SELECT centroid, seconds FROM meeting_speakers WHERE meeting_id = ? AND idx = ?")
                .bind(meeting_id)
                .bind(idx)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db)?;
        let Some((centroid, seconds)) = row else { return Err("Speaker not found".into()) };
        sqlx::query("UPDATE meeting_speakers SET person_id = ? WHERE meeting_id = ? AND idx = ?")
            .bind(&person_id)
            .bind(meeting_id)
            .bind(idx)
            .execute(&mut *tx)
            .await
            .map_err(db)?;

        // Renaming replaces what this speaker taught before, so a speaker who no longer teaches drops its print.
        match centroid.as_deref().and_then(decode) {
            Some(v) if seconds >= LEARN_MIN_SECONDS => {
                upsert_print(&mut *tx, &person_id, meeting_id, idx, &v, seconds).await.map_err(db)?;
                prune(&mut *tx, &person_id).await.map_err(db)?;
            }
            _ => {
                sqlx::query("DELETE FROM voiceprints WHERE source_meeting_id = ? AND source_idx = ?")
                    .bind(meeting_id)
                    .bind(idx)
                    .execute(&mut *tx)
                    .await
                    .map_err(db)?;
            }
        }
        tx.commit().await.map_err(db)?;
        Ok(person_id)
    }

    /// Moves one line to a speaker of its own meeting, or to nobody (`None`).
    pub async fn move_line(pool: &SqlitePool, transcript_id: &str, idx: Option<i64>) -> Result<(), String> {
        let mut tx = pool.begin().await.map_err(db)?;
        let meeting: Option<(String,)> = sqlx::query_as("SELECT meeting_id FROM transcripts WHERE id = ?")
            .bind(transcript_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
        let Some((meeting_id,)) = meeting else { return Err("Line not found".into()) };
        ensure_idle(&mut *tx, &meeting_id).await?;
        if let Some(idx) = idx {
            let known: Option<(i64,)> = sqlx::query_as("SELECT idx FROM meeting_speakers WHERE meeting_id = ? AND idx = ?")
                .bind(&meeting_id)
                .bind(idx)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db)?;
            if known.is_none() {
                return Err("Speaker not found in this meeting".into());
            }
        }
        sqlx::query("UPDATE transcripts SET speaker_idx = ? WHERE id = ?")
            .bind(idx)
            .bind(transcript_id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        tx.commit().await.map_err(db)
    }

    pub async fn list_people(pool: &SqlitePool) -> Result<Vec<PersonView>, SqlxError> {
        let rows: Vec<(String, String, i64, i64, Option<String>)> = sqlx::query_as(
            "SELECT p.id, p.name, p.is_self,
               (SELECT COUNT(DISTINCT ms.meeting_id) FROM meeting_speakers ms WHERE ms.person_id = p.id),
               (SELECT MAX(m.created_at) FROM meeting_speakers ms JOIN meetings m ON m.id = ms.meeting_id WHERE ms.person_id = p.id)
             FROM people p ORDER BY p.is_self DESC, p.name COLLATE NOCASE, p.id",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, is_self, meetings, last_heard)| PersonView { id, name, is_self: is_self != 0, meetings, last_heard })
            .collect())
    }

    pub async fn rename_person(pool: &SqlitePool, id: &str, name: &str) -> Result<(), String> {
        let name = clean_name(name)?;
        let r = sqlx::query("UPDATE people SET name = ? WHERE id = ?")
            .bind(&name)
            .bind(id)
            .execute(pool)
            .await
            .map_err(db)?;
        if r.rows_affected() == 0 {
            return Err("Person not found".into());
        }
        Ok(())
    }

    /// Past meetings keep the name. The FKs then clear person_id and cascade the prints.
    pub async fn delete_person(pool: &SqlitePool, id: &str) -> Result<(), String> {
        let mut tx = pool.begin().await.map_err(db)?;
        let person: Option<(i64,)> = sqlx::query_as("SELECT is_self FROM people WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
        match person {
            None => return Err("Person not found".into()),
            Some((1,)) => return Err("You can't be deleted".into()),
            Some(_) => {}
        }
        sqlx::query("UPDATE meeting_speakers SET name = (SELECT name FROM people WHERE id = ?) WHERE person_id = ?")
            .bind(id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        sqlx::query("DELETE FROM people WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        tx.commit().await.map_err(db)
    }

    /// Moves `from`'s prints and meeting speakers to `into`, keeps the 20 newest prints, then deletes `from`.
    pub async fn merge_people(pool: &SqlitePool, from: &str, into: &str) -> Result<(), String> {
        if from == into {
            return Err("A person can't be merged into themselves".into());
        }
        let mut tx = pool.begin().await.map_err(db)?;
        let people: Vec<(String, i64)> = sqlx::query_as("SELECT id, is_self FROM people WHERE id IN (?, ?)")
            .bind(from)
            .bind(into)
            .fetch_all(&mut *tx)
            .await
            .map_err(db)?;
        if people.len() != 2 {
            return Err("Person not found".into());
        }
        if people.iter().any(|(_, is_self)| *is_self != 0) {
            return Err("You can't be merged".into());
        }
        sqlx::query("UPDATE voiceprints SET person_id = ? WHERE person_id = ?")
            .bind(into)
            .bind(from)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        sqlx::query("UPDATE meeting_speakers SET person_id = ? WHERE person_id = ?")
            .bind(into)
            .bind(from)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        prune(&mut *tx, into).await.map_err(db)?;
        sqlx::query("DELETE FROM people WHERE id = ?")
            .bind(from)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        tx.commit().await.map_err(db)
    }
}

type JobTuple = (String, String, Option<String>, Option<String>, String, Option<f64>, Option<f64>, String);

fn job_of(t: JobTuple) -> JobRow {
    JobRow {
        meeting_id: t.0,
        status: t.1,
        reason: t.2,
        backend: t.3,
        run_token: t.4,
        audio_seconds: t.5,
        run_seconds: t.6,
        updated_at: t.7,
    }
}

fn now() -> String {
    // Fixed width, so the text sorts by time.
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn db(e: SqlxError) -> String {
    e.to_string()
}

/// 192 little-endian f32 values, the only size the schema holds.
fn encode(v: &[f32]) -> Result<Vec<u8>, SqlxError> {
    if v.len() != DIM {
        return Err(SqlxError::Protocol(format!("speaker vector has {} values, expected {DIM}", v.len())));
    }
    if v.iter().any(|f| !f.is_finite()) {
        return Err(SqlxError::Protocol("speaker vector has a non-finite value".into()));
    }
    Ok(v.iter().flat_map(|f| f.to_le_bytes()).collect())
}

/// Any other stored length, or a non-finite value, is skipped with a warning.
fn decode(b: &[u8]) -> Option<Vec<f32>> {
    if b.len() != DIM * 4 {
        warn!("speakers: skipped a stored vector of {} bytes", b.len());
        return None;
    }
    let v: Vec<f32> = b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    if v.iter().any(|f| !f.is_finite()) {
        warn!("speakers: skipped a stored vector with a non-finite value");
        return None;
    }
    Some(v)
}

fn clean_name(name: &str) -> Result<String, String> {
    let t = name.trim();
    if (1..=NAME_MAX_CHARS).contains(&t.chars().count()) {
        Ok(t.to_string())
    } else {
        Err(format!("Name must be 1 to {NAME_MAX_CHARS} characters"))
    }
}

async fn ensure_idle(conn: &mut SqliteConnection, meeting_id: &str) -> Result<(), String> {
    let job: Option<(String,)> = sqlx::query_as("SELECT status FROM speaker_jobs WHERE meeting_id = ?")
        .bind(meeting_id)
        .fetch_optional(conn)
        .await
        .map_err(db)?;
    match job {
        Some((s,)) if s == "pending" || s == "running" => Err(RUNNING.into()),
        _ => Ok(()),
    }
}

/// Writes the print for (meeting, idx), replacing any earlier print from that speaker.
async fn upsert_print(conn: &mut SqliteConnection, person_id: &str, meeting_id: &str, idx: i64, v: &[f32], seconds: f64) -> Result<(), SqlxError> {
    sqlx::query(
        "INSERT INTO voiceprints (id, person_id, source_meeting_id, source_idx, vector, seconds, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(source_meeting_id, source_idx) DO UPDATE SET
           person_id = excluded.person_id, vector = excluded.vector, seconds = excluded.seconds, created_at = excluded.created_at",
    )
    .bind(format!("voiceprint-{}", Uuid::new_v4()))
    .bind(person_id)
    .bind(meeting_id)
    .bind(idx)
    .bind(encode(v)?)
    .bind(seconds)
    .bind(now())
    .execute(conn)
    .await?;
    Ok(())
}

/// Keeps the person's 20 newest prints.
async fn prune(conn: &mut SqliteConnection, person_id: &str) -> Result<(), SqlxError> {
    sqlx::query(
        "DELETE FROM voiceprints WHERE person_id = ? AND id NOT IN (
           SELECT id FROM voiceprints WHERE person_id = ? ORDER BY created_at DESC, rowid DESC LIMIT ?)",
    )
    .bind(person_id)
    .bind(person_id)
    .bind(PRINTS_PER_PERSON)
    .execute(conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::meeting::MeetingsRepository;
    use crate::database::repositories::test_pool;
    use crate::database::repositories::transcript::TranscriptsRepository;

    fn speaker(idx: i64, person: Option<&str>, seconds: f64) -> ResolvedSpeaker {
        ResolvedSpeaker {
            idx,
            person_id: person.map(str::to_string),
            centroid: Some(vec![0.5; DIM]),
            seconds,
            turns: vec![],
        }
    }

    fn publish_of(speakers: Vec<ResolvedSpeaker>, lines: Vec<LineOut>, self_print: Option<f64>) -> Publish {
        Publish {
            speakers,
            lines,
            self_print: self_print.map(|s| (vec![0.25; DIM], s)),
            backend: "cpu".into(),
            audio_seconds: 3600.0,
            run_seconds: 120.0,
        }
    }

    async fn meeting(pool: &SqlitePool, feature: bool) -> String {
        TranscriptsRepository::save_transcript(pool, "M", &[], None, &[], feature).await.unwrap()
    }

    async fn line(pool: &SqlitePool, meeting_id: &str, start: f64, end: f64) -> String {
        let id = format!("transcript-{}", Uuid::new_v4());
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration)
             VALUES (?, ?, 'hi', 'ts-1', ?, ?, ?)",
        )
        .bind(&id)
        .bind(meeting_id)
        .bind(start)
        .bind(end)
        .bind(end - start)
        .execute(pool)
        .await
        .unwrap();
        id
    }

    async fn new_person(pool: &SqlitePool, name: &str) -> String {
        let id = format!("person-{}", Uuid::new_v4());
        sqlx::query("INSERT INTO people (id, name, is_self, created_at) VALUES (?, ?, 0, 'ts-1')")
            .bind(&id)
            .bind(name)
            .execute(pool)
            .await
            .unwrap();
        id
    }

    /// Claims the next pending job; it must be `meeting_id`.
    async fn claim(pool: &SqlitePool, meeting_id: &str) -> String {
        let (id, token) = SpeakersRepository::claim_next(pool).await.unwrap().unwrap();
        assert_eq!(id, meeting_id);
        token
    }

    /// Enqueues, claims and publishes one whole job.
    async fn run_job(pool: &SqlitePool, meeting_id: &str, p: Publish) -> bool {
        SpeakersRepository::enqueue(pool, meeting_id).await.unwrap();
        let token = claim(pool, meeting_id).await;
        SpeakersRepository::publish(pool, meeting_id, &token, &p).await.unwrap()
    }

    /// Gives `person` one learned voiceprint in each of `n` new meetings.
    async fn learn(pool: &SqlitePool, person: &str, n: usize) -> Vec<String> {
        let mut meetings = vec![];
        for _ in 0..n {
            let m = meeting(pool, true).await;
            assert!(run_job(pool, &m, publish_of(vec![speaker(1, None, 25.0)], vec![], None)).await);
            SpeakersRepository::name_speaker(pool, &m, 1, NameTarget::Person(person.into())).await.unwrap();
            meetings.push(m);
        }
        meetings
    }

    async fn status(pool: &SqlitePool, meeting_id: &str) -> String {
        SpeakersRepository::get_job(pool, meeting_id).await.unwrap().unwrap().status
    }

    async fn count(pool: &SqlitePool, sql: &str, bind: &str) -> i64 {
        sqlx::query_as::<_, (i64,)>(sql).bind(bind).fetch_one(pool).await.unwrap().0
    }

    async fn print_owners(pool: &SqlitePool, meeting_id: &str, idx: i64) -> Vec<String> {
        sqlx::query_as::<_, (String,)>("SELECT person_id FROM voiceprints WHERE source_meeting_id = ? AND source_idx = ?")
            .bind(meeting_id)
            .bind(idx)
            .fetch_all(pool)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.0)
            .collect()
    }

    #[tokio::test]
    async fn save_without_feature_creates_no_job() {
        let pool = test_pool().await;
        let m = meeting(&pool, false).await;
        assert!(SpeakersRepository::get_job(&pool, &m).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn save_with_feature_creates_pending_job() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        let job = SpeakersRepository::get_job(&pool, &m).await.unwrap().unwrap();
        assert_eq!(job.status, "pending");
        assert_eq!(job.reason, None);
        assert!(!job.run_token.is_empty());
    }

    #[tokio::test]
    async fn claim_and_publish_with_matching_token() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        let old = line(&pool, &m, 0.0, 4.0).await;
        let token = claim(&pool, &m).await;
        assert_eq!(status(&pool, &m).await, "running");
        let lines = vec![
            LineOut { id: Some(old.clone()), source_id: old.clone(), start: 0.0, end: 2.0, text: "Hi.".into(), sentences: None, speaker_idx: Some(1) },
            LineOut { id: None, source_id: old.clone(), start: 2.0, end: 4.5, text: "Bye.".into(), sentences: None, speaker_idx: Some(2) },
        ];
        let p = publish_of(vec![speaker(0, Some(SELF_ID), 30.0), speaker(1, None, 25.0), speaker(2, None, 5.0)], lines, None);
        assert!(SpeakersRepository::publish(&pool, &m, &token, &p).await.unwrap());

        let job = SpeakersRepository::get_job(&pool, &m).await.unwrap().unwrap();
        assert_eq!((job.status.as_str(), job.backend.as_deref()), ("done", Some("cpu")));
        assert_eq!(SpeakersRepository::last_done_job(&pool).await.unwrap().unwrap().meeting_id, m);

        let speakers = SpeakersRepository::meeting_speakers(&pool, &m).await.unwrap();
        let shown: Vec<(i64, &str, bool)> = speakers.iter().map(|s| (s.idx, s.name.as_str(), s.is_self)).collect();
        assert_eq!(shown, vec![(0, "You", true), (1, "Speaker 1", false), (2, "Speaker 2", false)]);

        let updated: (String, f64, f64, Option<i64>) =
            sqlx::query_as("SELECT transcript, audio_end_time, duration, speaker_idx FROM transcripts WHERE id = ?")
                .bind(&old)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(updated, ("Hi.".into(), 2.0, 2.0, Some(1)));

        let split: (String, String, String, f64, f64, f64, Option<i64>, Option<String>) = sqlx::query_as(
            "SELECT id, timestamp, transcript, audio_start_time, audio_end_time, duration, speaker_idx, mark
             FROM transcripts WHERE meeting_id = ? AND id <> ?",
        )
        .bind(&m)
        .bind(&old)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(split.0.starts_with("transcript-"));
        assert_eq!(split.1, "ts-1");
        assert_eq!((split.2.as_str(), split.3, split.4, split.5), ("Bye.", 2.0, 4.5, 2.5));
        assert_eq!((split.6, split.7), (Some(2), None));
    }

    #[tokio::test]
    async fn publish_with_stale_token_does_nothing() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        let old = line(&pool, &m, 0.0, 4.0).await;
        let token = claim(&pool, &m).await;
        SpeakersRepository::enqueue(&pool, &m).await.unwrap();
        let lines = vec![LineOut { id: Some(old.clone()), source_id: old.clone(), start: 0.0, end: 2.0, text: "Hi.".into(), sentences: None, speaker_idx: Some(1) }];
        let p = publish_of(vec![speaker(1, None, 25.0)], lines, Some(30.0));
        assert!(!SpeakersRepository::publish(&pool, &m, &token, &p).await.unwrap());
        assert_eq!(status(&pool, &m).await, "pending");
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM meeting_speakers WHERE meeting_id = ?", &m).await, 0);
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM voiceprints WHERE source_meeting_id = ?", &m).await, 0);
        let (end,): (f64,) = sqlx::query_as("SELECT audio_end_time FROM transcripts WHERE id = ?").bind(&old).fetch_one(&pool).await.unwrap();
        assert_eq!(end, 4.0);
    }

    #[tokio::test]
    async fn finish_with_stale_token_does_nothing() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        let token = claim(&pool, &m).await;
        SpeakersRepository::enqueue(&pool, &m).await.unwrap();
        assert!(!SpeakersRepository::finish_without_publish(&pool, &m, &token, "failed", Some("boom"), None).await.unwrap());
        assert_eq!(status(&pool, &m).await, "pending");

        // The current owner can still finish its own run.
        let current = claim(&pool, &m).await;
        assert!(SpeakersRepository::finish_without_publish(&pool, &m, &current, "failed", Some("boom"), None).await.unwrap());
        let job = SpeakersRepository::get_job(&pool, &m).await.unwrap().unwrap();
        assert_eq!((job.status.as_str(), job.reason.as_deref()), ("failed", Some("boom")));
    }

    #[tokio::test]
    async fn replacing_transcripts_with_feature_off_invalidates_running_job() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        let token = claim(&pool, &m).await;
        let mut tx = pool.begin().await.unwrap();
        SpeakersRepository::transcripts_replaced_in_tx(&mut tx, &m, false).await.unwrap();
        tx.commit().await.unwrap();
        let p = publish_of(vec![speaker(1, None, 25.0)], vec![], None);
        assert!(!SpeakersRepository::publish(&pool, &m, &token, &p).await.unwrap());
        let job = SpeakersRepository::get_job(&pool, &m).await.unwrap().unwrap();
        assert_eq!((job.status.as_str(), job.reason.as_deref()), ("skipped", Some("feature_off")));
    }

    #[tokio::test]
    async fn publish_after_meeting_deleted_does_nothing() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        let token = claim(&pool, &m).await;
        assert!(MeetingsRepository::delete_meeting(&pool, &m).await.unwrap());
        let p = publish_of(vec![speaker(1, None, 25.0)], vec![], Some(30.0));
        assert!(!SpeakersRepository::publish(&pool, &m, &token, &p).await.unwrap());
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM meeting_speakers WHERE meeting_id = ?", &m).await, 0);
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM voiceprints WHERE source_meeting_id = ?", &m).await, 0);
        assert!(SpeakersRepository::get_job(&pool, &m).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn recover_requeues_running_with_new_token() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        let token = claim(&pool, &m).await;
        assert_eq!(SpeakersRepository::recover_jobs(&pool).await.unwrap(), 1);
        let job = SpeakersRepository::get_job(&pool, &m).await.unwrap().unwrap();
        assert_eq!(job.status, "pending");
        assert_ne!(job.run_token, token);
        // The run from before the restart can no longer publish.
        assert!(!SpeakersRepository::publish(&pool, &m, &token, &publish_of(vec![speaker(1, None, 25.0)], vec![], None)).await.unwrap());
        // The re-run can.
        let token2 = claim(&pool, &m).await;
        assert!(SpeakersRepository::publish(&pool, &m, &token2, &publish_of(vec![speaker(1, None, 25.0)], vec![], None)).await.unwrap());
    }

    #[tokio::test]
    async fn publish_removes_prints_learned_from_this_meeting_on_rerun() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        assert!(run_job(&pool, &m, publish_of(vec![speaker(1, None, 25.0)], vec![], None)).await);
        let alice = SpeakersRepository::name_speaker(&pool, &m, 1, NameTarget::New("Alice".into())).await.unwrap();
        assert_eq!(print_owners(&pool, &m, 1).await, vec![alice]);
        // Re-identify: the speaker is not named any more after the rerun.
        assert!(run_job(&pool, &m, publish_of(vec![speaker(1, None, 25.0)], vec![], None)).await);
        assert!(print_owners(&pool, &m, 1).await.is_empty());
        let (known_self, known) = SpeakersRepository::known_people(&pool).await.unwrap();
        assert!(known_self.is_empty());
        assert!(known.is_empty());
    }

    #[tokio::test]
    async fn name_speaker_learns_once_and_replaces_on_rename() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        assert!(run_job(&pool, &m, publish_of(vec![speaker(1, None, 25.0)], vec![], None)).await);
        let alice = SpeakersRepository::name_speaker(&pool, &m, 1, NameTarget::New("Alice".into())).await.unwrap();
        assert!(alice.starts_with("person-"));
        assert_eq!(print_owners(&pool, &m, 1).await, vec![alice.clone()]);

        let bob = SpeakersRepository::name_speaker(&pool, &m, 1, NameTarget::New(" Bob ".into())).await.unwrap();
        assert_ne!(bob, alice);
        assert_eq!(print_owners(&pool, &m, 1).await, vec![bob.clone()]);
        // Naming to an existing person returns that person's id.
        assert_eq!(SpeakersRepository::name_speaker(&pool, &m, 1, NameTarget::Person(bob.clone())).await.unwrap(), bob);
        assert_eq!(print_owners(&pool, &m, 1).await, vec![bob]);
    }

    #[tokio::test]
    async fn name_speaker_under_20s_does_not_learn() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        assert!(run_job(&pool, &m, publish_of(vec![speaker(1, None, 19.9)], vec![], None)).await);
        let alice = SpeakersRepository::name_speaker(&pool, &m, 1, NameTarget::New("Alice".into())).await.unwrap();
        assert!(print_owners(&pool, &m, 1).await.is_empty());
        let row = SpeakersRepository::meeting_speakers(&pool, &m).await.unwrap().into_iter().find(|s| s.idx == 1).unwrap();
        assert_eq!((row.name.as_str(), row.person_id.as_deref()), ("Alice", Some(alice.as_str())));
    }

    #[tokio::test]
    async fn name_refused_while_job_running() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        assert!(run_job(&pool, &m, publish_of(vec![speaker(1, None, 25.0)], vec![], None)).await);
        SpeakersRepository::enqueue(&pool, &m).await.unwrap();
        let refused = Err("Speaker analysis is running".to_string());
        assert_eq!(SpeakersRepository::name_speaker(&pool, &m, 1, NameTarget::New("Alice".into())).await, refused);
        claim(&pool, &m).await;
        assert_eq!(SpeakersRepository::name_speaker(&pool, &m, 1, NameTarget::New("Alice".into())).await, refused);
    }

    #[tokio::test]
    async fn name_self_refused() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        assert!(run_job(&pool, &m, publish_of(vec![speaker(0, Some(SELF_ID), 30.0), speaker(1, None, 25.0)], vec![], Some(30.0))).await);
        assert!(SpeakersRepository::name_speaker(&pool, &m, 0, NameTarget::New("Me".into())).await.is_err());
        assert!(SpeakersRepository::name_speaker(&pool, &m, 1, NameTarget::Person(SELF_ID.into())).await.is_err());
        assert!(print_owners(&pool, &m, 1).await.is_empty());
        assert_eq!(SpeakersRepository::list_people(&pool).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn prints_capped_at_20_newest() {
        let pool = test_pool().await;
        for i in 0..21 {
            let m = meeting(&pool, true).await;
            assert!(run_job(&pool, &m, publish_of(vec![speaker(0, Some(SELF_ID), 30.0)], vec![], Some(i as f64))).await);
        }
        let kept: Vec<(f64,)> = sqlx::query_as("SELECT seconds FROM voiceprints WHERE person_id = ? ORDER BY seconds")
            .bind(SELF_ID)
            .fetch_all(&pool)
            .await
            .unwrap();
        let kept: Vec<f64> = kept.into_iter().map(|r| r.0).collect();
        assert_eq!(kept.len(), 20);
        assert_eq!((kept[0], kept[19]), (1.0, 20.0));
    }

    #[tokio::test]
    async fn delete_person_keeps_name_on_past_meetings() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        assert!(run_job(&pool, &m, publish_of(vec![speaker(1, None, 25.0)], vec![], None)).await);
        let alice = SpeakersRepository::name_speaker(&pool, &m, 1, NameTarget::New("Alice".into())).await.unwrap();
        SpeakersRepository::delete_person(&pool, &alice).await.unwrap();
        let row = SpeakersRepository::meeting_speakers(&pool, &m).await.unwrap().into_iter().find(|s| s.idx == 1).unwrap();
        assert_eq!((row.name.as_str(), row.person_id), ("Alice", None));
        assert!(print_owners(&pool, &m, 1).await.is_empty());
        assert!(SpeakersRepository::delete_person(&pool, &alice).await.is_err());
    }

    #[tokio::test]
    async fn merge_moves_prints_and_speakers_and_prunes() {
        let pool = test_pool().await;
        let alice = new_person(&pool, "Alice").await;
        let bob = new_person(&pool, "Bob").await;
        let alice_meetings = learn(&pool, &alice, 15).await;
        learn(&pool, &bob, 15).await;

        SpeakersRepository::merge_people(&pool, &alice, &bob).await.unwrap();
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM people WHERE id = ?", &alice).await, 0);
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM voiceprints WHERE person_id = ?", &bob).await, 20);
        assert!(print_owners(&pool, &alice_meetings[0], 1).await.is_empty());
        let moved = SpeakersRepository::meeting_speakers(&pool, &alice_meetings[0]).await.unwrap();
        assert_eq!(moved.into_iter().find(|s| s.idx == 1).unwrap().person_id, Some(bob));
    }

    #[tokio::test]
    async fn self_cannot_be_deleted_or_merged() {
        let pool = test_pool().await;
        let alice = new_person(&pool, "Alice").await;
        assert!(SpeakersRepository::delete_person(&pool, SELF_ID).await.is_err());
        assert!(SpeakersRepository::merge_people(&pool, SELF_ID, &alice).await.is_err());
        assert!(SpeakersRepository::merge_people(&pool, &alice, SELF_ID).await.is_err());
        assert!(SpeakersRepository::merge_people(&pool, &alice, &alice).await.is_err());
        // Self can be renamed.
        SpeakersRepository::rename_person(&pool, SELF_ID, "Me").await.unwrap();
        let people = SpeakersRepository::list_people(&pool).await.unwrap();
        assert_eq!(people.iter().find(|p| p.is_self).unwrap().name, "Me");
    }

    #[tokio::test]
    async fn move_line_validates_idx_belongs_to_meeting() {
        let pool = test_pool().await;
        let m1 = meeting(&pool, true).await;
        let l1 = line(&pool, &m1, 0.0, 2.0).await;
        assert!(run_job(&pool, &m1, publish_of(vec![speaker(0, Some(SELF_ID), 30.0), speaker(1, None, 25.0)], vec![], None)).await);
        let m2 = meeting(&pool, true).await;
        let l2 = line(&pool, &m2, 0.0, 2.0).await;
        assert!(run_job(&pool, &m2, publish_of(vec![speaker(0, Some(SELF_ID), 30.0)], vec![], None)).await);

        assert!(SpeakersRepository::move_line(&pool, &l1, Some(1)).await.is_ok());
        assert!(SpeakersRepository::move_line(&pool, &l1, Some(7)).await.is_err());
        assert!(SpeakersRepository::move_line(&pool, &l2, Some(1)).await.is_err());
        assert!(SpeakersRepository::move_line(&pool, &l2, Some(0)).await.is_ok());
        assert!(SpeakersRepository::move_line(&pool, "nope", None).await.is_err());
        assert!(SpeakersRepository::move_line(&pool, &l1, None).await.is_ok());
        let (idx,): (Option<i64>,) = sqlx::query_as("SELECT speaker_idx FROM transcripts WHERE id = ?").bind(&l1).fetch_one(&pool).await.unwrap();
        assert_eq!(idx, None);

        // Moves are refused while the meeting's analysis is queued.
        SpeakersRepository::enqueue(&pool, &m1).await.unwrap();
        assert_eq!(SpeakersRepository::move_line(&pool, &l1, Some(1)).await, Err("Speaker analysis is running".to_string()));
    }

    #[tokio::test]
    async fn bad_vector_blob_is_skipped() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        assert!(run_job(&pool, &m, publish_of(vec![speaker(0, Some(SELF_ID), 30.0)], vec![], Some(30.0))).await);
        let alice = new_person(&pool, "Alice").await;
        sqlx::query("INSERT INTO voiceprints (id, person_id, source_meeting_id, source_idx, vector, seconds, created_at) VALUES ('bad-1', ?, NULL, NULL, X'0102', 1.0, 'ts-1')")
            .bind(&alice)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO voiceprints (id, person_id, source_meeting_id, source_idx, vector, seconds, created_at) VALUES ('bad-2', ?, NULL, NULL, X'0102', 1.0, 'ts-1')")
            .bind(SELF_ID)
            .execute(&pool)
            .await
            .unwrap();
        let (self_prints, people) = SpeakersRepository::known_people(&pool).await.unwrap();
        assert_eq!(self_prints.len(), 1);
        assert!(people.is_empty());
    }

    #[tokio::test]
    async fn wrong_length_vector_is_refused_on_write() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        let mut bad = speaker(1, None, 25.0);
        bad.centroid = Some(vec![0.5; 3]);
        SpeakersRepository::enqueue(&pool, &m).await.unwrap();
        let token = claim(&pool, &m).await;
        assert!(SpeakersRepository::publish(&pool, &m, &token, &publish_of(vec![bad], vec![], None)).await.is_err());
        // The failed publish rolled back, so the run is still the owner's.
        assert_eq!(status(&pool, &m).await, "running");
    }

    #[tokio::test]
    async fn publish_tolerates_people_deleted_or_merged_mid_run() {
        let pool = test_pool().await;
        let alice = new_person(&pool, "Alice").await;
        let bob = new_person(&pool, "Bob").await;
        let carol = new_person(&pool, "Carol").await;
        let m = meeting(&pool, true).await;
        SpeakersRepository::enqueue(&pool, &m).await.unwrap();
        let token = claim(&pool, &m).await;
        SpeakersRepository::delete_person(&pool, &alice).await.unwrap();
        SpeakersRepository::merge_people(&pool, &bob, &carol).await.unwrap();
        let p = publish_of(vec![speaker(1, Some(&alice), 25.0), speaker(2, Some(&bob), 25.0)], vec![], None);
        assert!(SpeakersRepository::publish(&pool, &m, &token, &p).await.unwrap());
        let shown: Vec<(i64, String, Option<String>)> = SpeakersRepository::meeting_speakers(&pool, &m)
            .await
            .unwrap()
            .into_iter()
            .map(|s| (s.idx, s.name, s.person_id))
            .collect();
        assert_eq!(shown, vec![(1, "Speaker 1".into(), None), (2, "Speaker 2".into(), None)]);
    }

    #[tokio::test]
    async fn replacing_transcripts_with_feature_off_and_no_job_writes_nothing() {
        let pool = test_pool().await;
        let m = meeting(&pool, false).await;
        let mut tx = pool.begin().await.unwrap();
        SpeakersRepository::transcripts_replaced_in_tx(&mut tx, &m, false).await.unwrap();
        tx.commit().await.unwrap();
        assert!(SpeakersRepository::get_job(&pool, &m).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn replacing_transcripts_with_feature_off_reports_a_skipped_pending_job() {
        let pool = test_pool().await;
        let m = meeting(&pool, false).await;
        SpeakersRepository::enqueue(&pool, &m).await.unwrap();
        let before = SpeakersRepository::get_job(&pool, &m).await.unwrap().unwrap();
        let mut tx = pool.begin().await.unwrap();
        let changed = SpeakersRepository::transcripts_replaced_in_tx(&mut tx, &m, false).await.unwrap();
        tx.commit().await.unwrap();
        assert!(changed, "an existing pending job is skipped, which the UI must hear about");
        let after = SpeakersRepository::get_job(&pool, &m).await.unwrap().unwrap();
        assert_eq!((after.status.as_str(), after.reason.as_deref()), ("skipped", Some("feature_off")));
        assert_ne!(after.run_token, before.run_token, "the in-flight run is overtaken");
    }

    #[tokio::test]
    async fn replacing_transcripts_with_feature_on_queues_pending_job() {
        let pool = test_pool().await;
        let m = meeting(&pool, false).await;
        let mut tx = pool.begin().await.unwrap();
        SpeakersRepository::transcripts_replaced_in_tx(&mut tx, &m, true).await.unwrap();
        tx.commit().await.unwrap();
        let job = SpeakersRepository::get_job(&pool, &m).await.unwrap().unwrap();
        assert_eq!((job.status.as_str(), job.reason.as_deref()), ("pending", None));
    }

    #[tokio::test]
    async fn non_finite_vectors_are_refused_and_skipped() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        SpeakersRepository::enqueue(&pool, &m).await.unwrap();
        let token = claim(&pool, &m).await;
        let mut nan = speaker(1, None, 25.0);
        nan.centroid = Some(vec![f32::NAN; DIM]);
        assert!(SpeakersRepository::publish(&pool, &m, &token, &publish_of(vec![nan], vec![], None)).await.is_err());
        assert_eq!(status(&pool, &m).await, "running");

        // A non-finite print already stored is skipped on read
        let bad: Vec<u8> = (0..DIM).flat_map(|_| f32::INFINITY.to_le_bytes()).collect();
        sqlx::query("INSERT INTO voiceprints (id, person_id, source_meeting_id, source_idx, vector, seconds, created_at) VALUES ('bad-3', ?, NULL, NULL, ?, 1.0, 'ts-1')")
            .bind(SELF_ID)
            .bind(bad)
            .execute(&pool)
            .await
            .unwrap();
        let (self_prints, _) = SpeakersRepository::known_people(&pool).await.unwrap();
        assert!(self_prints.is_empty());
    }

    #[tokio::test]
    async fn rerun_keeps_learned_voice_under_new_index() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        assert!(run_job(&pool, &m, publish_of(vec![speaker(1, None, 25.0)], vec![], None)).await);
        let alice = SpeakersRepository::name_speaker(&pool, &m, 1, NameTarget::New("Alice".into())).await.unwrap();
        // Re-identify: Alice matches the same voice as idx 2
        assert!(run_job(&pool, &m, publish_of(vec![speaker(2, Some(&alice), 25.0)], vec![], None)).await);
        assert_eq!(print_owners(&pool, &m, 2).await, vec![alice.clone()]);
        assert!(print_owners(&pool, &m, 1).await.is_empty());
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM voiceprints WHERE source_meeting_id = ?", &m).await, 1);
    }

    #[tokio::test]
    async fn rerun_without_match_drops_the_meetings_print() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        assert!(run_job(&pool, &m, publish_of(vec![speaker(1, None, 25.0)], vec![], None)).await);
        let alice = SpeakersRepository::name_speaker(&pool, &m, 1, NameTarget::New("Alice".into())).await.unwrap();
        assert!(run_job(&pool, &m, publish_of(vec![speaker(1, None, 25.0)], vec![], None)).await);
        assert!(print_owners(&pool, &m, 1).await.is_empty());
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM voiceprints WHERE person_id = ? AND source_meeting_id = ?")
            .bind(&alice)
            .bind(&m)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn auto_matched_speaker_does_not_learn_into_a_new_meeting() {
        let pool = test_pool().await;
        let old = meeting(&pool, true).await;
        assert!(run_job(&pool, &old, publish_of(vec![speaker(1, None, 25.0)], vec![], None)).await);
        let alice = SpeakersRepository::name_speaker(&pool, &old, 1, NameTarget::New("Alice".into())).await.unwrap();
        // A different meeting where Alice is only matched by voice, never named
        let m = meeting(&pool, true).await;
        assert!(run_job(&pool, &m, publish_of(vec![speaker(1, Some(&alice), 25.0)], vec![], None)).await);
        assert!(print_owners(&pool, &m, 1).await.is_empty());
    }

    #[tokio::test]
    async fn publish_clears_labels_of_lines_left_out() {
        let pool = test_pool().await;
        let m = meeting(&pool, true).await;
        let kept = line(&pool, &m, 0.0, 2.0).await;
        let untimed = line(&pool, &m, 2.0, 4.0).await;
        sqlx::query("UPDATE transcripts SET speaker_idx = 1 WHERE id = ?").bind(&untimed).execute(&pool).await.unwrap();
        let lines = vec![LineOut { id: Some(kept.clone()), source_id: kept.clone(), start: 0.0, end: 2.0, text: "Hi.".into(), sentences: None, speaker_idx: Some(2) }];
        assert!(run_job(&pool, &m, publish_of(vec![speaker(1, None, 25.0), speaker(2, None, 25.0)], lines, None)).await);
        let label = |id: &str| sqlx::query_as::<_, (Option<i64>,)>("SELECT speaker_idx FROM transcripts WHERE id = ?").bind(id.to_string()).fetch_one(&pool);
        assert_eq!(label(&kept).await.unwrap().0, Some(2));
        assert_eq!(label(&untimed).await.unwrap().0, None);
    }

    #[tokio::test]
    async fn person_names_are_trimmed_and_limited() {
        let pool = test_pool().await;
        let alice = new_person(&pool, "Alice").await;
        assert!(SpeakersRepository::rename_person(&pool, &alice, "   ").await.is_err());
        assert!(SpeakersRepository::rename_person(&pool, &alice, &"x".repeat(81)).await.is_err());
        SpeakersRepository::rename_person(&pool, &alice, &"x".repeat(80)).await.unwrap();
        SpeakersRepository::rename_person(&pool, &alice, "  Bob  ").await.unwrap();
        let people = SpeakersRepository::list_people(&pool).await.unwrap();
        assert_eq!(people.into_iter().find(|p| p.id == alice).unwrap().name, "Bob");
    }
}
