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
use tokio::task::JoinError;
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
use super::models::ensure_pron_models;
use super::pron::{PronEngine, SegmentHint};
use super::prompts::report_prompts;
use super::report::{
    count_fillers, items_from_json, long_pauses, looks_looped, schema, select_hints, validate_items, word_count, wpm,
    Fluency, ItemKind, PronHint, Report, ReportItem,
};
use super::settings;
use super::{ModelHold, LONG_PAUSE_SECS, RECENT_PATTERNS_MAX, YOU_WAV_RATE};

/// Asks Whisper to write fillers and false starts down, so the transcript keeps them.
pub const VERBATIM_PROMPT: &str = "Um, uh, so, like, I mean, I- I was, uh, thinking that, you know, we, we had it.";
/// Pause length that splits speech into segments in the word-for-word pass.
const VERBATIM_REDEMPTION_MS: u32 = 300;
/// How often a waiting job checks whether a meeting recording has stopped.
const WAIT_POLL: Duration = Duration::from_secs(10);
/// Held while a report is claimed and while a session is deleted, so the two cannot interleave.
pub(super) static REPORT_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const SAVE_FAILED: &str = "Couldn't save the report — Retry";
/// Shown when a report task or a Whisper segment panics or is cancelled. The panic text stays in the log.
const STOPPED: &str = "The report stopped unexpectedly — Retry";
/// The report's note when the pronunciation pass fails. The report itself is kept.
const PRON_UNAVAILABLE: &str = "Pronunciation hints unavailable";

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

