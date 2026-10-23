# Local end-to-end speech-to-speech models on a 16 GB GPU (RTX 4070 Ti SUPER), as of October 2026

Scope: open-weight end-to-end (speech-to-speech, full-duplex, omni) models for an English speaking-practice role-play app (professor, doctor, mechanic). German would be a bonus. Target: 16 GB VRAM on Windows 10 or WSL2, possibly with Whisper (~2-3 GB) also loaded.
Research date: 2026-10-16. About 19 tool calls. Several older models (GLM-4-Voice, Freeze-Omni, LLaMA-Omni 2, Mini-Omni, VITA-Audio, Baichuan-Omni, Sesame CSM, Ultravox) could not be checked in this budget. They are listed under Gaps rather than described from memory.

## Q1. NVIDIA NemotronLabs VoiceChat 11B: does it really run on 16 GB (Q4_K_M claim), on Windows or WSL2?

### Takeaway
Partly true, with a correction. No official quantized build exists, and NVIDIA's documented path is Linux with vLLM and an 80 GB GPU. One unofficial community GGUF does exist (Q8_0 and Q4_0, not Q4_K_M), and it runs end to end, including the speech encoder and the TTS/codec, through a small llama.cpp fork that ships Windows CUDA binaries. The quant sizes suggest the model should fit on 16 GB. However, nobody has published a measured run on a 16 GB or 24 GB card, there is no quality benchmark of the quant, the fork does WAV in and WAV out rather than a live mic, and the model has no persona or chat template in that fork. It is English-only and officially "research purposes only".

