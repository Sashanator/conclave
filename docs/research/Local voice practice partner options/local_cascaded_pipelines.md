# Local cascaded voice-conversation pipelines (STT -> LLM -> TTS) on one 16 GB GPU (RTX 4070 Ti SUPER), October 2026

Context for the report writer: the target is a personal English speaking-practice app (role-play characters, German later, transcript needed for a weak-spots report) inside Conclave (Tauri 2 + Rust on Windows 10, already running whisper.cpp large-v3-turbo on CUDA). Research was done on 2026-10-16. A summarising layer sat between the search tool and these notes, and many 2026 model names below (Gemma 4, Qwen3.6/3.8, Breeze TTS 2, Fish Audio Drama 3) appeared only in search snippets and aggregator pages. Treat them as "reported", not "verified". Most latency figures are self-reported by vendors or projects. I found **no independent, reproducible end-to-end benchmark on a 4070-class card**.

## STT: best streaming / low-latency options, VAD, turn detection, barge-in

### Takeaway
Fast GPU STT is no longer the bottleneck. Parakeet TDT 0.6B v2/v3, Canary, Kyutai STT, Moonshine Streaming and Whisper large-v3-turbo all land around 6-7% WER on the Open ASR Leaderboard. The real latency lever is end-of-turn detection: Silero VAD plus Pipecat Smart Turn v3.x (23 languages including English and German, BSD-2, about 10-65 ms on CPU). Kyutai STT is the only option with built-in semantic VAD, but it covers English and French only, not German.

