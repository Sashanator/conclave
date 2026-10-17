# Speaker parity reference kit (local only)

`make_reference.py` produces sherpa-onnx 1.13.8 reference outputs for the Rust speaker-ID
parity tests. It runs on a developer PC only and is never part of CI. The recordings contain
real voices, so the outputs stay out of git.

## Setup

```bash
python3 -m venv /workspace/.superpowers/speaker-parity/venv
/workspace/.superpowers/speaker-parity/venv/bin/pip install -r scripts/speaker-parity/requirements.txt
```

`requirements.txt` pins `sherpa-onnx==1.13.8` and `av==18.1.0` (PyAV, used for decoding), plus `numpy`.

Download the two models into `/workspace/.superpowers/speaker-parity/models/`. The script checks
the size and SHA-256 of each and stops on a mismatch.

| File | URL | Bytes | SHA-256 |
|---|---|---|---|
| `segmentation-3.0.onnx` | `https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/9403a6902bb58e3d5ae8c7e77c3422de279db2e0/model.onnx` | 5,992,913 | `220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079` |
| `titanet-large.onnx` | `https://huggingface.co/csukuangfj/speaker-embedding-models/resolve/0743f301363dec56491a490f6d6cbc9d67f9a3bf/nemo_en_titanet_large.onnx` | 101,405,493 | `d51abcf31717ef28162f26acb9d44dd4127c3d44c9b8624f699f3425daca8e77` |

## Run

```bash
nice -n 15 /workspace/.superpowers/speaker-parity/venv/bin/python -I scripts/speaker-parity/make_reference.py \
  --audio-dir /workspace/.superpowers/spike-audio \
  --models /workspace/.superpowers/speaker-parity/models \
  --out /workspace/.superpowers/speaker-parity
```

`-I` is required. The run uses `num_threads=3` and handles one recording at a time. On the 8-core dev VM
the four recordings took about 1.5 hours in total (about 3.3 h of audio, two diarization passes
each: Ira 4 min, Slava 24 min, german-lesson 34 min, meetup 16 min).
A recording whose `merged.json` already exists is skipped, so an interrupted run can resume.
`--merge-only` rebuilds just `merged.json` from the existing `diar_step1.json` and `audio16k.f32`
(TitaNet centroids only, no diarization).

## Decoding

The recordings are decoded with PyAV (libswresample, the same resampler as ffmpeg) to 16 kHz
mono `f32le`. The ffmpeg CLI is not installed. The output matches `ffmpeg -ac 1 -ar 16000 -f f32le`
up to floating-point rounding.

## Outputs

Per recording, in `/workspace/.superpowers/speaker-parity/<name>/` (never committed):

- `audio16k.f32`: decoded audio, 16 kHz mono `f32le`.
- `diar_step1.json`, `diar_step2.json`: `[[start, end, cluster], ...]` from sherpa
  `OfflineSpeakerDiarization` with `window_shift_ratio` 0.1 and 0.2, clustering threshold 0.7,
  `min_duration_on` 0.3, `min_duration_off` 0.5.
- `clips.json`: 20 clips `[[start, end], ...]`, 1.5–10 s. Taken from the step-1 segments of at
  least 1.5 s, clipped to 10 s, and picked at 20 evenly spaced indices.
- `clip_emb.f32`: raw (not normalised) TitaNet-large embeddings from sherpa
  `SpeakerEmbeddingExtractor`, little-endian `f32`, row-major `[20, 192]`.
- `merged.json`: `{"speakers", "seconds", "unknown_seconds"}`, the result of spec §8.2 steps 6–7 on
  step-1 clusters. `unknown_seconds` lists the seconds that are not assigned to any speaker.

Merge details (spec §8.2 steps 6–7):
- Centroid windows (plan Task 14 rule): turns under 1.5 s are skipped; 1.5–3 s turns are used whole;
  turns of 3 s or more give consecutive 3 s windows from the turn start, and a remainder of at least
  1.5 s is its own window. With more than 60 windows, index `(i * (n - 1)) // 59` is kept for
  `i` in 0..60. The centroid is the normalised mean of the normalised window embeddings.
- Anchors (≥ 30 s) are compared, biggest first, with each kept anchor's ORIGINAL centroid. A
  match is cosine ≥ 0.7 to the most similar one, and seconds add up. Otherwise the anchor is kept.
  An anchor without a centroid is kept as its own speaker.
- Small clusters (3 s ≤ seconds < 30 s) attach to the nearest kept anchor's original centroid at
  cosine ≥ 0.65. Unmatched ones stay as separate speakers when there is no anchor, and become
  unknown otherwise.
- Steps 8–9 (self and known voices) need `levels.bin` and voiceprints, so they are not in this kit.

The script runs a self-check of the merge rules at start-up.

## Results (this run)

| Recording | speakers | seconds |
|---|---|---|
| 1-2-1_Ira | 2 | 1020.03, 366.49 |
| 1-2-1_Slava | 2 | 1645.99, 1376.49 |
| german-lesson | 2 | 1897.42, 1675.94 |
| meetup | 5 | 2378.94, 443.17, 248.59, 93.13, 35.61 |

The spike expected `meetup` ≥ 3 speakers with 3 of them at ≥ 30 s; this run gives 5, all at ≥ 30 s.
