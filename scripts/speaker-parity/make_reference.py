#!/usr/bin/env python3
"""Local parity reference kit: sherpa-onnx 1.13.8 outputs for the Rust speaker tests.

Run with `python -I` from the venv in requirements.txt. Writes OUT/<recording>/ files.
Nothing here is committed; the recordings contain real voices.
"""
import argparse
import hashlib
import json
import os

import av
import numpy as np
import sherpa_onnx as so

RECORDINGS = ["1-2-1_Ira", "1-2-1_Slava", "german-lesson", "meetup"]
SAMPLE_RATE = 16000
THREADS = 3
# name -> (bytes, sha256); the Models table in global-constraints.md.
MODELS = {
    "segmentation-3.0.onnx": (5992913, "220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079"),
    "titanet-large.onnx": (101405493, "d51abcf31717ef28162f26acb9d44dd4127c3d44c9b8624f699f3425daca8e77"),
}
CLUSTER_THRESHOLD = 0.7  # FastClustering cut at distance 0.7, i.e. cosine 0.3 (sherpa's threshold)
CLIP_COUNT, CLIP_MIN_SECS, CLIP_MAX_SECS = 20, 1.5, 10.0
SEG_MIN_SECS, SUB_SECS, MAX_CENTROIDS = 1.5, 3.0, 60
ANCHOR_SECS, SMALL_SECS = 30.0, 3.0
ANCHOR_MERGE, ATTACH = 0.7, 0.65


def check_models(models_dir):
    paths = {}
    for name, (size, sha) in MODELS.items():
        path = os.path.join(models_dir, name)
        with open(path, "rb") as f:
            data = f.read()
        if len(data) != size or hashlib.sha256(data).hexdigest() != sha:
            raise SystemExit(f"{path}: size or SHA-256 does not match the pinned model")
        paths[name] = path
    return paths


def decode(src, dst):
    """Stream to 16 kHz mono little-endian f32 (PyAV/libswresample, not the ffmpeg CLI)."""
    with av.open(src) as container, open(dst, "wb") as out:
        stream = next(s for s in container.streams if s.type == "audio")
        resampler = av.AudioResampler(format="flt", layout="mono", rate=SAMPLE_RATE)

        def write(frames):
            for frame in frames:
                out.write(frame.to_ndarray().astype("<f4").tobytes())

        for frame in container.decode(stream):
            write(resampler.resample(frame))
        write(resampler.resample(None))


def diarize(models, audio, window_shift_ratio):
    cfg = so.OfflineSpeakerDiarizationConfig(
        segmentation=so.OfflineSpeakerSegmentationModelConfig(
            pyannote=so.OfflineSpeakerSegmentationPyannoteModelConfig(
                model=models["segmentation-3.0.onnx"], window_shift_ratio=window_shift_ratio),
            num_threads=THREADS, provider="cpu"),
        embedding=so.SpeakerEmbeddingExtractorConfig(
            model=models["titanet-large.onnx"], num_threads=THREADS, provider="cpu"),
        clustering=so.FastClusteringConfig(num_clusters=-1, threshold=CLUSTER_THRESHOLD),
        min_duration_on=0.3, min_duration_off=0.5)
    result = so.OfflineSpeakerDiarization(cfg).process(audio)
    return [[float(s.start), float(s.end), int(s.speaker)] for s in result.sort_by_start_time()]


class Embedder:
    def __init__(self, path):
        self.ex = so.SpeakerEmbeddingExtractor(
            so.SpeakerEmbeddingExtractorConfig(model=path, num_threads=THREADS, provider="cpu"))

    def __call__(self, samples):
        stream = self.ex.create_stream()
        stream.accept_waveform(SAMPLE_RATE, samples)
        stream.input_finished()
        assert self.ex.is_ready(stream)
        return np.asarray(self.ex.compute(stream), dtype=np.float32)


def unit(v):
    return v / np.linalg.norm(v)


def pick_clips(segs):
    cands = [(s, min(e, s + CLIP_MAX_SECS)) for s, e, _ in segs if e - s >= CLIP_MIN_SECS]
    if len(cands) < CLIP_COUNT:
        raise SystemExit(f"only {len(cands)} speech segments of at least 1.5 s")
    idx = np.linspace(0, len(cands) - 1, CLIP_COUNT).round().astype(int)
    return [[round(cands[i][0], 3), round(cands[i][1], 3)] for i in idx]