### Cited Findings
- Open ASR Leaderboard snapshot (aggregator, dated 2026-05-06, may be stale), average WER: Canary-Qwen 2.5B 5.63; Parakeet TDT 0.6B v2 6.05; Parakeet v3 6.32; Canary 1B Flash 6.35; Kyutai 2.6B English 6.4; Moonshine Streaming Medium 6.66. The leaderboard covers 60+ systems on 11 datasets and also reports RTFx. — [benchmarklist.com Open ASR](https://benchmarklist.com/benchmarks/open_asr_leaderboard/)
- Parakeet TDT 0.6B v3: 6.34% average WER at RTFx about 3,333. The 1.1B variant has RTFx above 2,000. — [MarkTechPost ASR 2026 comparison](https://www.marktechpost.com/2026/07/23/best-open-speech-recognition-asr-models-in-2026-wer-languages-latency-and-license-compared/); [Gladia 2026 open-source STT](https://www.gladia.io/blog/best-open-source-speech-to-text-models)
- Streaming support is contested. One source says Moonshine and Vosk stream natively, while Parakeet TDT and faster-whisper stream only by chunking. Another describes Canary-Qwen 2.5B as batch-only and NeMo/Canary streaming as limited. — [AssemblyAI open-source STT](https://www.assemblyai.com/blog/top-open-source-stt-options-for-voice-applications); [speechdata.ai](https://www.speechdata.ai/blog/best-asr-models-2026)
- Kyutai STT 2.6B (English) has a 2.5 s delay and 6.4% WER. Kyutai STT ~1B (English/French) has a 0.5 s delay and built-in semantic VAD, and a GitHub issue notes that no accuracy figure has been published for it. — [localvoxtral issue #907](https://github.com/T0mSIlver/localvoxtral/issues/907)
- Kyutai STT 1B in Unmute: 6-token delay (~200 ms), target STT time-to-first-token under 50 ms, about 2.5 GB VRAM. — [Unmute docs (Mintlify mirror)](https://www.mintlify.com/kyutai-labs/unmute/requirements); [kyutai-labs/unmute README](https://github.com/kyutai-labs/unmute)
- Moonshine (project's own figures): Medium Streaming 107 ms latency on a MacBook Pro versus about 11.3 s for Whisper large-v3; Tiny Streaming 34 ms; Medium Streaming 6.65% WER. — [MarkTechPost](https://www.marktechpost.com/2026/07/23/best-open-speech-recognition-asr-models-in-2026-wer-languages-latency-and-license-compared/)
- Leaderboard datasets are mostly read or broadcast speech, not conversational speech. Latency numbers come from different hardware and methods and are not comparable. — [MarkTechPost](https://www.marktechpost.com/2026/07/23/best-open-speech-recognition-asr-models-in-2026-wer-languages-latency-and-license-compared/)
- Blog estimate: faster-whisper medium on a GPU takes about 150-250 ms per utterance. — [Botmonster local voice 2026](https://botmonster.com/smart-home/build-private-local-ai-voice-assistant-2026/)
- Smart Turn (Pipecat), now v3.2:
  - about 8M parameters on a Whisper-Tiny backbone; about 8 MB int8 for CPU and about 32 MB fp32 for GPU (the GPU build is about 1% more accurate);
  - 23 languages, including English and German; BSD 2-Clause licence;
  - runs in 10 ms on some CPUs, under 100 ms on most cloud instances, and about 65 ms on a Pipecat Cloud 1x instance;
  - designed to run after Silero VAD detects silence, on the whole current turn (last 8 s). If the user resumes speaking, it reruns on the whole turn.
  - — [pipecat-ai/smart-turn](https://github.com/pipecat-ai/smart-turn); [Pipecat Smart Turn docs](https://docs.pipecat.ai/api-reference/server/utilities/turn-detection/smart-turn-overview)
- Smart Turn v3 breaks when the input is 8 kHz. Keep the input at 16 kHz (third-party report about Twilio, Pipecat issue #3844). — [Evalgent blog](https://www.evalgent.com/blog/pipecat-interruption-strategy-smart-turn)
- Third-party reading of Pipecat 1.12: VAD stop_secs and Smart Turn inference fall inside the STT deadline rather than adding to it (unverified). — [luonghongthuan Pipecat production guide](https://luonghongthuan.com/en/blog/pipecat-voice-agent-production-scalable-guide/)

### Inferences
- For Conclave, keeping whisper.cpp large-v3-turbo (already integrated, multilingual, handles German) is reasonable. The bigger win is adding Silero VAD plus a Smart Turn ONNX model in Rust (for example through the `ort` crate) rather than switching engines. Parakeet v3 is the faster alternative for European languages, and Conclave already has a Parakeet path.
- Conclave's own memory notes report wrong-language detection on short chunks. For a practice app, pin the language per session (English or German) instead of auto-detecting.
- Barge-in works the same way in every framework: VAD fires while TTS is playing, playback stops, and the LLM/TTS generation is cancelled. No source gave false-barge-in rates. An echo-cancelled mic, or headphones, is effectively required on a PC.

### Gaps
- No measured STT latency on a 4070 Ti SUPER for any of these models.
- No WER on conversational or L2 (non-native) English. That matters a lot for a speaking-practice app, because learner accents and errors may be "auto-corrected" by a strong STT, which hides mistakes from the weak-spots report.
- I found no current figures for the LiveKit turn detector.

## LLM: local models for role-play that fit beside STT and TTS, and time-to-first-token

### Takeaway
With 16 GB shared with STT and TTS, the practical LLM budget is about 8-11 GB. That means a dense 8-14B model at Q4, or a small MoE such as gpt-oss-20B, rather than the 26-27B models that 16 GB guides recommend when the LLM has the card to itself. I found no measured TTFT figures on this card. Generation speed (about 40-110 tok/s) is more than enough for speech, and with short role-play prompts TTFT should be well under a second.

### Cited Findings
- Gemma 4 26B-A4B QAT is the most-cited "best for 16 GB" pick when the LLM is alone on the card: 4B active parameters, about 17 GB download, about 15 GB VRAM, 8K-16K context with KV quantisation. Gemma 4 12B is the easy fit, and Google names 16 GB as its local target. — [runaihome 16 GB guide](https://www.runaihome.com/blog/best-local-llm-16gb-vram-2026/); [MindStudio Gemma 4 Ollama](https://www.mindstudio.ai/blog/how-to-run-gemma-4-locally-ollama)
- Qwen 3.5/3.6 27B is tight on 16 GB. One source says Q4_K_M is about 16 GB; another says Qwen3.6-27B Q4 needs about 18.9 GB loaded, so the real tier is 24 GB. On 16 GB it needs IQ3/Q3. — [runaihome](https://www.runaihome.com/blog/best-local-llm-16gb-vram-2026/); [omidsaffari 2026](https://omidsaffari.com/blog/best-local-llms-2026)
- Role-play/uncensored guide: Qwen3.8-27B at Q3_K_P or IQ3_M is practical on 16 GB; IQ4_XS is too tight. — [bswen uncensored 16 GB](https://docs.bswen.com/blog/2026-03-10-uncensored-llm-16gb-vram/)
- Generation speed on an RTX 5060 Ti 16 GB: GPT-OSS 20B at 82-111 tok/s; Gemma 4 26B-A4B at about 40-50 tok/s. These are not TTFT figures. — [runaihome](https://www.runaihome.com/blog/best-local-llm-16gb-vram-2026/)
- RTX 4070 Ti SUPER generation speed:
  - Qwen3 14B Q4_K_M estimated at about 45 tok/s (an engine estimate, not a measurement) — [ModelFit](https://modelfit.io/can-i-run/qwen3-14b-q4-on-rtx-4070-ti-super/)
  - Qwen 2.5 14B Q4_K_M "measured" at 54 tok/s, without saying whether that is prompt processing or generation — [kunalganglani llm-benchmarks](https://kunalganglani.com/llm-benchmarks)
- Guide estimates for first-token latency inside a voice loop: 0.5-1.5 s, 0.5-2 s, or 200-400 ms with a small model. — [promptquorum voice assistant 2026](https://www.promptquorum.com/power-local-llm/build-local-voice-assistant-2026); [local-llm.net guide](https://www.local-llm.net/guides/local-voice-assistant/); [Botmonster](https://botmonster.com/smart-home/build-private-local-ai-voice-assistant-2026/)
- On a 4070, Whisper, the LLM and TTS compete for GPU cycles, so expect contention when all three share the card. — [kunalganglani 4060 Ti vs 4070](https://www.kunalganglani.com/blog/rtx-4060-vs-rtx-4070-for-ai)
- Unmute's local default LLM is tiny. The README says Gemma 3 1B via vLLM; the Mintlify docs say Llama 3.2 1B, about 6.1 GB under vLLM. This is a conflict. Production unmute.sh uses GPT-OSS 120B via OpenRouter. — [unmute README](https://github.com/kyutai-labs/unmute); [Unmute requirements (Mintlify)](https://www.mintlify.com/kyutai-labs/unmute/requirements)
- KoljaB (RealtimeVoiceChat author) self-reports about 500 ms response latency even with a 24B Mistral fine-tune via Ollama, hardware unspecified beyond "decent CUDA GPU". — [HN thread](https://hn.nuxt.dev/item/43899028)

### Inferences
- TTFT in a voice loop is dominated by prompt processing of the system prompt plus history. On a 4070 Ti SUPER, llama.cpp prompt processing for 8-14B Q4 models typically runs at thousands of tok/s (general knowledge, not sourced here). With prompt caching (llama.cpp `cache_prompt`, KV reuse across turns), TTFT for a 1-2K-token role-play context should be about 100-300 ms. Measure it with `llama-bench -p 512 -n 128` and server timings.
- Candidates to evaluate, in VRAM order:
  - Gemma 4 12B Q4, about 7-8 GB (estimate);
  - Qwen3.x 14B Q4, about 9 GB;
  - gpt-oss-20B (MXFP4, about 12-13 GB from training knowledge; probably too large beside STT and TTS);
  - Mistral Small 24B would need Q3 and leaves no room.
- For a learner, the character must stay in plain, level-appropriate English and keep turns short. Prompting matters more than raw benchmark rank, and a 12-14B model is likely sufficient.
- Disable "thinking" modes (Qwen3, gpt-oss reasoning). A reasoning preamble adds seconds before the first spoken word.
- Fallback: Conclave already supports cloud LLMs with the user's own keys. A cloud LLM (for example Groq) in the middle of a local STT/TTS cascade frees about 8 GB VRAM and often gives lower TTFT than local. This trades privacy for the user's stated #1 priority, cost, which is roughly neutral here.

### Gaps
- No measured TTFT or prompt-processing speed on an RTX 4070 Ti SUPER for any 2026 model.
- No role-play quality benchmark covering these models. Several "Qwen3.6/3.8" and "Gemma 4" claims come only from SEO-style 2026 guides with inconsistent sizes.

## TTS: naturalness, streaming latency, VRAM, English/German, licence

### Takeaway
On the Artificial Analysis open-weights arena (Oct 2026):
- Top 3: Fish Audio Drama 3 Preview (Elo about 1244), Breeze TTS 2 (about 1222), Fish Audio S2 Pro (about 1116).
- Next: Step Audio EditX (1097), Voxtral TTS (1087), NVIDIA Magpie-Multilingual 357M (1063).
- Kokoro 82M (1061, Apache-2.0) still ties Magpie and beats Chatterbox, Higgs Audio V3, VibeVoice and XTTS v2.

For a personal app on 16 GB, the pragmatic pair is Kokoro for English (tiny, extremely fast, no German) and Chatterbox Multilingual V3 (MIT, 500M, 23 languages including German) or Fish S2 Pro (German, research licence) for German and expressiveness.

### Cited Findings
- Artificial Analysis Provider Voice Arena. Elo 95% intervals are about ±9 to ±19, and ranges mean statistical ties; Zonos-v0.1 is the 1000 anchor.

  | Model | Elo | Released |
  |---|---|---|
  | Fish Audio Drama 3 Preview | 1244 | Oct 2026 |
  | Breeze TTS 2 | 1222 | Aug 2026 |
  | Fish Audio S2 Pro | 1116 | Mar 2026 |
  | Step Audio EditX | 1097 | |
  | Voxtral TTS (Mistral) | 1087 | |
  | Magpie-Multilingual 357M (NVIDIA) | 1063 | |
  | Kokoro 82M v1.0 | 1061 | |
  | Maya1 | 1046 | |
  | OpenAudio S1 Mini | 1044 | |
  | Higgs Audio V3 | 1038 | |
  | Chatterbox | 1025 | |
  | Zonos v0.1 | 1000 | |
  | VibeVoice 1.5B | 956 | |
  | XTTS v2 | 915 | |
  | StyleTTS 2 | 894 | |

  The fetched view did not clearly show the open-weights filter as applied, and no snapshot date is given. — [Artificial Analysis open-weights TTS leaderboard](https://artificialanalysis.ai/text-to-speech/leaderboard/provider-voice/open-weights)
- Older snapshots named Fish Audio S2 Pro (1123) or Step Audio EditX as the top open model. Rankings move month to month. — [Artificial Analysis TTS leaderboard](https://artificialanalysis.ai/text-to-speech/leaderboard); [lukeocodes open TTS](https://lukeocodes.dev/open-weights-eating-ai-best-open-tts-models)
- Licences:
  - Breeze TTS 2, Fish Audio S2 Pro and Voxtral TTS are reported as non-commercial or research only.
  - Step Audio EditX is Apache-2.0.
  - Fish's own repo says the S2-Pro weights are under the Fish Audio Research License, and some aggregators wrongly say Apache-2.0.
  - S2 Pro supports German, with a stated streaming latency of about 97 ms.
  - — [yaps.ai local TTS licences](https://www.yaps.ai/blog/best-local-tts-models); [fishaudio/fish-speech](https://github.com/fishaudio/fish-speech); [Cartesia open-source TTS by licence](https://www.cartesia.ai/learn/open-source-tts)
- Scores from blogs (for example "Chatterbox 1481", or Zonos2/CosyVoice 3 at 1200-1300) use different scales and do not match Artificial Analysis. Discount them. — [offlinetts ranking](https://offlinetts.com/blog/tts-model-ranking-2026/)
- Kokoro-82M: Apache-2.0, 54 voices in 9 languages (American and British English, Japanese, Mandarin, Spanish, French, Hindi, Italian, Brazilian Portuguese). **No German.** No voice cloning. — [Kokoro model card](https://huggingface.co/hexgrad/Kokoro-82M); [Kokoro VOICES.md](https://huggingface.co/hexgrad/Kokoro-82M/blob/main/VOICES.md)
- Kokoro speed: README claims 35x-100x+ real time on a 4060 Ti or better; another guide gives RTF about 0.03 on a high-end GPU. — [Kokoro-FastAPI](https://github.com/moses-root/Kokoro-FastAPI); [promptquorum](https://www.promptquorum.com/power-local-llm/build-local-voice-assistant-2026)
- Chatterbox (Resemble AI, MIT):
  - Variants: Turbo 350M (English, one-step distilled decoder for low latency, paralinguistic tags such as [laugh]); Nano 110M (English, 3x real time on 8 CPU cores); Multilingual V3 500M (23 languages, **German included**); original 500M (exaggeration/cfg controls).
  - Every output carries a Perth watermark.
  - The repo makes no explicit streaming claim for the open models. "Sub 200 ms" refers to Resemble's paid hosted service.
  - — [resemble-ai/chatterbox](https://github.com/resemble-ai/chatterbox)
- One source gives Chatterbox-Turbo about 470 ms to first chunk. — [Cartesia open-source TTS](https://www.cartesia.ai/learn/open-source-tts)
- Blog figures on a 4070 (unverified): XTTS-v2 about 200 ms to first chunk, F5-TTS about 350 ms. — [kunalganglani local voice assistant](https://www.kunalganglani.com/blog/local-ai-voice-assistant-whisper-piper-ollama)
- Orpheus: creators claim about 200 ms latency, about 100 ms with input streaming (third-party relay). — [Dograh open-source TTS comparison](https://www.dograh.com/feeds/blog/open-source-tts-comparison)
- Kyutai TTS:
  - 220 ms from the first text token to the first audio chunk;
  - on an L40S it serves 32 concurrent requests at 350 ms, or 16 connections at more than 2x real time;
  - about 5.3 GB VRAM in Unmute;
  - TTS latency is about 750 ms when everything shares a single L40S, versus about 450 ms with separate GPUs.
  - — [Kyutai TTS page](https://kyutai.org/next/tts); [unmute README](https://github.com/kyutai-labs/unmute)
- Qwen3-TTS is listed by one aggregator as strong multilingual streaming under Apache-2.0 (single source, not verified). — [pinggy self-hosted TTS 2026](https://pinggy.io/blog/best_open_source_self_hosted_text_to_speech_models/)

### Inferences
- **Superseded or not worth picking in 2026:** XTTS v2 (Coqui is defunct, CPML licence, Elo 915), StyleTTS 2, MetaVoice, OpenVoice v2, Piper (fine for robots and Home Assistant, but clearly synthetic for conversation practice). Zonos v0.1 and VibeVoice 1.5B rank below Kokoro. Sesame CSM, Dia and Orpheus did not appear on the current arena view, so their standing is unknown.
- Kokoro's weakness for this use case is prosody on conversational text (one user reported "weird pronunciation") and the lack of per-character emotion. Distinct voices per character (professor vs mechanic) are available from its 54 presets. Chatterbox or Fish S2 can clone a reference voice per character.
- Licences are personal-use only for Fish S2 Pro and Breeze. That is acceptable for this personal app but would block a public release.

### Gaps
- No measured VRAM for Chatterbox Multilingual V3, Fish S2 Pro, Breeze TTS 2 or Fish Drama 3. No measured time-to-first-audio on a 4070 Ti SUPER for any of them.
- I could not verify German support for Breeze TTS 2, Voxtral TTS, Step Audio EditX or Magpie-Multilingual.
- No Windows-native (non-Python) runtime was found for the top models. Kokoro has ONNX ports (general knowledge), which matter for embedding in a Rust/Tauri app.

## End-to-end frameworks that wire this together locally; Windows/WSL2; measured voice-to-voice latency

### Takeaway
Two kinds of framework exist:
- **Integrated stacks:** Kyutai Unmute (MIT, needs at least 16 GB, Linux or WSL2 only, English/French) and KoljaB's RealtimeVoiceChat (RealtimeSTT + Ollama + Kokoro/Orpheus/Coqui, about 500 ms self-reported, manual install on Windows).
- **Toolkits:** Pipecat and LiveKit Agents, Python, with local Whisper/Kokoro/Ollama plug-ins and Smart Turn.

Claimed voice-to-voice latency ranges from about 450-500 ms (Unmute production on multiple GPUs; KoljaB self-report) to 1-3 s in blog estimates for naive single-GPU setups. Nobody has published a reproducible single-consumer-GPU measurement.

### Cited Findings
- Unmute:
  - MIT; needs an x86_64 CUDA GPU with at least 16 GB;
  - per-service VRAM: STT about 2.5 GB, TTS about 5.3 GB, LLM about 6.1 GB, about 14 GB in total;
  - "Running on Windows natively is not supported", so use WSL; no Mac support;
  - the LLM can be swapped for any OpenAI-compatible server (vLLM default, Ollama via `KYUTAI_LLM_URL`, OpenRouter);
  - unmute.sh claims about 450 ms response using separate GPUs; TTS alone is about 750 ms on one shared L40S;
  - the README does not cover barge-in;
  - the STT detects end of speech (semantic VAD).
  - — [kyutai-labs/unmute](https://github.com/kyutai-labs/unmute); [Unmute requirements (Mintlify)](https://www.mintlify.com/kyutai-labs/unmute/requirements)
- RealtimeVoiceChat (KoljaB):
  - the author claims about 500 ms latency even with a 24B Mistral fine-tune on Ollama;
  - needs a "decent CUDA GPU";
  - supports Kokoro, Coqui and Orpheus;
  - one source says manual install works on Windows, Linux and macOS; another says Linux is recommended for Docker GPU;
  - a user found Kokoro "fast, but some pronunciation is weird".
  - — [HN discussion](https://hn.nuxt.dev/item/43899028); [sourcepulse RealtimeVoiceChat](https://www.sourcepulse.org/projects/2399606)
- Pipecat local example: Apoorva87/local-voice-agent runs Pipecat 1.7 + Smart Turn v3 + MLX Whisper + Ollama + Kokoro fully on-device (Mac) and logs per-stage timings. — [local-voice-agent](https://github.com/Apoorva87/local-voice-agent)
- Pipecat ships LocalSmartTurnAnalyzerV3 (ONNX, bundled model). — [Pipecat Smart Turn docs](https://docs.pipecat.ai/api-reference/server/utilities/turn-detection/smart-turn-overview)
- Blog latency budgets:
  - target 500-800 ms mouth-to-ear, 1,100 ms upper bound — [dev.to Voice AI guide](https://dev.to/programmerraja/2025-voice-ai-guide-how-to-make-your-own-real-time-voice-agent-part-3-3ocb)
  - 1-2 s with Whisper large-v3 + Llama 3.1 8B on an RTX 3060 12 GB — [promptquorum](https://www.promptquorum.com/power-local-llm/build-local-voice-assistant-2026)
  - 1.5-3 s with a 4090 + small.en + Qwen 2.5 14B + Kokoro — [local-llm.net](https://www.local-llm.net/guides/local-voice-assistant/)
  - These are estimates, not measurements.
- Streaming LLM output into TTS sentence by sentence cuts perceived latency by 0.3-0.7 s (blog). — [promptquorum](https://www.promptquorum.com/power-local-llm/build-local-voice-assistant-2026)
- Research reference: the i-LAVA paper reports a first audio chunk in as little as 640.9 ms on GPU for its low-latency voice-to-voice cascade. — [arXiv 2509.20971](https://arxiv.org/html/2509.20971v1)
- A vendor reports that a well-built cascade responds in about 1 s end to end, with about 110 ms p50 STT time-to-final-segment. — [AssemblyAI cascaded vs S2S](https://www.assemblyai.com/blog/speech-to-speech-for-voice-agents)

### Inferences
- **Realistic single-4070-Ti-SUPER budget** with streaming at every stage:
  - VAD hangover plus Smart Turn: about 200-400 ms
  - final STT on a short utterance (whisper turbo or Parakeet): about 50-200 ms
  - LLM TTFT plus the first sentence (12-14B Q4, cached prompt, about 15-25 tokens at 45-60 tok/s): about 300-600 ms
  - TTS first chunk: Kokoro about 50-150 ms; Chatterbox or Fish about 200-500 ms
  - **Total: about 0.7-1.3 s** voice to voice.

  This is my estimate, assembled from the component figures above, not a measurement. Unmute's own number (about 450 ms) needs separate datacentre GPUs; on one shared GPU its TTS alone degraded to about 750 ms.
- **Windows fit:** none of the Python frameworks run natively inside Conclave. For a Tauri/Rust app the options are:
  - (a) run Pipecat or RealtimeVoiceChat as a Python sidecar on Windows;
  - (b) run Unmute in WSL2 with Docker (heavy, English/French only, Kyutai TTS has no German);
  - (c) build the cascade natively in Rust, reusing whisper.cpp, llama.cpp (Conclave already bundles a `llama-helper` sidecar), Silero VAD and Smart Turn via ONNX, and Kokoro via ONNX.

  Option (c) best matches the existing architecture and avoids WSL2 GPU passthrough overhead.
- Transcripts for the weak-spots report come for free in a cascade (STT text plus LLM text). This is a structural advantage over end-to-end speech models.

### Gaps
- No measured voice-to-voice latency for any framework on an RTX 4070 Ti SUPER or any 16 GB consumer card.
- Pipecat and LiveKit Agents support for native Windows (not WSL) was not confirmed in this research.
- No current figures for Vocode, and no confirmation that it is still maintained.

## Total VRAM budget for a good combo on 16 GB, and expected voice-to-voice latency

### Takeaway
A good all-local combo fits in roughly 11-14 GB:
- whisper.cpp large-v3-turbo or Parakeet (about 1.5-2.5 GB)
- a 12-14B Q4 LLM (about 7-9 GB plus KV)
- Kokoro (<1 GB) or Chatterbox Multilingual (a few GB)
- Smart Turn and Silero (negligible)

Expected latency is about 0.7-1.3 s voice to voice. Unmute's reference stack uses about 14 GB, but only with a 1B LLM.

### Cited Findings
- Unmute's reference split is STT 2.5 GB + TTS 5.3 GB + LLM 6.1 GB, about 14 GB with a 1B LLM. At least 16 GB is required for one GPU, and 3 or more GPUs are recommended for best performance. — [Unmute requirements (Mintlify)](https://www.mintlify.com/kyutai-labs/unmute/requirements); [unmute README](https://github.com/kyutai-labs/unmute)
- Gemma 4 26B-A4B QAT alone takes about 15 GB, so it cannot share the card with STT and TTS. — [runaihome](https://www.runaihome.com/blog/best-local-llm-16gb-vram-2026/)
- Qwen3 14B Q4_K_M is estimated to fit a 4070 Ti SUPER at about 45 tok/s (estimate). — [ModelFit](https://modelfit.io/can-i-run/qwen3-14b-q4-on-rtx-4070-ti-super/)
- Smart Turn takes 8-32 MB. — [smart-turn](https://github.com/pipecat-ai/smart-turn)
- Kokoro has 82M parameters. — [Kokoro model card](https://huggingface.co/hexgrad/Kokoro-82M)

### Inferences
- Suggested budgets. Component sizes for whisper turbo, Parakeet and Chatterbox VRAM are from general knowledge or estimates, not sources found here; verify them with nvidia-smi.
  - **English, fastest:** whisper large-v3-turbo (~1.6 GB fp16 weights + runtime about 2 GB) + Gemma 4 12B or Qwen3 14B Q4_K_M with 8K context (~9-10 GB) + Kokoro (~0.5-1 GB) + Silero/Smart Turn. **About 12-13 GB**, leaving headroom for the Windows desktop compositor and a browser.
  - **German / expressive:** swap Kokoro for Chatterbox Multilingual V3 (500M; probably 3-4 GB in fp16/fp32, unverified) and drop the LLM to about 8B-12B. **About 13-15 GB.** Tight.
- GPU compute contention: STT, LLM and TTS run largely in sequence within a turn, so contention is small. Overlap happens only when TTS synthesis of sentence 1 runs while the LLM is still generating sentence 2, which slows both somewhat. Pin all three to the single GPU and avoid batching.
- Windows desktop and other apps typically hold 0.5-1.5 GB of VRAM. Plan for about 14.5 GB usable, not 16.

### Gaps
- No source measured the combined VRAM of these exact components.

## How a cascade compares with end-to-end models and ChatGPT Advanced Voice Mode

### Takeaway
End-to-end and full-duplex models (Moshi about 160 ms; OpenAI Realtime/AVM about 200-300 ms) win on latency, overlap, backchannels and prosody. A well-streamed cascade lands around 0.5-1 s and sounds like "a fast, polite turn-taker" rather than a human. For practice with a needed transcript and a weak-spots report, the cascade's text at every step is an advantage. No controlled naturalness (MOS or preference) comparison against GPT Realtime/AVM was found.

### Cited Findings
- 2026 French full-duplex benchmark paper: cascades stay attractive because each component can be improved independently, but they add latency and lose acoustic information. Speech-to-speech systems suit full-duplex interaction (listening while speaking, backchannels, overlap) better. — [arXiv 2609.10765](https://arxiv.org/pdf/2609.10765)
- MTR-DuplexBench: cascades show "extremely high latency", and first-packet latency alone is insufficient, so report multi-round latency. — [arXiv 2511.10262](https://arxiv.org/pdf/2511.10262)
- Vendor figures: Moshi about 160 ms; GPT-4o Realtime targets 200-300 ms end to end. — [Modulate ebook](https://www.modulate.ai/ebooks/beat-the-black-box-why-cascade-beats-speech-to-speech-for-enterprise-voice-agents)
- Speech-to-speech is 85% lower latency than a non-streaming cascade, but the gap "shrinks substantially" against an optimised streaming cascade. Cascades still win for most 2026 enterprise deployments because they are easier to evaluate. — [Coval S2S vs cascaded](https://www.coval.ai/blog/speech-to-speech-vs-cascaded-voice-ai-which-architecture-should-you-deploy/)
- Cascade voices are synthesised from flat text with less emotional nuance, while S2S captures tone and prosody directly (a qualitative claim). Hybrids are emerging. — [concret.io](https://www.concret.io/blog/cascade-vs-speech-to-speech-voice-agent-architecture)
- ICASSP 2026 HumDial challenge study on full-duplex interaction (title only reviewed). — [arXiv 2604.21406](https://arxiv.org/pdf/2604.21406)

### Inferences
- For language practice specifically, the cascade's weaknesses (no backchannels, no overlap, flat text-to-voice) matter less than its strengths:
  - an exact transcript of what the learner said (needed for the weak-spots report);
  - controllable vocabulary and level through the LLM prompt;
  - per-character voices;
  - German later by swapping models.
- An end-to-end local model on 16 GB (for example Moshi) would also hurt the transcript requirement and role-play quality, because of its small built-in LLM.
- One caveat for the weak-spots report: Whisper-class STT tends to normalise learner errors (grammar and pronunciation) into fluent text. The report may need a separate, lower-temperature or verbatim transcription pass, or word-level confidence, to catch mistakes.

### Gaps
- No MOS or human-preference study comparing a local cascade with GPT Realtime/AVM or Moshi.
- I did not check whether new 2026 open full-duplex models that fit in 16 GB with German support have appeared since Moshi.
