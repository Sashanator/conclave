//! The report job. After a session ends it waits for any meeting recording to stop, re-transcribes
//! each learner turn word for word from you.wav, computes fluency, asks Claude for grammar,
//! unnatural and clarity items, and stores the report. Every outcome ends in 'ready' or 'failed'.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use log::warn;
use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio_util::sync::CancellationToken;

use crate::audio::common::acquire_engine_lifecycle_lock;
use crate::audio::recording_commands::is_recording_now;
use crate::audio::transcription::engine::get_or_init_whisper;
use crate::audio::vad::get_speech_chunks;
use crate::dictate;
use crate::state::AppState;

use super::audio::read_span;
use super::claude::{self, ClaudeError};
use super::commands::NOT_WHISPER;
use super::db::{self, TurnRow};
use super::files::PRACTICE_FILES;
use super::prompts::report_prompts;
use super::report::{
    count_fillers, items_from_json, long_pauses, looks_looped, schema, validate_items, word_count, wpm, Fluency,
    ItemKind, Report, ReportItem,
};
use super::{ModelHold, LONG_PAUSE_SECS, YOU_WAV_RATE};

/// Asks Whisper to write fillers and false starts down, so the transcript keeps them.
pub const VERBATIM_PROMPT: &str = "Um, uh, so, like, I mean, I- I was, uh, thinking that, you know, we, we had it.";
/// Pause length that splits speech into segments in the word-for-word pass.
const VERBATIM_REDEMPTION_MS: u32 = 300;
/// How often a waiting job checks whether a meeting recording has stopped.
const WAIT_POLL: Duration = Duration::from_secs(10);
/// Held while a report is claimed and while a session is deleted, so the two cannot interleave.
pub(super) static REPORT_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const SAVE_FAILED: &str = "Couldn't save the report — Retry";

#[derive(Clone, Serialize)]
pub struct ReportStatusPayload {
    pub session_id: String,
    pub status: &'static str,
}

/// One turn after the word-for-word pass. `timing` is (speech seconds, long pauses) for a learner turn with audio.
struct Heard {
    idx: i64,
    speaker: String,
    text: String,
    timing: Option<(f64, u32)>,
}

/// Runs the report for one session. The claim makes a second run for the same session a no-op.
/// `hold` keeps Whisper loaded across the hand-off from the session; the job releases it when done.
pub async fn run<R: Runtime>(app: AppHandle<R>, session_id: String, hold: ModelHold) {
    let pool = app.state::<AppState>().db_manager.pool().clone();
    match claim_for_report(&pool, &session_id).await {
        Ok(true) => {}
        other => {
            if let Err(e) = other {
                warn!("practice: could not start the report: {e}");
            }
            drop(hold);
            dictate::unload_if_idle().await;
            return;
        }
    }
    publish(&app, &session_id, "waiting");
    wait_while_recording().await;
    store(&app, &pool, &session_id, "running", None, None).await;
    // The analysis runs as its own task, so a panic in it still ends in 'failed'.
    let job = tauri::async_runtime::spawn(analyse(app.clone(), pool.clone(), session_id.clone(), hold));
    let result = match job.await {
        Ok(result) => result,
        Err(e) => {
            warn!("practice: the report task stopped: {e}");
            Err("The report stopped unexpectedly — Retry".to_string())
        }
    };
    dictate::unload_if_idle().await;
    let (status, json, error) = finish(result);
    save_final(&app, &pool, &session_id, status, json, error).await;
}

/// Claims the session's report under the gate that a delete also takes.
pub async fn claim_for_report(pool: &SqlitePool, session_id: &str) -> Result<bool, String> {
    let _gate = REPORT_GATE.lock().await;
    db::claim_report(pool, session_id).await
}

/// Maps the job's result to the stored status, report JSON and error. Pure, so the tests can check it.
pub fn finish(result: Result<Report, String>) -> (&'static str, Option<String>, Option<String>) {
    match result.and_then(|report| serde_json::to_string(&report).map_err(|e| e.to_string())) {
        Ok(json) => ("ready", Some(json), None),
        Err(e) => ("failed", None, Some(e)),
    }
}

