# Conclave: Speaker identification — Design

Date: 2026-10-15
Status: Design approved section by section in conversation (2026-10-15); written spec awaiting the user's review
Target release: v0.4.0 (together with Phase 3, which is merged but not tagged)

## 1. Purpose

Transcripts and summaries do not say who said what. Notion AI Meeting Notes does, and the user's summaries (action owners, "Сергей will…") depend on it. This feature labels every transcript line with a speaker, remembers people by voice across meetings, and passes names into the summary.

Next feature after this one: an AI chat over all meetings.

## 2. Success criteria

1. With Settings → Speakers → "Identify speakers" on, live transcript lines are labelled **You** or **Others** during a call.
2. After stop, within about a minute per hour of meeting on the user's PC (DirectML), lines are relabelled: **You**, already-known people by name, new people as **Speaker 1, 2, …**. The automatic summary waits for this and uses the names.
3. Naming a speaker once (Speakers bar) makes Conclave recognise that person automatically in later meetings.
4. A line in which two people speak is split at sentence ends, so short replies are attributed to the right person.
5. "Identify speakers" works on any older or imported meeting that has a saved recording.
6. Settings → Speakers shows which backend the last analysis used (DirectML or CPU) and how long it took.
7. With the feature off, nothing changes compared with today: no level track, no labels, no waiting, byte-identical summary prompt.
8. The installer stays under 100 MB (v0.3.0: 66.7 MB). Speaker models are downloaded on first use, not bundled.
9. Everything works on Windows 10 x64.

## 3. Decisions (with reasons)

| Decision | Reason |
|---|---|
| Real names remembered across meetings; learned when the user names a speaker after a meeting; no enrollment screen | User's choice |
| The microphone is always only the user | User's setup |
| Source signal: a per-50 ms mic/system level track recorded at mix time (not a separate system-only audio file) | Smallest change. Upgrade path if quality disappoints: also keep a temporary 16 kHz system-only WAV (~115 MB/h, deleted after analysis) so diarization never hears the user (§13) |
| Engine: pyannote segmentation-3.0 + NeMo TitaNet-large + sherpa-onnx clustering + centroid merge | Won the 2026-10-14/15 spike on 4 real recordings (correct speaker counts on all, 11/12 clusters pure in a blind listening test). VAD-window clustering lumped two people together; WeSpeaker/CAM++ were too weak on call audio |
| Port the pipeline to Rust on the existing `ort` crate, not `sherpa-rs` | One ONNX Runtime in the process (no second `onnxruntime.dll`), DirectML through `ort` |
| DirectML from the start, CPU fallback, DirectML runtime bundled | User's choice. CUDA EP would need cuDNN (~1 GB). DirectML works on Windows 10 with any DX12 GPU |
| Automatic summary waits for the analysis; "Summarize now" escape | Summaries need names; the user must never be stuck |
| Live labels: only You / Others | Matching known voices on short live chunks is untested (later list) |
| Lines containing two speakers are split at sentence ends | Accurate attribution of short replies |
| Known voices auto-match at cosine ≥ 0.75, no confirmation step | Spike: same person 0.87–0.96 across meetings, different people 0.2–0.6 |
| Feature off by default | An update changes nothing until the user opts in |

## 4. Current state (as of main 56fd56f)