/// A learner turn whose verbatim text was kept: its audio span in you.wav and the words of each speech segment.
struct PronInput {
    idx: i64,
    offset: i64,
    len: i64,
    segments: Vec<String>,
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
            Err(STOPPED.to_string())
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

/// While a meeting recording runs, marks the report waiting, blocks until it stops, then marks it running again.
/// Called before each heavy step (the model download and each turn), so a recording that starts takes precedence.
async fn yield_to_recording<R: Runtime>(app: &AppHandle<R>, pool: &SqlitePool, session_id: &str) {
    if is_recording_now() {
        store(app, pool, session_id, "waiting", None, None).await;
        wait_while_recording().await;
        store(app, pool, session_id, "running", None, None).await;
    }
}

/// Everything after the claim: the word-for-word pass, the fluency numbers, then Claude's items.
async fn analyse<R: Runtime>(app: AppHandle<R>, pool: SqlitePool, session_id: String, hold: ModelHold) -> Result<Report, String> {
    let (session, turns) = db::get_session(&pool, &session_id)
        .await?
        .ok_or("Session not found")?;
    let folder = session.folder_path.ok_or("Session folder is missing")?;
    let wav = Path::new(&folder).join(PRACTICE_FILES[0]);
    let pass = word_for_word(&app, &pool, &session_id, &wav, &turns, hold).await;
    // The Whisper model is released before the pronunciation pass and Claude are called, so an idle engine can unload.
    dictate::unload_if_idle().await;
    let (heard, pron_inputs) = pass?;

    let learner: BTreeMap<i64, String> = heard
        .iter()
        .filter(|h| h.speaker == "user")
        .map(|h| (h.idx, h.text.clone()))
        .collect();
    let learner_texts: Vec<&str> = learner.values().map(String::as_str).collect();
    let timing: Vec<(f64, u32)> = heard.iter().filter_map(|h| h.timing).collect();
    let fluency = fluency(&learner_texts, &timing);
    let pron = pronunciation(&app, &pool, &session_id, &wav, &turns, pron_inputs).await;
    if let Err(e) = &pron {
        warn!("practice: pronunciation hints skipped: {e}");
    }
    // Nothing to grade: an empty report, with no key read and no Claude call (its prompt would be empty).
    if !has_learner_text(&learner_texts) {
        return Ok(with_pronunciation(build_report(fluency, vec![], &learner), pron));
    }

    let key = claude::api_key(&pool).await.map_err(|e| report_copy(&e).to_string())?;
    let rows: Vec<(i64, &str, &str)> = heard
        .iter()
        .map(|h| (h.idx, h.speaker.as_str(), h.text.as_str()))
        .collect();
    let recent = db::recent_patterns(&pool, RECENT_PATTERNS_MAX).await.unwrap_or_else(|e| {
        warn!("practice: could not read the recent pattern keys: {e}");
        Vec::new()
    });
    let (system, user) = report_prompts(&rows, &recent);
    let models = settings::current();
    let body = claude::report_body(&models.plan_model, &models.plan_effort, &system, &user, &schema());
    let text = claude::stream(&claude::http_client(), &key, &body, &CancellationToken::new(), |_| {})
        .await
        .map_err(|e| report_copy(&e).to_string())?;
    parse_report(&text, fluency, &learner).map(|report| with_pronunciation(report, pron))
}

/// Re-transcribes each learner turn that has audio, word for word, and stores it. Other turns keep their live text.
/// Whisper runs once per VAD segment, and before each segment a meeting recording that has started takes the
/// engine first, so the job waits behind it. Also returns the turns whose verbatim text was kept, for the
/// pronunciation pass.
async fn word_for_word<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    session_id: &str,
    wav: &Path,
    turns: &[TurnRow],
    _hold: ModelHold,
) -> Result<(Vec<Heard>, Vec<PronInput>), String> {
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
    let mut pron_inputs = Vec::new();
    for turn in turns {
        let mut text = turn.live_text.clone();
        let mut timing = None;
        if let (true, Some(offset), Some(len), Some(engine)) = (turn.speaker == "user", turn.wav_offset, turn.wav_len, engine.as_ref()) {
            let (segments, t) = read_speech(wav.to_path_buf(), offset, len).await?;
            let mut spoken = Vec::with_capacity(segments.len());
            for (_, samples) in segments {
                yield_to_recording(app, pool, session_id).await;
                // Whisper inference is synchronous, so it runs on a blocking thread, not a Tokio worker.
                let worker = engine.clone();
                let heard = tokio::task::spawn_blocking(move || {
                    tauri::async_runtime::block_on(worker.transcribe_audio_raw(
                        samples,
                        Some("en".to_string()),
                        Some(VERBATIM_PROMPT),
                    ))
                })
                .await;
                let text = segment_text(heard)?;
                spoken.push(text);
            }
            // A turn whose words are blank or looped keeps its live text, and gets no pronunciation pass.
            let verbatim = joined_verbatim(&spoken);
            if verbatim.is_some() {
                pron_inputs.push(PronInput {
                    idx: turn.idx,
                    offset,
                    len,
                    segments: spoken,
                });
            }
            text = verbatim.unwrap_or(text);
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
    Ok((heard, pron_inputs))
}

/// The pronunciation pass over the kept learner turns, on a blocking thread. `Ok(vec![])` when no turn has audio.
/// A model download or load failure, or a pass where every turn failed, is an `Err`, which `with_pronunciation`
/// turns into the note. A recording that starts first takes the job's place, as in the Whisper pass.
async fn pronunciation<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    session_id: &str,
    wav: &Path,
    turns: &[TurnRow],
    inputs: Vec<PronInput>,
) -> Result<Vec<PronHint>, String> {
    if !has_learner_audio(turns) {
        return Ok(vec![]);
    }
    yield_to_recording(app, pool, session_id).await;
    let dir = ensure_pron_models(|_, _| {}).await?;
    let (app, pool, session_id, wav) = (app.clone(), pool.clone(), session_id.to_string(), wav.to_path_buf());
    tokio::task::spawn_blocking(move || -> Result<Vec<PronHint>, String> {
        let mut engine = PronEngine::load(&dir)?;
        let mut per_turn = Vec::with_capacity(inputs.len());
        for input in inputs {
            tauri::async_runtime::block_on(yield_to_recording(&app, &pool, &session_id));
            per_turn.push((input.idx, turn_hints(&mut engine, &wav, &input)));
        }
        merge_turns(per_turn)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The hints for one learner turn: each speech segment's words aligned to its audio, shifted into turn time.
fn turn_hints(engine: &mut PronEngine, wav: &Path, input: &PronInput) -> Result<Vec<PronHint>, String> {
    let (segments, _) = split_turn(wav, input.offset, input.len)?;
    if segments.len() != input.segments.len() {
        return Err("the speech segments changed between passes".to_string());
    }
    let turn_secs = input.len as f64 / f64::from(YOU_WAV_RATE);
    let mut hints = Vec::new();
    for ((start, samples), words) in segments.iter().zip(&input.segments) {
        let found = engine.segment_hints(samples, words)?;
        hints.extend(shift_hints(input.idx, *start, found, turn_secs));
    }
    Ok(hints)
}

/// Merges each turn's hints. A failed turn is logged (without its text) and skipped; the others are kept.
/// Err only when every turn failed.
pub fn merge_turns(per_turn: Vec<(i64, Result<Vec<PronHint>, String>)>) -> Result<Vec<PronHint>, String> {
    let total = per_turn.len();
    let mut hints = Vec::new();
    let mut failed = 0;
    for (idx, result) in per_turn {
        match result {
            Ok(found) => hints.extend(found),
            Err(e) => {
                warn!("practice: pronunciation skipped turn {idx}: {e}");
                failed += 1;
            }
        }
    }
    if total > 0 && failed == total {
        return Err("every learner turn failed the pronunciation pass".to_string());
    }
    Ok(hints)
}

/// Moves a segment's hints from segment time to turn time, clamped to the turn.
pub fn shift_hints(turn: i64, segment_start: f64, hints: Vec<SegmentHint>, turn_secs: f64) -> Vec<PronHint> {
    hints
        .into_iter()
        .map(|h| PronHint {
            turn,
            word: h.word,
            start_secs: (segment_start + h.start_secs).clamp(0.0, turn_secs),
            end_secs: (segment_start + h.end_secs).clamp(0.0, turn_secs),
            score: h.score,
            phone: h.phone,
        })
        .collect()
}

/// Adds the pronunciation pass to the report. A failed pass keeps the report and sets the note instead.
pub fn with_pronunciation(report: Report, pron: Result<Vec<PronHint>, String>) -> Report {
    match pron {
        Ok(hints) => Report {
            pronunciation: select_hints(hints),
            ..report
        },
        Err(_) => Report {
            pronunciation_note: Some(PRON_UNAVAILABLE.to_string()),
            ..report
        },
    }
}

/// Whether any learner turn has audio to re-transcribe.
pub fn has_learner_audio(turns: &[TurnRow]) -> bool {
    turns.iter().any(|t| t.speaker == "user" && t.wav_offset.is_some() && t.wav_len.is_some())
}

/// Whether the learner said any words, live or verbatim.
pub fn has_learner_text(texts: &[&str]) -> bool {
    texts.iter().any(|t| !t.trim().is_empty())
}

/// The verbatim text: the segments' words joined with spaces. None when they are blank or looped.
pub fn joined_verbatim(spoken: &[String]) -> Option<String> {
    let joined = spoken.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    (!joined.is_empty() && !looks_looped(&joined)).then_some(joined)
}

/// Splits one turn of you.wav into its VAD speech segments. Each segment comes with its start in the turn, in seconds
/// and its samples. Also returns the turn's (speech seconds, long pauses).
fn split_turn(wav: &Path, offset: i64, len: i64) -> Result<(Vec<(f64, Vec<f32>)>, (f64, u32)), String> {
    let samples = read_span(wav, offset, len)?;
    let segments = get_speech_chunks(&samples, VERBATIM_REDEMPTION_MS).map_err(|e| e.to_string())?;
    let speech_secs = segments.iter().map(|s| s.samples.len()).sum::<usize>() as f64 / f64::from(YOU_WAV_RATE);
    let spans: Vec<(f64, f64)> = segments
        .iter()
        .map(|s| (s.start_timestamp_ms / 1000.0, s.end_timestamp_ms / 1000.0))
        .collect();
    let timing = (speech_secs, long_pauses(&spans, LONG_PAUSE_SECS));
    let starts = segments.into_iter().map(|s| (s.start_timestamp_ms / 1000.0, s.samples)).collect();
    Ok((starts, timing))
}

/// `split_turn` on a blocking thread.
async fn read_speech(wav: PathBuf, offset: i64, len: i64) -> Result<(Vec<(f64, Vec<f32>)>, (f64, u32)), String> {
    tokio::task::spawn_blocking(move || split_turn(&wav, offset, len))
        .await
        .map_err(|e| e.to_string())?
}

/// A Whisper segment's text. A panic or cancel in its blocking task reads as `STOPPED`, not as the panic text.
fn segment_text<E: std::fmt::Display>(joined: Result<Result<String, E>, JoinError>) -> Result<String, String> {
    match joined {
        Ok(text) => text.map_err(|e| e.to_string()),
        Err(e) => {
            warn!("practice: a report segment stopped: {e}");
            Err(STOPPED.to_string())
        }
    }
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
        version: 2,
        fluency,
        items,
        dropped_items,
        pronunciation: vec![],
        pronunciation_note: None,
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
            pattern: None,
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
        assert_eq!(report.version, 2);
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
            version: 2,
            fluency: fluency.clone(),
            items: grammar,
            dropped_items: 0,
            pronunciation: vec![],
            pronunciation_note: None,
        };
        assert_eq!(summary_line(&report), "4 grammar · 31 fillers");

        let quiet = Report {
            version: 2,
            fluency: Fluency {
                filler_total: 3,
                ..fluency
            },
            items: vec![item(ItemKind::Unnatural, 1, "x")],
            dropped_items: 0,
            pronunciation: vec![],
            pronunciation_note: None,
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
        assert_eq!(stored.version, 2);
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
    fn verbatim_is_none_when_blank_or_looped() {
        assert_eq!(joined_verbatim(&["  I think so.  ".into()]).as_deref(), Some("I think so."));
        assert_eq!(joined_verbatim(&[]), None);
        assert_eq!(joined_verbatim(&["".into(), " \n ".into()]), None);
        assert_eq!(joined_verbatim(&["the the the the the the".into()]), None);
    }

    #[test]
    fn verbatim_joins_segments_with_spaces_and_checks_the_joined_text_for_loops() {
        assert_eq!(joined_verbatim(&["I think".into(), "  so.".into(), "".into()]).as_deref(), Some("I think so."));
        // Each segment alone is fine; the repeats only show once they are joined.
        assert_eq!(joined_verbatim(&["the the the".into(), "the the the".into()]), None);
    }

    #[test]
    fn hints_are_shifted_into_the_turn_and_clamped() {
        let hint = SegmentHint {
            word: "think".into(),
            start_secs: 0.1,
            end_secs: 0.5,
            score: -7.0,
            phone: "TH".into(),
        };
        // The segment starts 2 s into a 2.3 s turn: its end is clamped to the turn.
        let got = shift_hints(4, 2.0, vec![hint], 2.3);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].turn, 4);
        assert!((got[0].start_secs - 2.1).abs() < 1e-9, "{}", got[0].start_secs);
        assert!((got[0].end_secs - 2.3).abs() < 1e-9, "{}", got[0].end_secs);
    }

    #[test]
    fn a_failed_turn_is_skipped_and_the_others_are_kept() {
        let hint = |turn: i64| PronHint {
            turn,
            word: "think".into(),
            start_secs: 0.0,
            end_secs: 0.3,
            score: -7.0,
            phone: "TH".into(),
        };
        let kept = merge_turns(vec![(1, Ok(vec![hint(1)])), (2, Err("bad turn".into())), (3, Ok(vec![hint(3)]))]).unwrap();
        assert_eq!(kept.iter().map(|h| h.turn).collect::<Vec<_>>(), [1, 3]);
        assert!(merge_turns(vec![(1, Err("x".into())), (2, Err("y".into()))]).is_err(), "every turn failed");
        assert!(merge_turns(vec![]).unwrap().is_empty());
    }

    #[test]
    fn pron_failure_keeps_the_report() {
        let turns = BTreeMap::from([(1, "I has a dog.".to_string())]);
        let report = build_report(Fluency::default(), vec![item(ItemKind::Grammar, 1, "I has")], &turns);
        let failed = with_pronunciation(report.clone(), Err("models missing".into()));
        assert_eq!(failed.items.len(), 1);
        assert_eq!(failed.pronunciation_note.as_deref(), Some("Pronunciation hints unavailable"));
        assert!(failed.pronunciation.is_empty());

        let hint = PronHint {
            turn: 1,
            word: "has".into(),
            start_secs: 0.0,
            end_secs: 0.4,
            score: -7.5,
            phone: "HH".into(),
        };
        let ok = with_pronunciation(report, Ok(vec![hint]));
        assert_eq!(ok.items.len(), 1);
        assert_eq!(ok.pronunciation.len(), 1);
        assert_eq!(ok.pronunciation_note, None);
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

    #[tokio::test]
    async fn segment_panic_maps_to_friendly_copy() {
        let panicked = tokio::task::spawn_blocking(|| -> Result<String, String> { panic!("whisper internal") }).await;
        assert_eq!(segment_text(panicked).unwrap_err(), "The report stopped unexpectedly — Retry");
        let heard = tokio::task::spawn_blocking(|| -> Result<String, String> { Ok("I think".into()) }).await;
        assert_eq!(segment_text(heard).unwrap(), "I think");
    }
}