/// Stores the final status, and tells the UI only once it is saved. A failed write is retried once as 'failed'.
async fn save_final<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    session_id: &str,
    status: &'static str,
    json: Option<String>,
    error: Option<String>,
) {
    if db::set_report(pool, session_id, status, json.as_deref(), error.as_deref())
        .await
        .is_ok()
    {
        publish(app, session_id, status);
        return;
    }
    warn!("practice: could not save the report");
    match db::set_report(pool, session_id, "failed", None, Some(SAVE_FAILED)).await {
        Ok(()) => publish(app, session_id, "failed"),
        Err(e) => warn!("practice: could not mark the report failed: {e}"),
    }
}

/// Stores a transitional status and tells the UI. A failed write is logged; the UI keeps the last status it saw.
async fn store<R: Runtime>(app: &AppHandle<R>, pool: &SqlitePool, session_id: &str, status: &'static str, json: Option<&str>, error: Option<&str>) {
    if let Err(e) = db::set_report(pool, session_id, status, json, error).await {
        warn!("practice: could not store the report status: {e}");
    }
    publish(app, session_id, status);
}

fn publish<R: Runtime>(app: &AppHandle<R>, session_id: &str, status: &'static str) {
    let _ = app.emit(
        "practice-report",
        ReportStatusPayload {
            session_id: session_id.to_string(),
            status,
        },
    );
}

/// Blocks while a meeting recording runs. The caller publishes the status around the wait.
async fn wait_while_recording() {
    while is_recording_now() {
        tokio::time::sleep(WAIT_POLL).await;
    }
}

/// Everything after the claim: the word-for-word pass, the fluency numbers, then Claude's items.
async fn analyse<R: Runtime>(app: AppHandle<R>, pool: SqlitePool, session_id: String, hold: ModelHold) -> Result<Report, String> {
    let (session, turns) = db::get_session(&pool, &session_id)
        .await?
        .ok_or("Session not found")?;
    let folder = session.folder_path.ok_or("Session folder is missing")?;
    let wav = Path::new(&folder).join(PRACTICE_FILES[0]);
    let heard = word_for_word(&app, &pool, &session_id, &wav, &turns, hold).await;
    // The model is released before Claude is called, so an idle engine can unload.
    dictate::unload_if_idle().await;
    let heard = heard?;

    let learner: BTreeMap<i64, String> = heard
        .iter()
        .filter(|h| h.speaker == "user")
        .map(|h| (h.idx, h.text.clone()))
        .collect();
    let learner_texts: Vec<&str> = learner.values().map(String::as_str).collect();
    let timing: Vec<(f64, u32)> = heard.iter().filter_map(|h| h.timing).collect();
    let fluency = fluency(&learner_texts, &timing);
    // Nothing to grade: an empty report, with no key read and no Claude call (its prompt would be empty).
    if !has_learner_text(&learner_texts) {
        return Ok(build_report(fluency, vec![], &learner));
    }

    let key = claude::api_key(&pool).await.map_err(|e| report_copy(&e).to_string())?;
    let rows: Vec<(i64, &str, &str)> = heard
        .iter()
        .map(|h| (h.idx, h.speaker.as_str(), h.text.as_str()))
        .collect();
    let (system, user) = report_prompts(&rows);
    let body = claude::report_body(&system, &user, &schema());
    let text = claude::stream(&claude::http_client(), &key, &body, &CancellationToken::new(), |_| {})
        .await
        .map_err(|e| report_copy(&e).to_string())?;
    parse_report(&text, fluency, &learner)
}