- `audio/pipeline.rs:845-905`: mic and system are mixed in 600 ms windows (`window_ms`, `:41`; the 50 ms comment at `:40` is stale) at 48 kHz. The **mixed** audio goes both to VAD → Whisper (`:860-874`, chunk tagged `DeviceType::Microphone`) and to the recording (`:900`). Per-source identity is lost here. `extract_window` zero-pads the shorter stream; overflow drops oldest samples (`:92-97`).
- Transcript times: VAD segment timestamps are in the mixed-sample timeline (`vad.rs:222-223`), `audio_start_time = chunk_timestamp` (`transcription/worker.rs:194`). The recording concatenates the same mixed samples (`incremental_saver.rs:54`). So transcript times, `audio.mp4` and anything indexed by emitted mixed samples share one clock.
- Whisper (`whisper_engine.rs:559`, `:676`) runs with `set_no_timestamps(true)` and `set_token_timestamps(true)` (`:589-590`, `:706-707`), joins segment texts into one string and runs `clean_repetitive_text`. Segment `t0/t1` are read and discarded (`:801`). With timestamp tokens off, segments are not sentences; token times are available.
- `transcripts.speaker TEXT` exists (`migrations/20251110000001_add_speaker_field.sql`) but nothing writes or reads it.
- Saving: `useRecordingStop.ts:256` → `api_save_transcript` (`api/api.rs:930`) → `TranscriptsRepository::save_transcript` (`database/repositories/transcript.rs:13`, insert `:50`). Import (`import.rs:723`) and re-transcribe (`retranscription.rs:437-461`, delete + insert in one transaction) are the other writers. Loaders: `meeting.rs:78` (no ORDER BY) and paginated `meeting.rs:154` (by `audio_start_time`).
- Auto-summary: `page-content.tsx:162-180` starts when transcripts exist; `useSummaryGeneration.ts:348-361` formats `[MM:SS] text` lines from DB rows. Summary attempts are guarded by start time (`database/repositories/summary.rs:145`).
- Recording is saved as `<meeting folder>/audio.mp4` when `auto_save` is on (default; `recording_preferences.rs:39`); `meetings.folder_path` stores the folder.
- `decoder.rs:383` `decode_audio_file` returns the whole file at its original rate, interleaved.
- ONNX Runtime 1.22.0 CPU build is downloaded at build time (`build/onnxruntime.rs:14`) and bundled (`onnxruntime.dll`, `onnxruntime_providers_shared.dll`); the app loads it by absolute path (`lib.rs:475-484`, `ort` with `load-dynamic`). Parakeet sessions are CPU-only with parallel execution (`parakeet_engine/model.rs:96-131`).
- Model downloaders: Whisper (`whisper_engine.rs:1100+`) and Parakeet (`parakeet_engine.rs:618+`, resume + size checks, no checksums) are separate and engine-bound.
- Existing deps usable here: `ort`, `realfft`, `ndarray`, `rubato`.

## 5. Architecture

New Rust module `frontend/src-tauri/src/speakers/`:

