# Dictate: dictionary-as-Whisper-prompt gate (plan Task 6)

Date: 2026-10-19. Run by the controller in the dev container.

**Setup:** whisper.cpp 1.7.1 + ggml-large-v3-turbo (the app's versions), beam 5, 3 threads. Saved chunks from `.superpowers/quality-debug/`:
- the Slava call: 53 Russian chunks, decoded with `-l ru`;
- the German lesson: 95 de/ru chunks, decoded with `-l auto`.

Each chunk was decoded twice: without a prompt, and with the worst-case English-only prompt `Conclave, Tauri, Claude Code, VMware, Groq.`. The script is `dict_gate.py`, in the session scratchpad.

## Result: FAIL

| Gate | Threshold | Measured |
|---|---|---|
| (a) prompt words inserted | 0 | 0 |
| (b) Cyrillic share of Russian chunks | drop ≤ 2 points | 84.8 % → 73.5 % (−11.3) |
| (c) new hallucinations | 0 | 0 |
| (d) words changed vs. no prompt | ≤ 3 % | 20.5 % (243/1188) |

By chunk length:

| Length | Chunks | Words changed | Russian chunks flipped from Cyrillic to Latin script |
|---|---|---|---|
| < 1.5 s | 37 | 31 % | 6 |
| 1.5–3 s | 31 | 35 % | 2 |
| ≥ 3 s | 80 | 18 % | 6 |

Examples, without the prompt → with it:
- «Что так?» → "Co tak?"
- «Не оправдан.» → "- Me apago."
- «Я...» → "- But yeah."
- «Мальшауэн – это типа «посмотрим».» → "Mal schauen, это типа посмотрим." (here the prompt helped)

## Decision (controller ruling, reported to the user)

- **Meetings:** no Whisper initial prompt. A non-empty dictionary is added to the summary system prompt as `Correct spellings of names and terms: …` (plan Task 7B, spec §9 fallback).
- **Dictation:** no Whisper initial prompt either. The same script flips occur on long chunks. The dictionary reaches cloud modes as spelling hints, which the cleanup prompt already does. "As spoken" mode gets no dictionary help.
- The `initial_prompt` parameter stays in `WhisperEngine` (unused) for a later per-language prompt experiment.