/// Re-transcribes each learner turn that has audio, word for word, and stores it. Other turns keep their live text.
/// Whisper runs once per VAD segment, and before each segment a meeting recording that has started takes the
/// engine first, so the job waits behind it.
async fn word_for_word<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    session_id: &str,
    wav: &Path,
    turns: &[TurnRow],
    _hold: ModelHold,
) -> Result<Vec<Heard>, String> {
    // Whisper is loaded only when a learner turn has audio to re-transcribe.
    let engine = if has_learner_audio(turns) {
        if !dictate::engine_is_whisper(app).await {
            return Err(NOT_WHISPER.to_string());
        }
        let _lifecycle = acquire_engine_lifecycle_lock().await;
        Some(get_or_init_whisper(app).await?)
    } else {
        None
    };
    let mut heard = Vec::with_capacity(turns.len());
    for turn in turns {
        let mut text = turn.live_text.clone();
        let mut timing = None;
        if let (true, Some(offset), Some(len), Some(engine)) = (turn.speaker == "user", turn.wav_offset, turn.wav_len, engine.as_ref()) {
            let (segments, t) = read_speech(wav.to_path_buf(), offset, len).await?;
            let mut spoken = Vec::with_capacity(segments.len());
            for samples in segments {
                if is_recording_now() {
                    store(app, pool, session_id, "waiting", None, None).await;
                    wait_while_recording().await;
                    store(app, pool, session_id, "running", None, None).await;
                }
                // Whisper inference is synchronous, so it runs on a blocking thread, not a Tokio worker.
                let worker = engine.clone();
                let text = tokio::task::spawn_blocking(move || {
                    tauri::async_runtime::block_on(worker.transcribe_audio_raw(
                        samples,
                        Some("en".to_string()),
                        Some(VERBATIM_PROMPT),
                    ))
                })
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
                spoken.push(text);
            }
            text = choose_verbatim(&spoken, &text);
            // A turn with no words at all keeps no verbatim text; the live text stays the only copy.
            if !text.is_empty() {
                db::set_verbatim(pool, session_id, turn.idx, &text).await?;
            }
            timing = Some(t);
        }
        heard.push(Heard {
            idx: turn.idx,
            speaker: turn.speaker.clone(),
            text,
            timing,
        });
    }
    Ok(heard)
}

/// Whether any learner turn has audio to re-transcribe.
pub fn has_learner_audio(turns: &[TurnRow]) -> bool {
    turns.iter().any(|t| t.speaker == "user" && t.wav_offset.is_some() && t.wav_len.is_some())
}

/// Whether the learner said any words, live or verbatim.
pub fn has_learner_text(texts: &[&str]) -> bool {
    texts.iter().any(|t| !t.trim().is_empty())
}

/// The stored verbatim text: the segments' words joined with spaces, or the live text when they are blank or looped.
pub fn choose_verbatim(spoken: &[String], live: &str) -> String {
    let joined = spoken.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    if joined.is_empty() || looks_looped(&joined) {
        live.to_string()
    } else {
        joined
    }
}