| File | Job |
|---|---|
| `levels.rs` | Write and read the per-50 ms mic/system level track |
| `models.rs` | Download and verify the two models |
| `runtime.rs` | ORT sessions for the speaker models: DirectML or CPU, settings, fallback |
| `features.rs` | TitaNet input features (port of sherpa-onnx's NeMo feature extraction) |
| `diarize.rs` | Segmentation, embeddings, clustering, merge (port of sherpa-onnx 1.13.8) |
| `sentences.rs` | Build timed sentences from Whisper tokens |
| `assign.rs` | Self detection, known-voice matching, numbering, line labels and splits |
| `voices.rs` | People, voiceprints, learning, rename/delete/merge |
| `job.rs` | Job queue, states, run tokens, recovery, publication |
| `commands.rs` | Tauri commands and events |

Pure logic (`features`, `diarize` clustering/merge, `sentences`, `assign`, `voices` rules) takes plain data and has no I/O, so it can be unit-tested on Linux.

### 5.1 Flow

1. **During the call** (feature on): each 600 ms mix window also yields 12 mic/system level pairs, appended to `levels.bin`. Each transcript line gets `speaker_idx = 0` (You) or NULL (Others) from the levels over its time range (§6). Whisper's timed sentences are kept with the line.
2. **On save**: transcripts (with `speaker_idx` and `sentences`) and a `speaker_jobs` row with status `pending` are written in the same transaction.
3. **Background job** (§8) runs, then publishes speakers, labels, splits and the terminal status in one transaction and emits `speakers-updated`.
4. **Meeting page** shows progress. The auto-summary starts once the job status is terminal (`done`, `failed`, `skipped`) or the user clicks "Summarize now". The prompt is built from DB rows with resolved names.
5. **"Identify speakers"** and **re-transcribe** enqueue the same job. Old meetings without `levels.bin` find "You" through the self voiceprints.
6. **Feature off**: no levels, no labels, no `speaker_jobs` row, no waiting, today's prompt format.

## 6. Level track and live labels

- `levels.bin` in the meeting folder: a small header (format version, slice length 50 ms, sample rate) followed by `(mic_rms, sys_rms)` pairs as `f32`, one pair per 50 ms of **emitted mixed samples**. Each extracted 600 ms window produces exactly 12 pairs, computed from that window's `mic_window` and `sys_window` before mixing. No wall clock is used. About 150 KB per hour.
- Mic audio is loudness-normalised at capture and system audio is raw, so levels are not compared across channels. Each channel keeps its own running noise floor. A slice is **mic-only** when the mic is above its floor by a margin and the system is within its floor margin (quiet). A slice with both active counts as Others (this also covers the user's mic picking up speakers).
- Live label for a line: **You** if most of its slices are mic-only, else **Others**.
- Only written when the feature is on. Deleted with the meeting folder.

## 7. Timed sentences

- The Whisper engine returns, in addition to its unchanged string, a list of `(t0, t1, text)` sentences built from token timestamps: tokens are grouped and cut after `.`, `?`, `!` (and `…`), times converted from chunk-local 10 ms units to meeting seconds by adding the chunk's start time.
- If `clean_repetitive_text` changed the text, no sentences are returned for that line (the pieces would no longer match it).
- Written by all three transcript writers (recording, import, re-transcribe) into `transcripts.sentences` (JSON). Parakeet returns none.
- Token times without timestamp tokens are approximate. Before relying on them, the plan measures split accuracy on 10-minute excerpts of the spike recordings (§12). Lines whose sentence timing is invalid (overlaps, out of the line's range, unordered) are treated as having no sentences.

## 8. Analysis job

### 8.1 Queue and states

- One analysis job runs at a time across the whole app (a global queue). One job per meeting at a time; re-transcribe and identify for the same meeting are serialised with it.
- `speaker_jobs.status`: `pending → running → done | failed | skipped`, with `reason`, `backend` and a `run_token`. Claiming and publishing are conditional updates on the token. At startup, `pending`/`running` jobs are re-queued with a new token.
- Speaker editing for a meeting is disabled while its job is pending or running.

### 8.2 Steps

0. **Preconditions** (else `skipped` with a reason): feature on, both models present with correct SHA-256, `audio.mp4` exists.
1. **Decode**: stream the recording packet by packet, downmix to mono and resample to 16 kHz (`rubato`) into a temporary `speakers.tmp.f32` file in the meeting folder. Memory stays bounded regardless of meeting length. The file is deleted when the job ends, whatever the outcome. (A streaming variant of the existing decoder is added; `decode_audio_file` is unchanged.)
2. **Segmentation**: port of sherpa-onnx 1.13.8 `OfflineSpeakerDiarizationPyannoteImpl`: 10 s windows, 1 s step, powerset decoding to local speakers, overlapped frames excluded from embedding audio.
3. **Embeddings**: per window and local speaker, the disjoint speech is concatenated and embedded with TitaNet-large, exactly as sherpa does. Features: port of sherpa's NeMo extractor (framing/window from model metadata, librosa mel filters, pre-emphasis, `snip_edges=true`, no DC removal, per-feature normalisation with `stddev + 1e-5`, input `[1, bins, frames]` plus length). Invalid (non-finite or zero-norm) embeddings are dropped.
4. **Clustering**: sherpa's FastClustering: complete linkage on `1 − cosine` of L2-normalised embeddings, cut at distance 0.7. The distance matrix is stored as `f32`. Above a size cap (to be set from measurement, aiming at ≤ ~0.5 GB) the window step becomes 2 s; the parity tests confirm this keeps the same speakers.
5. **Reconstruction**: global speaker activity from overlapping-window votes and estimated speaker counts, as sherpa does; then `min_duration_on 0.3 s`, `min_duration_off 0.5 s`; activity clamped to the decoded duration.
6. **Merge**: clusters with ≥ 30 s of speech are anchors. Centroids from 3 s sub-windows of segments ≥ 1.5 s. Greedy merge, biggest first, at cosine ≥ 0.7 (spike: stable for 0.6–0.8).
7. **Small clusters** (≥ 3 s, < 30 s): first go through self and known-voice matching (steps 8–9). Unmatched ones attach to the nearest anchor at cosine ≥ 0.65. If the meeting has no anchor at all (short meetings), they stay as separate speakers; otherwise they become Unknown. Clusters under 3 s become Unknown.
8. **Self**:
   - With `levels.bin`: the cluster with the most mic-only time, if at least half of its own time is mic-only, becomes idx 0.
   - Without it: the cluster best matching the self voiceprints at ≥ 0.75.
   - If none qualifies (for example the user never spoke), idx 0 has no lines.
   - Self learning uses only 3 s sub-windows that are ≥ 90 % mic-only, and needs ≥ 20 s of them.
9. **Known people**: each remaining cluster takes the person with the highest max-cosine over that person's voiceprints, if ≥ 0.75. Clusters matching the same person are merged.
10. **Numbering**: remaining speakers become 1..n by talk time, descending.
11. **Lines**: per sentence, the speaker with the most time overlap. A line is split only where a sentence has ≥ 60 % and ≥ 1 s overlap with a different speaker than its neighbours; adjacent sentences with the same speaker stay together. Sentences with no overlap inherit the line's majority. Lines without sentences get the majority speaker. Split lines get fresh ids, their own times, text and sentence subset.
12. **Publish** (one transaction, token-checked): replace the meeting's `meeting_speakers`, update/split transcript rows, write learned self voiceprint, delete voiceprints previously learned from this meeting if this is a re-run, set `done`. Emit `speakers-updated`. Log backend, audio length and run time; store them for Settings.

Degenerate input is a successful result: silence or no valid embeddings → `done` with no speakers; a single window or a single embedding skips clustering.

### 8.3 Runtime (DirectML / CPU)

- The bundled ONNX Runtime becomes the DirectML build of 1.22.0 (the same version, which includes the CPU provider), plus `DirectML.dll` next to the exe. The build script pins both archives by SHA-256, like today. It replaces the existing CPU runtime for the whole app, including Parakeet, so Parakeet and startup are re-verified (§12).
- Windows 10's System32 has an older `DirectML.dll` and an older `onnxruntime.dll`. The app keeps loading ORT by absolute path (`lib.rs:483`), and the bundled `DirectML.dll` in the app folder is loaded first.
- Speaker sessions have their own builder. DirectML: sequential execution, memory pattern off, one run at a time per session. CPU: intra-op threads = half the logical cores, spinning off.
- ORT silently uses CPU when a requested provider is unavailable, so the job checks registration explicitly and records the real backend.
- If DirectML fails at session creation or during inference, the sessions are rebuilt on CPU and the job continues from the last completed stage.
- Each model may end up on a different backend: if profiling on the user's PC shows TitaNet's variable-length inputs are slower on DirectML than on CPU, TitaNet runs on CPU.
- Sessions are dropped when the job ends.

### 8.4 Resources (estimates, to be confirmed by the job's log)

DirectML: under a minute per hour of audio; CPU: 2–3 minutes. A few hundred MB of VRAM next to the loaded Whisper model. RAM bounded by streaming; clustering matrix capped (§8.2 step 4). Disk: 105 MB of models once; `levels.bin` ~150 KB/h; temp file ~230 MB/h during the job only.

## 9. Data model

One new migration. Existing tables only gain nullable columns. sqlx enables SQLite foreign keys by default.

```sql
CREATE TABLE people (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  is_self INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX people_one_self ON people(is_self) WHERE is_self = 1;
INSERT INTO people (id, name, is_self, created_at) VALUES ('self', 'You', 1, datetime('now'));

CREATE TABLE meeting_speakers (
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  idx INTEGER NOT NULL,              -- 0 = self, 1..n = "Speaker n"
  person_id TEXT REFERENCES people(id) ON DELETE SET NULL,
  name TEXT,                         -- snapshot shown when person_id is NULL
  centroid BLOB,                     -- 192 x f32
  seconds REAL NOT NULL,
  PRIMARY KEY (meeting_id, idx)
);

CREATE TABLE voiceprints (
  id TEXT PRIMARY KEY,
  person_id TEXT NOT NULL REFERENCES people(id) ON DELETE CASCADE,
  source_meeting_id TEXT REFERENCES meetings(id) ON DELETE SET NULL,
  source_idx INTEGER,
  vector BLOB NOT NULL,              -- 192 x f32, L2-normalised
  seconds REAL NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (source_meeting_id, source_idx)
);

CREATE TABLE speaker_jobs (
  meeting_id TEXT PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
  status TEXT NOT NULL,              -- pending | running | done | failed | skipped
  reason TEXT,
  backend TEXT,                      -- directml | cpu | mixed
  run_token TEXT NOT NULL,
  audio_seconds REAL,
  run_seconds REAL,
  updated_at TEXT NOT NULL
);

ALTER TABLE transcripts ADD COLUMN speaker_idx INTEGER;   -- NULL = Others/Unknown
ALTER TABLE transcripts ADD COLUMN sentences TEXT;        -- JSON [[t0, t1, text], ...]
```

The legacy `transcripts.speaker` column stays unused. Meeting deletion (`meeting.rs:248-268`) also deletes `meeting_speakers` and `speaker_jobs` explicitly, matching how it handles other children.

Rules:
- **Display name** of a line: `people.name` via `meeting_speakers.person_id`, else `meeting_speakers.name`, else "Speaker n"; NULL `speaker_idx` shows "Others" (live) or "Unknown" (after analysis).
- **Learning**: naming meeting speaker n as person P upserts the voiceprint for source `(meeting, n)` if that speaker has ≥ 20 s; renaming that speaker to someone else replaces it. Auto-matched speakers do not learn. Self learns automatically (§8.2 step 8). After an insert or a merge, each person keeps their 20 newest voiceprints.
- **Two speakers named the same person** in one meeting are shown as one speaker.
- **Known voices**: rename = update `people.name` (shown everywhere); delete = copy the name into `meeting_speakers.name` for that person's rows, then delete the person (voiceprints cascade); merge A into B = move A's voiceprints and meeting speakers to B, prune, delete A. Self can be renamed, not deleted or merged.
- **Re-running Identify** replaces the meeting's speakers and all its labels, including manual line moves (the UI confirms first), and removes voiceprints learned from this meeting; known-voice matching restores names.
- **Transcript order**: both loaders order by `audio_start_time`, then `id`.
- **Privacy**: voiceprints and centroids never leave the PC; only display names go into the cloud summary prompt.
- **Feature off**: data kept, labels hidden.
- **Known gap**: the meeting folder's `transcripts.json` is not updated by splits or labels; the DB is the source of truth.
- **Known limit**: moving single lines does not recompute the speaker's centroid used for learning (a voiceprint averages minutes of speech; a few moved lines barely affect it).

## 10. UI

### 10.1 Settings → Speakers (new tab)

- "Identify speakers" toggle, **off by default**. Turning it on downloads the models with a progress bar; the feature becomes active after both pass SHA-256. Failure: message + Retry; partial files deleted. "Delete models" button.
- "Last analysis: DirectML · 41 s for a 58-min meeting" (backend, run time, audio length from the latest `speaker_jobs` row).
- **Known voices**: name, number of meetings, last heard. Actions: Rename, Merge into…, Delete (confirm: "Voiceprints are removed; past transcripts keep the name"). The self entry is pinned on top, marked "this is you", rename only.

### 10.2 During a call

Each live line starts with a small **You** / **Others** label (feature on only).

### 10.3 Meeting page

- While the job is pending/running: "Identifying speakers… 40%" over the transcript; the summary panel shows "Waiting for speaker analysis…" with a **Summarize now** link.
- **Speakers bar** above the transcript: one chip per speaker with a colour dot and talk time (`● You 12m  ● Сергей 18m  ● Speaker 2 1m ✎`). Clicking opens a name box with suggestions from Known voices and "New person…".
- **Line label** click: move that line to another speaker of the meeting or to Unknown.
- Editing is disabled while a job runs.
- After any name change: a hint in the summary panel, "Speaker names changed. Regenerate the summary to use them", using the existing Regenerate button and its unsaved-edits confirmation.
- **Copy transcript** includes names (`[12:04] Сергей: …`) when the feature is on.
- **Identify speakers** button next to Copy and Recording, shown when the feature is on, models are present and the meeting has a recording. On an analysed meeting it reads **Re-identify** and confirms that labels, including manual fixes, will be replaced.
- Failure: "Speaker analysis failed: <reason>. [Retry]". Skipped: a quiet note with the reason (e.g. "No recording saved, so speakers weren't identified").
- Re-transcribe dialog (still behind its beta flag): "Speaker labels will be rebuilt afterwards."

### 10.4 Summary prompt

Feature on: `[MM:SS] Name: text` per line, names resolved at generation time from the DB. Feature off: exactly today's `[MM:SS] text`.

## 11. Network and packaging

- New outbound calls: the two model files from Hugging Face (an allowed model host): pyannote segmentation-3.0 (sherpa-onnx export, ~6 MB) and NeMo TitaNet-large ONNX (~97 MB). Exact URLs and SHA-256 are pinned in `speakers/models.rs` and listed in `docs/network-calls.md`; `scripts/check-no-phone-home.sh` must pass.
- Models live in `<app_data>/models/speakers/`. Downloads use a small shared downloader extracted from Parakeet's (resume, size check) with SHA-256 verification added.
- Installer: DirectML ORT build replaces the CPU build, plus `DirectML.dll` (estimate +10–15 MB compressed; ~80 MB total). `release.yml` fails if the installer exceeds 100 MB.

## 12. Tests

**Linux, free:**
- Unit: level slicing and mic-only rule; sentence building from tokens (incl. cleaned-text and invalid-timing fallbacks); line assignment and splitting; matching, learning, voiceprint replacement and the 20-print cap; rename/delete/merge; clustering, merge and small-cluster rules on synthetic vectors; degenerate inputs; job states, run tokens and startup recovery; migration applied to a copy of a pre-feature DB (existing rows unchanged); meeting deletion removes speaker rows.
- Parity (local, `#[ignore]`, not in CI): Rust vs the Python spike with sherpa-onnx 1.13.8 on the 4 spike recordings: feature tensors and embeddings numerically close (cosine > 0.999), final speakers identical; also with the 2 s step. Fixtures are derived from real people's voices and stay out of git (`.superpowers/`, gitignored).
- Sentence-split accuracy on 10-minute excerpts of the recordings, measured before splitting is enabled.
- Frontend (Bun): feature off → summary payload byte-identical to today; names in prompt and Copy; summary waiting and "Summarize now"; Speakers bar naming and same-name merge.
- `bash scripts/check-no-phone-home.sh`, `actionlint`.

**Windows CI (manual dispatch, checkpoints only):** build with the DirectML runtime; app startup and Parakeet with the new DLL; speaker job end to end on CPU (runners have no GPU); installer size check.

**User's PC (after install):** enable the feature and download models; record a 5-minute test call; Settings shows DirectML; name the other speaker; run Identify on an older meeting and check that the user and that person are recognised.

## 13. Out of scope / later

- Live matching of remote speech to known voices during the call (later list).
- System-only temporary audio track for cleaner diarization: the upgrade path if label quality disappoints (remind the user).
- Recomputing a speaker's centroid after manual line moves.
- Updating `transcripts.json` with speakers.
- Sentence splitting for Parakeet transcripts (they get one speaker per line).