def sub_windows(spans):
    """3 s tiles of each segment >= 1.5 s (a short segment is one window), at most 60, evenly spaced."""
    wins = []
    for s, e in spans:
        if e - s < SEG_MIN_SECS:
            continue
        n_full = int((e - s) // SUB_SECS)
        if n_full == 0:
            wins.append((s, e))
        else:
            wins += [(s + k * SUB_SECS, s + (k + 1) * SUB_SECS) for k in range(n_full)]
    if len(wins) > MAX_CENTROIDS:
        wins = [wins[i] for i in np.linspace(0, len(wins) - 1, MAX_CENTROIDS).round().astype(int)]
    return wins


def centroids(segs, audio, embed):
    """Cluster -> unit centroid of its sub-window embeddings (None when there is none)."""
    spans = {}
    for s, e, c in segs:
        spans.setdefault(c, []).append((s, e))
    cents = {}
    for c, sp in spans.items():
        embs = []
        for s, e in sub_windows(sp):
            v = embed(audio[int(round(s * SAMPLE_RATE)):int(round(e * SAMPLE_RATE))])
            if np.all(np.isfinite(v)) and np.linalg.norm(v) > 0:
                embs.append(unit(v))
        cents[c] = unit(np.mean(embs, axis=0)) if embs else None
    return cents


def merge(secs, cents):
    """Spec 8.2 steps 6-7: anchors merge greedily, small clusters attach or stay unknown.

    Shortcut: an anchor without a centroid becomes unknown, not its own speaker.
    """
    def cos(a, b):
        return float(np.dot(a, b))

    groups = []
    anchors = sorted((c for c in secs if secs[c] >= ANCHOR_SECS and cents[c] is not None),
                     key=lambda c: -secs[c])
    used = set()
    for c in anchors:
        sims = [cos(cents[c], g["cent"]) for g in groups]
        if sims and max(sims) >= ANCHOR_MERGE:
            g = groups[int(np.argmax(sims))]
            g["sum"] = g["sum"] + cents[c]
            g["cent"] = unit(g["sum"])
            g["secs"] += secs[c]
        else:
            groups.append({"sum": cents[c].copy(), "cent": cents[c], "secs": secs[c]})
        used.add(c)

    loose = []
    smalls = sorted((c for c in secs if SMALL_SECS <= secs[c] < ANCHOR_SECS), key=lambda c: -secs[c])
    for c in smalls:
        sims = [cos(cents[c], g["cent"]) for g in groups] if cents[c] is not None else []
        if sims and max(sims) >= ATTACH:
            groups[int(np.argmax(sims))]["secs"] += secs[c]
            used.add(c)
        else:
            loose.append(c)
    if groups:
        speakers = [g["secs"] for g in groups]
    else:
        speakers = [secs[c] for c in loose]
        used.update(loose)
    unknown = [secs[c] for c in secs if c not in used]
    speakers.sort(reverse=True)
    return {"speakers": len(speakers),
            "seconds": [round(x, 2) for x in speakers],
            "unknown_seconds": [round(x, 2) for x in sorted(unknown, reverse=True)]}


def self_check():
    e0, e1, e2 = np.eye(3)
    near = unit(e0 + 0.5 * e2)
    secs = {0: 40.0, 1: 35.0, 2: 40.0, 3: 5.0, 4: 1.0}
    cents = {0: e0, 1: near, 2: e1, 3: unit(e2 + 0.2 * e1), 4: e2}
    assert merge(secs, cents) == {"speakers": 2, "seconds": [75.0, 40.0], "unknown_seconds": [5.0, 1.0]}
    assert merge({0: 5.0, 1: 4.0}, {0: e0, 1: e1}) == {"speakers": 2, "seconds": [5.0, 4.0], "unknown_seconds": []}


def run(name, audio_dir, models, embed, out_root):
    out = os.path.join(out_root, name)
    os.makedirs(out, exist_ok=True)
    wav = os.path.join(out, "audio16k.f32")
    decode(os.path.join(audio_dir, name + ".mp4"), wav)
    audio = np.fromfile(wav, dtype="<f4")
    print(f"{name}: {len(audio) / SAMPLE_RATE:.1f} s decoded", flush=True)

    step1 = diarize(models, audio, 0.1)
    step2 = diarize(models, audio, 0.2)
    for tag, segs in (("step1", step1), ("step2", step2)):
        with open(os.path.join(out, f"diar_{tag}.json"), "w") as f:
            json.dump(segs, f)
    print(f"{name}: diarization done ({len(step1)} / {len(step2)} segments)", flush=True)

    clips = pick_clips(step1)
    with open(os.path.join(out, "clips.json"), "w") as f:
        json.dump(clips, f)
    clip_emb = np.stack([embed(audio[int(round(s * SAMPLE_RATE)):int(round(e * SAMPLE_RATE))])
                         for s, e in clips]).astype("<f4")
    assert clip_emb.shape == (CLIP_COUNT, 192), clip_emb.shape
    clip_emb.tofile(os.path.join(out, "clip_emb.f32"))

    secs = {}
    for s, e, c in step1:
        secs[c] = secs.get(c, 0.0) + (e - s)
    merged = merge(secs, centroids(step1, audio, embed))
    with open(os.path.join(out, "merged.json"), "w") as f:
        json.dump(merged, f)
    print(f"{name}: speakers={merged['speakers']} seconds={merged['seconds']}", flush=True)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--audio-dir", required=True)
    p.add_argument("--models", required=True)
    p.add_argument("--out", required=True)
    args = p.parse_args()
    self_check()
    models = check_models(args.models)
    embed = Embedder(models["titanet-large.onnx"])
    for name in RECORDINGS:
        if os.path.exists(os.path.join(args.out, name, "merged.json")):
            print(f"{name}: already done, skipping", flush=True)
            continue
        run(name, args.audio_dir, models, embed, args.out)


if __name__ == "__main__":
    main()