/// Reads one turn from you.wav and splits it into its VAD speech segments, on a blocking thread.
/// Returns the segments' samples and the turn's (speech seconds, long pauses).
async fn read_speech(wav: PathBuf, offset: i64, len: i64) -> Result<(Vec<Vec<f32>>, (f64, u32)), String> {
    tokio::task::spawn_blocking(move || -> Result<(Vec<Vec<f32>>, (f64, u32)), String> {
        let samples = read_span(&wav, offset, len)?;
        let segments = get_speech_chunks(&samples, VERBATIM_REDEMPTION_MS).map_err(|e| e.to_string())?;
        let speech_secs = segments.iter().map(|s| s.samples.len()).sum::<usize>() as f64 / f64::from(YOU_WAV_RATE);
        let spans: Vec<(f64, f64)> = segments
            .iter()
            .map(|s| (s.start_timestamp_ms / 1000.0, s.end_timestamp_ms / 1000.0))
            .collect();
        let timing = (speech_secs, long_pauses(&spans, LONG_PAUSE_SECS));
        Ok((segments.into_iter().map(|s| s.samples).collect(), timing))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Copy for a Claude failure in the report (R30). Busy, network and stream errors all read as "couldn't reach".
pub fn report_copy(e: &ClaudeError) -> &'static str {
    match e {
        ClaudeError::Busy => "Claude was busy — try again",
        ClaudeError::NoKey | ClaudeError::Rejected | ClaudeError::Settings => e.user_message(),
        _ => "Couldn't reach Claude — try again",
    }
}

/// Fluency over the learner's texts. `timing` holds (speech seconds, long pauses) for each turn with audio.
pub fn fluency(texts: &[&str], timing: &[(f64, u32)]) -> Fluency {
    let mut fillers: BTreeMap<String, u32> = BTreeMap::new();
    for text in texts {
        for (filler, n) in count_fillers(text) {
            *fillers.entry(filler).or_insert(0) += n;
        }
    }
    let words: u32 = texts.iter().map(|t| word_count(t)).sum();
    let speech_secs: f64 = timing.iter().map(|t| t.0).sum();
    Fluency {
        filler_total: fillers.values().sum(),
        fillers,
        words,
        speech_secs,
        wpm: wpm(words, speech_secs),
        long_pauses: timing.iter().map(|t| t.1).sum(),
    }
}

/// Parses Claude's JSON and builds the report, keeping only items grounded in the learner's turns.
pub fn parse_report(text: &str, fluency: Fluency, learner: &BTreeMap<i64, String>) -> Result<Report, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| format!("Claude's report was not valid JSON: {e}"))?;
    let items = items_from_json(&value)?;
    Ok(build_report(fluency, items, learner))
}

/// Drops the items whose quote is not in the learner's turn, and counts them.
pub fn build_report(fluency: Fluency, items: Vec<ReportItem>, learner_turns: &BTreeMap<i64, String>) -> Report {
    let (items, dropped_items) = validate_items(items, learner_turns);
    Report {
        version: 1,
        fluency,
        items,
        dropped_items,
    }
}