### Cited Findings
**Official release**
- Released August 3, 2026, v1.0. It is 11B parameters with a hybrid Mamba/Transformer design: a Fast Conformer speech encoder, a Nemotron Nano v2 LLM backbone, an NVIDIA TTS decoder and codec, and a separate output channel for tool calls — [HF model card](https://huggingface.co/nvidia/NVIDIA-NemotronLabs-VoiceChat-11B)
- The official checkpoint is 44.4 GB of safetensors — [HF search summary of nvidia repo](https://huggingface.co/nvidia/NVIDIA-NemotronLabs-VoiceChat-11B/tree/main)
- The card lists the A100, H100, H200, B100, B200 and RTX-6000 GPUs. It was tested on an H100, and the supported OS is Linux. The runtime is vLLM, and interactive use runs in an NVIDIA container (CUDA, Triton, vLLM) over a WebSocket. The card gives no VRAM figure — [HF model card](https://huggingface.co/nvidia/NVIDIA-NemotronLabs-VoiceChat-11B)
- NVIDIA's NeMo repo states the hardware requirement as an NVIDIA GPU with at least 80 GB of memory — [NVIDIA-NeMo/Speech, nemotron-labs-voicechat branch](https://github.com/NVIDIA-NeMo/Speech/tree/nemotron-labs-voicechat) (from search-result summary; the page was not fetched directly)
- Language: English only — [HF model card](https://huggingface.co/nvidia/NVIDIA-NemotronLabs-VoiceChat-11B)
- Licence: OpenMDW License Agreement v1.1 — [HF model card](https://huggingface.co/nvidia/NVIDIA-NemotronLabs-VoiceChat-11B). Coverage quotes the model card as "ready for research purposes only" — [Yowox](https://yowox.com/posts/nvidia-nemotron-voicechat-tool-calling/). Another source also says it is labelled research-only despite the permissive OpenMDW weights licence — [search summary citing MarkTechPost](https://www.marktechpost.com/2026/08/09/nvidia-releases-nemotronlabs-voicechat-11b-an-open-full-duplex-speech-to-speech-model-with-450-ms-turn-taking-and-live-tool-calling/). **Conflict or ambiguity:** my fetch of the HF card returned the OpenMDW licence and "researchers, developers, and professionals" as users, but no explicit research-only line. Secondary sources quote one.
- The card supports system prompts and a default function-calling template. Its example persona is "NVIDIA Voice Chat", and system prompts and tool responses must be ASCII-only. Voice options are not stated — [HF model card](https://huggingface.co/nvidia/NVIDIA-NemotronLabs-VoiceChat-11B)
- Outputs are agent text, agent speech at 22.05 kHz, and user transcription text, so transcripts of both sides are available. Input is 16 kHz speech plus a text prompt — [HF model card](https://huggingface.co/nvidia/NVIDIA-NemotronLabs-VoiceChat-11B)
- It is full-duplex with barge-in. Turn-taking takes about 448 ms and yielding to a user interruption about 480 ms, measured on an H100. On Full-Duplex-Bench 1.0, smooth turn-taking TOR is 0.82, user-interruption TOR is 1, the GPT-4o rating is 4.33, and pause-handling TOR is 0.153 (synthetic) and 0.255 (Candor). It ranks #2 among open full-duplex models on both VoiceBench and FDB 1.0, but the card gives no numeric VoiceBench score — [HF model card](https://huggingface.co/nvidia/NVIDIA-NemotronLabs-VoiceChat-11B)
- Known failure modes include generation artifacts, repeated or truncated speech and runaway continuation. It also shows clarification loops, unreliable instruction following, transcription drops, and trouble in noisy rooms. It is "not explicitly trained for broad reasoning or alignment" — [Yowox](https://yowox.com/posts/nvidia-nemotron-voicechat-tool-calling/)
- A "two-minute context ceiling and degradation after several turns" is reported in secondary coverage — [MarkTechPost (via search summary, not fetched; treat as unverified)](https://www.marktechpost.com/2026/08/09/nvidia-releases-nemotronlabs-voicechat-11b-an-open-full-duplex-speech-to-speech-model-with-450-ms-turn-taking-and-live-tool-calling/)

**Quantized builds**
- HF lists 8 quantized derivatives: two mlx-community MLX builds (4-bit and 8-bit, Aug 8), two roman220220 GPTQ-MLX builds (3-bit and mixed), and OsaurusAI JANG_4, JANG_2 and MXFP8 builds (Aug 31). There is **one GGUF: hoidhxd/NVIDIA-NemotronLabs-VoiceChat-11B-GGUF** (Aug 17, about 2.06k downloads) — [HF quantized-derivatives listing](https://huggingface.co/models?other=base_model:quantized:nvidia/NVIDIA-NemotronLabs-VoiceChat-11B)
- MLX sizes are bf16 22.2 GB, 8-bit 13.9 GB and 4-bit 9.2 GB, where the 4-bit tier quantizes only the LLM and vocab heads. The mixed GPTQ-MLX build reports 6.4 GB of weights and 8.0 GB peak memory. **These run on Apple Silicon only**, not on NVIDIA — [roman220220 mixed](https://huggingface.co/roman220220/NemotronLabs-VoiceChat-11B-gptq-mlx-mixed), [mlx-community bf16](https://huggingface.co/mlx-community/NemotronLabs-VoiceChat-11B-mlx-bf16)
- The GGUF repo has **only Q8_0 (about 12.04 GB) and Q4_0 (about 6.62 GB)**, plus an F16 intermediate (22.2 GB). **There is no Q4_K_M.** A `llamacpp/` split built from Q4_0 includes the LLM (nemotron_h, 4.67 GiB), the function/turn-taking head (315 MiB), the speech encoder as an mtmd audio projector (435 MiB), and the speech generator (gemma3 backbone + MoG head + 31-stage RVQ + codec decoder, 686 MiB). That makes the full speech pipeline about 6.1 GiB at Q4_0 — [hoidhxd GGUF](https://huggingface.co/hoidhxd/NVIDIA-NemotronLabs-VoiceChat-11B-GGUF)
- **Stock llama.cpp cannot load it** ("unknown model architecture: 'nemotron_voicechat'"). It needs the fork sansamour/llama-voicechat.cpp, which runs on CPU or CUDA without torch or NeMo — [hoidhxd GGUF](https://huggingface.co/hoidhxd/NVIDIA-NemotronLabs-VoiceChat-11B-GGUF)
- The VRAM figures on the GGUF page are targets, not measurements: Q8_0 needs about 16 GB (or 12 GB with CPU offload) and Q4_0 fits 8, 12 or 16 GB tiers. The page warns that runtime VRAM exceeds file size because of the KV cache, activations and audio buffers. It gives no quality benchmarks, does not claim the quant matches the original, and calls itself an unofficial conversion — [hoidhxd GGUF](https://huggingface.co/hoidhxd/NVIDIA-NemotronLabs-VoiceChat-11B-GGUF)
- llama-voicechat.cpp provides **prebuilt Windows x64 CPU (AVX2) and CUDA 12.8 zips** with device code for sm_86, sm_89 (the 4070 Ti SUPER) and sm_120, and builds with MSVC. I/O is documented as WAV in and WAV out (`--audio`, `--tts-out`) plus `--say`. Multi-turn use goes through `--serve`, which takes one JSON object per line. **Live microphone capture is not documented.** — [llama-voicechat.cpp](https://github.com/sansamour/llama-voicechat.cpp)
- The fork has **no chat template or system prompt** ("conversation is the continuing timeline"). It answers in English only. Without `VC_NO_BARGE=1` and `VC_FORCE_BOS=1`, the model barges in about a second into a clip and later turns degrade. No tested GPUs, VRAM or latency figures are given. The repo has 8 stars and 3 forks — [llama-voicechat.cpp](https://github.com/sansamour/llama-voicechat.cpp)
- One aggregator says the model is on Ollama and that a desktop GPU "should be enough". This contradicts the official 80 GB requirement and is not corroborated — [aimarketcap](https://aimarketcap.tech/models/nvidia-nvidia-nemotronlabs-voicechat-11b) (from search summary)
- A search for Reddit and community reports of running it on 16 GB or 24 GB consumer cards found none — [search, Oct 2026](https://github.com/Nikki1404/nemotron_voicechat_11B)

### Inferences
- The user's "Q4_K_M fits in 16 GB" is directionally right about size, since Q4_0 is about 6.6 GB and Q8_0 about 12 GB. But the actual quant is Q4_0, the runtime is a tiny unofficial fork, and the 16 GB fit is unmeasured. With Whisper (2-3 GB) loaded too, Q4_0 is the safe choice and Q8_0 would be tight.
- On Windows, the only practical path is llama-voicechat.cpp, which runs natively and so needs no WSL2. The official NeMo/vLLM path is Linux-only and sized for 80 GB, so WSL2 plus the official stack is not realistic on 16 GB.
- For role-play, the community runtime's lack of a system prompt is a blocker for personas like "professor" or "doctor". The official stack supports system prompts but needs 80 GB. Its known instruction-following weaknesses and the reported short context also make it a weak sustained conversation partner.

### Gaps
- No measured VRAM, latency or quality of the GGUF on any consumer GPU. No WSL2 reports.
- Whether llama-voicechat.cpp can do live full-duplex mic streaming, or only WAV turns. Whether persona text can be injected as a prefix on the timeline.
- The exact context length (the two-minute claim is unverified). Voice options are undocumented.
- No vLLM or NeMo quantized (FP8/INT4) path for consumer cards was found.

## Q2. Moshi family: Kyutai Moshi, NVIDIA PersonaPlex 7B, moshi.cpp

### Takeaway
NVIDIA PersonaPlex-7B (January 2026, built on Moshi) is the strongest fit for persona role-play that is actually proven on consumer hardware. It is true full-duplex, a text prompt sets role, background and scenario, and an audio prompt sets the voice. It runs natively on Windows via moshi.cpp with q8_0 or q4_k, and bf16 uses about 19 GB on an RTX 3090. Its weaknesses: English only, a roughly 4-minute context window after which the persona drifts and the model loops, and Moshi-class shallow intelligence.

### Cited Findings
- PersonaPlex-7B-v1 was released on January 15, 2026. It is built on Moshi (moshiko weights) with the Mimi encoder and decoder plus Moshi's temporal and depth transformers. It is listed as 7B, though the safetensors metadata shows 8B. It was tested on an A100 80 GB under PyTorch on Linux via the `moshi` package, with a web UI on port 8998 — [HF personaplex-7b-v1](https://huggingface.co/nvidia/personaplex-7b-v1)
- Persona control: a text prompt sets role, background and scenario, and a voice prompt (audio tokens) sets voice and style. It outputs agent text alongside speech. It is English only — [HF personaplex-7b-v1](https://huggingface.co/nvidia/personaplex-7b-v1)
- Full-Duplex-Bench results: smooth turn-taking TOR 0.908, user interruption TOR 0.950, GPT-4o rating 4.290, latency 0.170 (turn-taking) and 0.240 (interruption), and pause handling TOR 0.358 (synthetic) and 0.431 (Candor). That is better than VoiceChat 11B on turn-taking, but VoiceChat is better on pause handling (0.153 and 0.255) — [HF personaplex-7b-v1](https://huggingface.co/nvidia/personaplex-7b-v1), [VoiceChat card](https://huggingface.co/nvidia/NVIDIA-NemotronLabs-VoiceChat-11B)
- Licence: NVIDIA Open Model License, "ready for commercial use", gated (you must share contact info) — [HF personaplex-7b-v1](https://huggingface.co/nvidia/personaplex-7b-v1). Another source gives weights under the NVIDIA Open Model License and code under MIT — [Digital Applied, Oct 2 2026](https://www.digitalapplied.com/blog/full-duplex-voice-models-compared-2026). **Conflict:** one guide calls it simply "MIT licensed" — [Menon Lab](https://themenonlab.blog/blog/nvidia-personaplex-full-duplex-voice-ai-how-to-guide) (search snippet)
- Hands-on report (MakeUseOf, July 21, 2026): bf16 used about 19 GB VRAM on an RTX 3090, in real time. The context is 3,000 frames, about 240 s. After about 4 minutes the persona forgot its name ("Alex" became "Kim"), then repeated phrases and looped, and the server has no effective repetition penalty. Custom voices are only approximate. The author called the quality initially "astonishing" and responses "way faster" than GPT-Live — [MakeUseOf](https://www.makeuseof.com/nvidia-personaplex-local-speech-model-8gb-vram/)
- VRAM claims conflict: "minimum 16 GB, 24 GB+ recommended" for PersonaPlex — [Menon Lab guide](https://themenonlab.blog/blog/nvidia-personaplex-full-duplex-voice-ai-how-to-guide) (search snippet); about 19 GB measured at bf16 — [MakeUseOf](https://www.makeuseof.com/nvidia-personaplex-local-speech-model-8gb-vram/); 8-16 GB with community 8-bit or 4-bit (moshi.cpp, ComfyUI_PersonaPlexMF) — [MakeUseOf](https://www.makeuseof.com/nvidia-personaplex-local-speech-model-8gb-vram/)
- moshi.cpp is a C++/ggml port of Moshi (moshika and moshiko) that also supports PersonaPlex and Kyutai TTS (1.6B, 0.75B) and STT (1B, 2.6B). Quants are `q8_0` and `q4_k`. It has **Windows binary releases with benchmarks on Win10 and Win11** and supports CUDA and Vulkan. Moshi at q4_k "can run on 8gb of vram" but "may not perform fast enough". The sts q4_k benchmark uses under 8 GB. Real time needs at least 12.5 fps — [moshi.cpp](https://github.com/Codes4Fun/moshi.cpp)
- moshi.cpp PersonaPlex takes a system prompt (`-p`) and a voice (`-v`): a built-in NVIDIA voice such as NATF0, or a WAV file for cloning. It is a "partial port", and Win11 may slow with large contexts (the suggested workaround is `-c 1000`) — [moshi.cpp](https://github.com/Codes4Fun/moshi.cpp)
- Moshi has about 160 ms theoretical latency and as low as 200 ms in practice on an L4. Weights are CC-BY 4.0 and code is MIT/Apache — [Digital Applied](https://www.digitalapplied.com/blog/full-duplex-voice-models-compared-2026). Moshi bf16 is listed at about 16-20 GB — [Spheron](https://www.spheron.network/blog/speech-to-speech-gpu-cloud-moshi-sesame-csm-hertz-dev/) (vendor blog, search snippet)
- Moshi is described as English-only. Language adaptations exist for Japanese (J-Moshi, LLM-jp-Moshi), but no German version was found. The M3-DuplexBench paper (arXiv 2607.29125) evaluated only English and Japanese open full-duplex models, all Moshi-based — [M3-DuplexBench](https://arxiv.org/pdf/2607.29125), [Japanese full-duplex paper](https://arxiv.org/pdf/2506.02979) (via search summary)
- Kyutai Unmute is a cascade, so it is outside this note's scope, and was not researched further.

### Inferences
- PersonaPlex at q8_0 (an inference: about 8-9 GB for 7-8B parameters) or q4_k via moshi.cpp on Windows should leave room for Whisper on 16 GB. The 4070 Ti SUPER (sm_89) is faster than the RTX 2070, 3060 and 4060 cards in moshi.cpp's benchmarks, so real time is plausible. No 4070 Ti SUPER number was found.
- The 4-minute context limit means role-play sessions would need short scenes, or resets that re-inject the persona prompt.
- Transcripts: the agent side comes from PersonaPlex's text stream. The user side would need the app's own Whisper, since PersonaPlex does not emit a user transcript (unlike VoiceChat 11B).

### Gaps
- No measured PersonaPlex q8_0 or q4_k VRAM or fps on a 16 GB Ada card. No WSL2-specific reports (moshi.cpp makes WSL2 unnecessary).
- Hertz-dev VRAM (8-12 GB) is only a parameter-count estimate — [Spheron](https://www.spheron.network/blog/speech-to-speech-gpu-cloud-moshi-sesame-csm-hertz-dev/). Not investigated further.

## Q3. Omni models: Qwen2.5-Omni, Qwen3-Omni, Qwen3.5-Omni, MiniCPM-o 4.5

### Takeaway
These models are much smarter than Moshi-class models, because they are built on real LLMs (Qwen3-8B, Qwen2.5-7B, a 30B MoE), and they accept system prompts. But they are turn-based, except MiniCPM-o 4.5, which offers full-duplex. **MiniCPM-o 4.5 (9B, Apache-2.0) is the best omni candidate for 16 GB:** int4 needs about 11 GB, llama.cpp-omni about 12 GB, and it supports voice cloning and role-play via reference audio. Speech is English and Chinese only. Qwen3-Omni has German speech output but needs about 79 GB at bf16, and no quantized build with speech was found. Qwen2.5-Omni-7B GPTQ-Int4 fits (about 11.6 GB).

### Cited Findings
**MiniCPM-o 4.5 (OpenBMB)**
- 9B total, built end to end on SigLip2, Whisper-medium, CosyVoice2 and **Qwen3-8B**. It supports full-duplex streaming with proactive 1 Hz speak-or-not decisions, though barge-in is not explicitly described — [HF MiniCPM-o-4_5](https://huggingface.co/openbmb/MiniCPM-o-4_5)
- VRAM is 19.0 GB at bf16 and **11.0 GB at int4**. The PyTorch demo needs at least 28 GB. **llama.cpp-omni needs about 12 GB on low-resource NVIDIA GPUs**. Full-duplex on a Mac needs an M4 Max with 24 GB. It ships int4 and GGUF quants in 16 sizes and runs on llama.cpp, Ollama, vLLM and SGLang — [HF MiniCPM-o-4_5](https://huggingface.co/openbmb/MiniCPM-o-4_5)
- Time to first token is 0.6 s (bf16 and int4). Decoding runs at 154 tok/s (bf16) and 212 tok/s (int4). End-to-end speech latency is not given — [HF MiniCPM-o-4_5](https://huggingface.co/openbmb/MiniCPM-o-4_5)
- VoiceBench AlpacaEval is about 4.8. Speech TriviaQA is 75.5 and Speech WebQuestions 70.2. Seed-TTS test-en WER is 2.38%. Table column alignment was uncertain in the fetch — [HF MiniCPM-o-4_5](https://huggingface.co/openbmb/MiniCPM-o-4_5)
- Real-time speech conversation is bilingual (English and Chinese); German speech is not mentioned. Voice cloning and role-play work "via a simple reference audio clip", with text and audio system prompts. Text is output alongside audio. Licence: Apache-2.0. Windows is not mentioned, and install docs cover macOS and Ubuntu — [HF MiniCPM-o-4_5](https://huggingface.co/openbmb/MiniCPM-o-4_5)
- The realtime web demo was open-sourced on 2026-02-06 — [HF MiniCPM-o-4_5](https://huggingface.co/openbmb/MiniCPM-o-4_5)

**Qwen3-Omni-30B-A3B**
- Variants are Instruct (thinker plus talker, outputs audio), Thinking (text out only) and Captioner. Flash is API-only. It is a 30B MoE — [QwenLM/Qwen3-Omni](https://github.com/QwenLM/Qwen3-Omni)
- Minimum GPU memory for Instruct at bf16 is **78.85 GB** (15 s video), and Thinking needs 68.74 GB — [QwenLM/Qwen3-Omni](https://github.com/QwenLM/Qwen3-Omni)
- `disable_talker()` saves about 10 GB, but then audio output is impossible — [QwenLM/Qwen3-Omni](https://github.com/QwenLM/Qwen3-Omni) (via search summary)
- It understands speech in 19 languages (German included) and **outputs speech in 10, including English and German** (plus Russian). Voices are Ethan, Chelsie and Aiden. A system prompt is supported. Licence: Apache-2.0. "vLLM serve for Qwen3-Omni currently only supports the thinker model" (text only). The repo mentions no AWQ, GPTQ or GGUF builds and does not claim full-duplex — [QwenLM/Qwen3-Omni](https://github.com/QwenLM/Qwen3-Omni)
- Qwen3.5-Omni (newer, 30B total / 3B active MoE) needs about 64 GB at FP16, and FP8 fits on a single 80 GB GPU — [Spheron](https://www.spheron.network/blog/deploy-qwen3-5-omni-gpu-cloud/) (vendor blog, search summary). That is out of reach for 16 GB.
- No source confirmed that a quantized Qwen3-Omni with the talker runs on 16 GB — [search, Oct 2026](https://github.com/QwenLM/Qwen3-Omni)

**Qwen2.5-Omni-7B**
- 31.11 GB at bf16 and **11.64 GB with GPTQ-Int4**, both for 15 s video input, so audio-only needs less — [HF Qwen2.5-Omni-7B](https://huggingface.co/Qwen/Qwen2.5-Omni-7B) (via search summary)
- VoiceBench: AlpacaEval 4.33, CommonEval 3.84, SD-QA 57.41, MMSU 56.38 — [Kimi-Audio tech report](https://arxiv.org/html/2504.18425v1)
- A 3B variant exists, with a tutorial listing 16 GB as the minimum — [NodeShift/dev.to](https://dev.to/nodeshiftcloud/how-to-install-qwen25-omni-3b-locally-1g6k) (search summary)

### Inferences
- For a "smart" partner (a professor who can discuss content, or a mechanic explaining a fault), MiniCPM-o 4.5 at int4 or GGUF is the only omni model found that is both full-duplex-capable and documented at about 11-12 GB. With Whisper loaded, it would be near the 16 GB limit. Its own Whisper-medium encoder plus text output might make a separate Whisper unnecessary for the agent side.
- Qwen3-Omni is the only open model found with German speech output, but it does not fit 16 GB with speech on.
- Qwen2.5-Omni-7B GPTQ-Int4 fits, but it is turn-based and superseded by Qwen3-Omni and MiniCPM-o 4.5.

### Gaps
- MiniCPM-o 4.5 on Windows or WSL2 (llama.cpp-omni Windows builds?), measured full-duplex VRAM on a 16 GB NVIDIA card, and how good its English role-play is in practice.
- Qwen2.5-Omni's speech output languages were not verified in this session.
- The release date and newer version numbers of MiniCPM-o (4.x after 4.5?) were not found.
- No community Qwen3-Omni AWQ, GGUF or talker quant was verified.

## Q4. Other end-to-end models (Kimi-Audio, Step-Audio 2, GLM-4-Voice, LLaMA-Omni 2, etc.)

### Takeaway
Kimi-Audio needs about 23 GB at bf16, so it does not fit natively. Step-Audio 2 mini is strong on paralinguistics and audio understanding, but no VRAM figure was found (it is 7B-class). The remaining older models (GLM-4-Voice, Freeze-Omni, LLaMA-Omni 2, Mini-Omni, VITA-Audio, Baichuan-Omni) were not verified this session. They are 2024 to mid-2025 releases and are likely superseded by MiniCPM-o 4.5 and PersonaPlex.

### Cited Findings
- Kimi-Audio needs about 23 GB of GPU VRAM at bf16 per its official card — [MoonshotAI/Kimi-Audio](https://github.com/MoonshotAI/Kimi-Audio) (via search summary). Its VoiceBench scores are AlpacaEval 4.46, CommonEval 3.97, SD-QA 63.12 and MMSU 62.17, above Qwen2.5-Omni — [Kimi-Audio tech report](https://arxiv.org/html/2504.18425v1)
- Step-Audio 2 mini (initialized from Qwen2.5-7B) scores 80.00 on StepEval paralinguistic, against 49.64 for Kimi-Audio and 44.18 for Qwen-Omni. On MMAU it scores 73.2, against 69.6 and 71.5. These are self-reported — [HF Step-Audio-2-mini](https://huggingface.co/stepfun-ai/Step-Audio-2-mini), [Step-Audio 2 tech report](https://arxiv.org/pdf/2507.16632)
- The same Kimi-Audio VoiceBench table includes Step-Audio, GLM-4-Voice, Baichuan-Audio and Qwen2-Audio as comparators, so these are mid-2025-era baselines — [Kimi-Audio tech report](https://arxiv.org/html/2504.18425v1)
- Ultravox (Inworld) is not full-duplex and was excluded from the Oct 2026 full-duplex comparison — [Digital Applied](https://www.digitalapplied.com/blog/full-duplex-voice-models-compared-2026)

### Inferences
- Kimi-Audio and Step-Audio 2 mini would need 8-bit or 4-bit quantization to fit next to Whisper. No quantized speech-output build was confirmed.

### Gaps
- No verified data this session for GLM-4-Voice, Freeze-Omni, LLaMA-Omni 2, Mini-Omni, VITA-Audio, Baichuan-Omni, Sesame CSM (essentially a conversational TTS, not a full S2S agent) or Ultravox VRAM. Report them as unverified or older rather than recommending them.
- No Step-Audio 2 mini VRAM, persona, language or Windows data. No Kimi-Audio quant reports.

## Q5. German support and cross-cutting comparison

### Takeaway
No open-weight end-to-end model with German speech output was found that fits on 16 GB. Qwen3-Omni outputs German but needs about 79 GB at bf16. Every full-duplex model that fits (PersonaPlex/Moshi, VoiceChat 11B GGUF, MiniCPM-o 4.5) is English, or English and Chinese. German for this app likely means a cascade (Whisper, an LLM, then German TTS such as Voxtral TTS).

### Cited Findings
- No 2026 open-weight, German-capable, full-duplex S2S model was found. Moshi derivatives exist only for English and Japanese — [M3-DuplexBench](https://arxiv.org/pdf/2607.29125)
- Voxtral TTS (Mistral, March 26 2026) is open-weight TTS in 9 languages, including German — [SiliconANGLE](https://siliconangle.com/2026/03/26/mistral-releases-open-weights-speaking-ai-model-voxtral-tts/), [TechCrunch](https://techcrunch.com/2026/03/26/mistral-releases-a-new-open-source-model-for-speech-generation/)
- The Hugging Face speech-to-speech repo is a cascaded local voice agent that can force German synthesis — [huggingface/speech-to-speech](https://github.com/huggingface/speech-to-speech)
- Qwen3-Omni speech output covers English, German and Russian among its 10 languages — [QwenLM/Qwen3-Omni](https://github.com/QwenLM/Qwen3-Omni)

### Inferences
Summary comparison for 16 GB, Windows 10, English role-play. All entries come from the cited findings above; "?" means unverified.

| Model | Fits 16 GB (+Whisper)? | Windows path | Duplex | Persona prompt | Langs (speech) | Both-side transcript | Licence | Smartness |
|---|---|---|---|---|---|---|---|---|
| PersonaPlex 7B (moshi.cpp q8_0/q4_k) | Likely yes (q4_k <8 GB measured for Moshi STS) | Native moshi.cpp binaries | Full | Yes (text + voice WAV) | EN | Agent text only | NVIDIA Open Model Lic., commercial OK | Low-moderate. Drifts and loops after about 4 min |
| VoiceChat 11B (community GGUF Q4_0/Q8_0) | Q4_0 likely (6.6 GB file); Q8_0 tight | Native llama-voicechat.cpp (Win CUDA 12.8) | Full, but the fork is WAV turn I/O | Official yes; **fork: no** | EN | Official: yes (user ASR + agent text) | OpenMDW 1.1, "research only" | Moderate. Known loops and weak instruction following |
| MiniCPM-o 4.5 (int4/GGUF) | Borderline (11-12 GB + Whisper) | ? (llama.cpp-omni; Windows untested) | Full | Yes (text + audio ref, cloning) | EN, ZH | Yes, text out (and ASR) | Apache-2.0 | Higher (Qwen3-8B backbone) |
| Qwen2.5-Omni-7B GPTQ-Int4 | Yes (about 11.6 GB at video load) | ? | Turn-based | Yes | ? | Text out | ? | Moderate. Superseded |
| Qwen3-Omni 30B-A3B | No (about 79 GB bf16) | n/a | Turn-based | Yes | 10 incl. EN, DE | Text out | Apache-2.0 | Highest of the group |
| Kimi-Audio | No at bf16 (about 23 GB) | ? | Turn-based? | ? | ? | ? | ? | VoiceBench above Qwen2.5-Omni |

- For an English speaking-practice role-play app on a 4070 Ti SUPER, the best end-to-end candidates are PersonaPlex via moshi.cpp, for natural full-duplex persona chats in short scenes, and MiniCPM-o 4.5, for smarter content at the cost of a tighter VRAM fit and less certainty on Windows. VoiceChat 11B is the newest, but on consumer hardware it is reachable only through an unbenchmarked, no-persona community fork.

### Gaps
- No German-capable end-to-end model fits 16 GB. Whether any MiniCPM-o or Qwen-Omni quant speaks German acceptably on 16 GB is unknown.
- No independent, head-to-head "conversation partner quality" evaluation covers PersonaPlex, VoiceChat and MiniCPM-o together.