/// The list line for a report, e.g. "4 grammar · 31 fillers".
pub fn summary_line(report: &Report) -> String {
    let grammar = report.items.iter().filter(|i| i.kind == ItemKind::Grammar).count();
    format!("{grammar} grammar · {} fillers", report.fluency.filler_total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(kind: ItemKind, turn: i64, quote: &str) -> ReportItem {
        ReportItem {
            kind,
            turn,
            quote: quote.to_string(),
            better: "better".to_string(),
            note: String::new(),
            echoed: false,
        }
    }

    #[test]
    fn build_report_drops_invented_quotes_and_counts_them() {
        let mut turns = BTreeMap::new();
        turns.insert(1, "I has a dog.".to_string());
        let items = vec![
            item(ItemKind::Grammar, 1, "I has"),
            item(ItemKind::Grammar, 1, "a cat"),
            item(ItemKind::Clarity, 2, "I has"),
        ];
        let report = build_report(Fluency::default(), items, &turns);
        assert_eq!(report.version, 1);
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].quote, "I has");
        assert_eq!(report.dropped_items, 2);
    }

    #[test]
    fn summary_line_formats_counts() {
        let fluency = Fluency {
            filler_total: 31,
            ..Fluency::default()
        };
        let grammar: Vec<ReportItem> = (0..4).map(|t| item(ItemKind::Grammar, t, "x")).collect();
        let report = Report {
            version: 1,
            fluency: fluency.clone(),
            items: grammar,
            dropped_items: 0,
        };
        assert_eq!(summary_line(&report), "4 grammar · 31 fillers");

        let quiet = Report {
            version: 1,
            fluency: Fluency {
                filler_total: 3,
                ..fluency
            },
            items: vec![item(ItemKind::Unnatural, 1, "x")],
            dropped_items: 0,
        };
        assert_eq!(summary_line(&quiet), "0 grammar · 3 fillers");
    }

    #[test]
    fn malformed_model_json_fails_the_report() {
        assert!(items_from_json(&json!({"grammar": "x"})).is_err());
        let result = parse_report(r#"{"grammar":"x"}"#, Fluency::default(), &BTreeMap::new());
        let (status, json, error) = finish(result);
        assert_eq!(status, "failed");
        assert!(json.is_none());
        assert!(error.is_some());
    }

    #[test]
    fn finish_stores_a_ready_report() {
        let report = build_report(Fluency::default(), Vec::new(), &BTreeMap::new());
        let (status, json, error) = finish(Ok(report));
        assert_eq!(status, "ready");
        assert!(error.is_none());
        let stored: Report = serde_json::from_str(&json.unwrap()).unwrap();
        assert_eq!(stored.version, 1);
    }

    #[test]
    fn fluency_sums_fillers_words_and_pauses() {
        let texts = ["Um, I think, uh, we ship it", "so, like, yes"];
        let f = fluency(&texts, &[(10.0, 1), (5.0, 2)]);
        assert_eq!(f.fillers.get("um"), Some(&1));
        assert_eq!(f.fillers.get("uh"), Some(&1));
        assert_eq!(f.fillers.get("like"), Some(&1));
        assert_eq!(f.filler_total, 3);
        assert_eq!(f.words, 10);
        assert_eq!(f.speech_secs, 15.0);
        assert_eq!(f.long_pauses, 3);
        assert_eq!(f.wpm, 40.0);
    }

    #[test]
    fn learner_audio_and_text_decide_whisper_and_claude() {
        let turn = |speaker: &str, wav: Option<(i64, i64)>| TurnRow {
            session_id: "s".into(),
            idx: 0,
            speaker: speaker.into(),
            live_text: String::new(),
            verbatim_text: None,
            at_secs: 0.0,
            wav_offset: wav.map(|w| w.0),
            wav_len: wav.map(|w| w.1),
        };
        assert!(!has_learner_audio(&[turn("character", Some((0, 16))), turn("user", None)]));
        assert!(has_learner_audio(&[turn("character", None), turn("user", Some((0, 16)))]));
        assert!(!has_learner_text(&[]));
        assert!(!has_learner_text(&["", "  \n "]));
        assert!(has_learner_text(&["", "Hi"]));
    }

    #[test]
    fn verbatim_falls_back_to_live_when_blank_or_looped() {
        assert_eq!(choose_verbatim(&["  I think so.  ".into()], "live"), "I think so.");
        assert_eq!(choose_verbatim(&[], "live words"), "live words");
        assert_eq!(choose_verbatim(&["".into(), " \n ".into()], "live words"), "live words");
        assert_eq!(choose_verbatim(&["the the the the the the".into()], "live words"), "live words");
        assert_eq!(choose_verbatim(&[], ""), "");
    }

    #[test]
    fn verbatim_joins_segments_with_spaces_and_checks_the_joined_text_for_loops() {
        assert_eq!(choose_verbatim(&["I think".into(), "  so.".into(), "".into()], "live"), "I think so.");
        // Each segment alone is fine; the repeats only show once they are joined.
        assert_eq!(choose_verbatim(&["the the the".into(), "the the the".into()], "live"), "live");
    }

    #[test]
    fn claude_failures_map_to_report_copy() {
        assert_eq!(report_copy(&ClaudeError::Busy), "Claude was busy — try again");
        assert_eq!(report_copy(&ClaudeError::Http(500)), "Couldn't reach Claude — try again");
        assert_eq!(report_copy(&ClaudeError::Stream("max_tokens".into())), "Couldn't reach Claude — try again");
        assert_eq!(report_copy(&ClaudeError::Cancelled), "Couldn't reach Claude — try again");
        assert_eq!(report_copy(&ClaudeError::Rejected), ClaudeError::Rejected.user_message());
        assert_eq!(report_copy(&ClaudeError::NoKey), ClaudeError::NoKey.user_message());
    }
}
