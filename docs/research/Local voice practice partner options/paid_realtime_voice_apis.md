# Paid cloud realtime voice-conversation APIs for a personal speaking-practice partner (as of 2026-10-16)

Scope: speech-to-speech (S2S) and cascade APIs a Tauri 2 + Rust + Next.js Windows app could call for English (later German) role-play practice. All prices USD. "Read" dates are when this researcher or the cited source read the page. Third-party figures are flagged as such; vendors change voice pricing often (xAI and Deepgram both changed it in Aug–Sep 2026).

Token-to-minute conversion used in inferences (not re-verified from an OpenAI page this session; it matches Micdrop's numbers below): OpenAI bills user audio at about 600 tokens/min and assistant audio at about 1,200 tokens/min. A "naive" per-minute price assumes a 50/50 user/assistant talk split and ignores the conversation context being re-billed as (cached) input on every turn.

## 1. Price per minute, 30-minute session and ~10 h/month

### Takeaway
The cheapest high-quality native S2S options are Google Gemini 3.8 Live (about $0.012/min naive; a free tier exists) and OpenAI gpt-realtime-2.1-mini (about $0.015/min naive). The flagship OpenAI models (gpt-realtime-2.1, about $0.05–0.11/min) and the managed agent platforms (ElevenLabs, Deepgram, Cartesia, Hume, Grok, about $0.04–0.08/min plus LLM) cost 3–8x more. At 10 h/month that is roughly $7–10 versus $30–66. A DIY cascade (Groq Whisper + cheap LLM + cheap TTS) costs about $0.01–0.03/min.

### Cited Findings

**OpenAI (official pricing page, read 2026-10-16)**
- Per 1M tokens: gpt-realtime, gpt-realtime-1.5, gpt-realtime-2 and gpt-realtime-2.1 all cost $32 audio in, $0.40 cached audio in and $64 audio out. Text costs $4 in and $16 out (gpt-realtime and gpt-realtime-1.5) or $24 out (gpt-realtime-2 and gpt-realtime-2.1). — [OpenAI pricing](https://developers.openai.com/api/docs/pricing)
- gpt-realtime-mini and gpt-realtime-2.1-mini: $10 audio in, $0.30 cached, $20 audio out; text $0.60 in and $2.40 out per 1M. — [OpenAI pricing](https://developers.openai.com/api/docs/pricing)
- gpt-live-1: $0.05 per minute of session, billed per second. "Backend model and tool usage are billed separately." — [OpenAI pricing](https://developers.openai.com/api/docs/pricing)
- Other per-minute models: gpt-realtime-whisper $0.017/min, gpt-realtime-translate $0.034/min, gpt-live-transcribe $0.017/min, gpt-transcribe $0.0045/min, gpt-4o-transcribe $0.006/min, gpt-4o-mini-transcribe $0.003/min, whisper $0.006/min. gpt-4o-mini-tts costs $12 per 1M audio output tokens plus $0.60 per 1M text input tokens. — [OpenAI pricing](https://developers.openai.com/api/docs/pricing)
- Micdrop (published 2026-09-18, updated 2026-10-15, prices read 2026-09-17) converts these to naive per-minute prices at a 50/50 split: GPT-Realtime-2.1 $0.048/min, GPT-Realtime-2.1-mini $0.015/min, GPT-Live-1 $0.05/min plus backend. It says GPT-Live-1 mini exists only in ChatGPT, not in the API. — [Micdrop](https://micdrop.dev/blog/gpt-live-vs-gpt-realtime-vs-gemini-live)
- Forasoft (third party) estimates a realistic flagship budget of $0.06–$0.11/min with caching, and up to $0.46/min uncached, because context is re-billed every turn. — [Forasoft](https://www.forasoft.com/article/openai-realtime-api-pricing)
- Artificial Analysis lists GPT-Realtime-2, GPT-Realtime-1.5 and GPT Realtime (Aug '25) at $1.15/hr input and $4.61/hr output audio, and GPT Realtime Mini (Oct '25) at $0.36/hr input and $1.44/hr output. — [Artificial Analysis S2S](https://artificialanalysis.ai/speech-to-speech)
- OpenAI announced GPT-Live-1 in the API on 2026-09-10. — [OpenAI dev community](https://community.openai.com/t/introducing-gpt-live-1-in-the-api/1396471); [unite.ai](https://www.unite.ai/openais-gpt-live-1-arrives-in-the-api-at-0-05-per-minute/)

**Google Gemini Live (official pricing page, last updated 2026-10-15)**
- `gemini-3.8-live`, `gemini-3.8-live-extended-thinking` and `gemini-3.1-flash-live-preview` cost, per 1M tokens: audio in $3.00 ("or $0.005/min"), text in $0.75, audio out $12.00 ("or $0.018/min"), text out $4.50. — [Gemini API pricing](https://ai.google.dev/gemini-api/docs/pricing)
- `gemini-2.5-flash-native-audio-preview-12-2025` costs $3 audio in, $0.50 text in, $12 audio out and $2 text out. — [Gemini API pricing](https://ai.google.dev/gemini-api/docs/pricing)
- **Free tier:** "Input and output are free of charge" for the 3.8 Live, 3.1 Flash Live Preview and 2.5 Flash Native Audio models. — [Gemini API pricing](https://ai.google.dev/gemini-api/docs/pricing)
- Micdrop: Gemini 3.8 Live costs about $0.012/min ($0.005/min heard, $0.018/min spoken), and Extended Thinking has the same rate. — [Micdrop](https://micdrop.dev/blog/gpt-live-vs-gpt-realtime-vs-gemini-live)

**xAI Grok Voice**
- Artificial Analysis lists Grok Voice Fast 1.0 at $3.00/hr in and $3.00/hr out, which is about $0.05/min. — [Artificial Analysis S2S](https://artificialanalysis.ai/speech-to-speech)
- A third-party source (dated 2026-08-04) says `grok-voice-think-fast-2.0` costs $0.08/min ($4.80/hr) and became the default alias on 2026-08-05, up from $0.05/min for think-fast-1.0. Pinning the old model keeps the old rate. xAI's own pricing page was not checked. — [eesel.ai](https://www.eesel.ai/blog/grok-voice-think-fast-2-pricing)

**Amazon Nova 2 Sonic**
- Artificial Analysis lists "Nova 2.0 Sonic (Mar 2026)" at $0.27/hr in and $1.08/hr out, which is about $0.011/min at 50/50. — [Artificial Analysis S2S](https://artificialanalysis.ai/speech-to-speech)
- Third-party blogs say $0.015–$0.017/min, and one lists $3/$12 per 1M tokens. The official Bedrock page was not checked. — [techplained](https://www.techplained.com/amazon-nova-2); [burki.dev](https://burki.dev/blog/11-voice-ai-pricing-comparison-2026)

**Alibaba Qwen (newer entrants on the leaderboard)**
- Qwen Audio 3.0 Realtime Plus costs $0.03/hr in and $0.18/hr out. Qwen3.5 Omni Flash Realtime costs $0.16/hr in and $0.82/hr out, with a time to first audio (TTFA) of 0.79 s. — [Artificial Analysis S2S](https://artificialanalysis.ai/speech-to-speech)

**Managed agent platforms (STT + LLM + TTS orchestrated for you)**
- **ElevenLabs Agents:** $0.08 per call minute on every plan (Starter $6 to Business $990), plus LLM cost passed through per minute ($0.0005/min on GPT-5 Nano up to $0.0446/min on GPT-5.5). Burst minutes cost $0.16/min. This comes from a September 2026 guide. Older sources say $0.10/min. — [Macha](https://www.getmacha.com/blog/elevenlabs-agents-pricing-explained); [Quiq](https://quiq.com/blog/elevenlabs-pricing/); older figure: [SIMBA](https://simbavoice.ai/resources/simba-vs-elevenlabs-pricing)
- **Deepgram Voice Agent API:** Standard costs $0.075/min. It was $0.056/min (promo) until about 2026-09-12. Advanced costs $0.163/min, and bring-your-own-LLM/TTS variants about $0.050–0.122/min. Sources disagree whether custom BYO-LLM is $0.059 or $0.065. — [Cekura](https://www.cekura.ai/blogs/deepgram-pricing); [Cartesia (competitor)](https://www.cartesia.ai/learn/deepgram-pricing)
- **Cartesia agents:** $0.06/min. — [eesel.ai](https://www.eesel.ai/blog/cartesia-sonic-3-pricing); [Cartesia pricing](https://www.cartesia.ai/pricing)
- **Hume EVI:** subscription tiers from $0 to $500/month with included EVI minutes. Overage is $0.06/min on Pro and $0.04/min on Business. The pricing page names EVI 3 and EVI 4 mini, with no separate full EVI 4 rate found. A review says it verified these figures on 2026-07-04. — [Hume pricing](https://www.hume.ai/pricing); [dupple review](https://dupple.com/reviews/hume)
- **Ultravox:** only a third-party calculator figure, $0.05/min bundled. No official page was found. — [softcery calculator](https://softcery.com/ai-voice-agents-calculator)
- **Azure Voice Live:** billed per 1M tokens in tiers by LLM. The Pro tier includes gpt-realtime, gpt-realtime-2 and gpt-realtime-2.1. The dollar amounts did not render on the official page, so no current numbers. — [Azure Speech pricing](https://azure.microsoft.com/en-us/pricing/details/speech/)

**Cascade building blocks**
- Groq whisper-large-v3-turbo costs about $0.04 per audio hour, and whisper-large-v3 $0.111/hr. These are aggregator figures (Oct 2026). — [morphllm](https://www.morphllm.com/groq-pricing); [computeprices](https://computeprices.com/providers/groq/models/whisper-v3-turbo)
- ElevenLabs Flash/Turbo TTS costs $0.05 per 1k characters (one outlier source says $0.04), and Multilingual v2/v3 $0.10 per 1k characters. — [activepieces](https://www.activepieces.com/blog/elevenlabs-pricing-2026-plans-credits-and-costs.md); [techjacksolutions](https://techjacksolutions.com/ai-tools/elevenlabs/elevenlabs-pricing/)
- Cartesia TTS is about 1 credit per character per its docs. Plans: Pro $4/mo for 100k credits, Startup $39 for 1.25M, Scale $239 for 8M. A calculator says Sonic-3.5 is billed per second instead, which conflicts with the docs. — [Cartesia docs](https://docs.cartesia.ai/pricing.md); [usagepricing](https://usagepricing.com/tools/pricing-calculator/cartesia)
- xAI TTS: $15 per 1M characters (third party). — [CometAPI](https://www.cometapi.com/grok-api-pricing-cost-guide/)

### Inferences
Assumptions: 30-minute session = 30 min; 10 h/month = 600 min; naive 50/50 split. "Realistic" adds context re-billing.

| Option | Naive $/min | 30-min session | 10 h/month | Notes |
|---|---|---|---|---|
| Gemini 3.8 Live | ~0.012 | ~$0.35 | ~$7 | Free tier exists; context re-billing per turn not checked (see Gaps) |
| Nova 2 Sonic | ~0.011–0.017 | ~$0.35–0.50 | ~$7–10 | AWS SigV4/Bedrock bidirectional stream; heavier to integrate |
| gpt-realtime-2.1-mini | ~0.015 | ~$0.45 (+ ~$0.3–0.5 context) | ~$9–20 | |
| gpt-realtime-2.1 | ~0.048 | ~$1.45, realistic ~$2–3.3 | ~$29, realistic ~$36–66 | Cached context at $0.40/M keeps growth modest |
| gpt-live-1 | 0.05 + backend LLM | ~$1.50 + LLM | ~$30 + LLM | Best conversational quality scores |
| Grok Voice | 0.05–0.08 | $1.50–2.40 | $30–48 | Prices from third parties |
| Hume EVI | 0.04–0.06 overage + plan | ~$1.2–1.8 | ~$24–36 + plan | |
| Ultravox / Cartesia agents | ~0.05–0.06 | ~$1.5–1.8 | ~$30–36 | |
| Deepgram Voice Agent Std | 0.075 | ~$2.25 | ~$45 | |
| ElevenLabs Agents | 0.08 + LLM | ~$2.4+ | ~$48+ | Best voice catalogue |
| DIY cascade (Groq Whisper turbo + small LLM + ElevenLabs Flash or Cartesia) | ~0.01–0.03 | ~$0.3–0.9 | ~$6–18 | TTS dominates: ~450 assistant chars/min x $0.05/1k = ~$0.022/min |

- Context re-billing estimate for gpt-realtime-2.1 over 30 min: the context grows to about 27k audio tokens. At about 4 turns/min, that is about 1.6M cached tokens x $0.40/M ≈ $0.65 per session. This is a rough model by this researcher, consistent with Forasoft's $0.06–0.11/min range.
- For a single user at 10 h/month, every option except ElevenLabs, Deepgram Advanced or an uncached flagship stays under about $50/month. Cost differences are small in absolute terms, so quality and transcript faithfulness should decide.

### Gaps
- The xAI, Nova Sonic, Ultravox, Hume (EVI 4) and Azure Voice Live prices come from third parties or rendered without numbers. The official pages were not verified.
- Not confirmed: whether the Gemini Live API re-bills the accumulated context window as input tokens every turn. That affects long-session cost, so check the Gemini Live pricing/billing docs.
- Not confirmed: the Gemini free tier's data-use terms. On Google free tiers, data may be used to improve products, which matters for privacy.
- Not researched for lack of tool budget: Sesame (no public API found in results), Kyutai (Moshi appears on Artificial Analysis with no price or score; no paid API found), Speechmatics Flow, Retell and Vapi pricing, and Deepgram Aura-2 TTS price.

## 2. Quality: latency, full-duplex/barge-in, voice naturalness, reviews

### Takeaway
On the Artificial Analysis S2S index (Oct 2026), xAI Grok Voice 3.0 (86.9), OpenAI GPT-Live-1 (80–83) and Google Gemini 3.8 Live Extended Thinking (82.6) lead. Time to first audio is about 0.8–1.4 s for all the major ones. GPT-Live-1 is OpenAI's new full-duplex "listen while speaking" layer, and Gemini is the strongest price/quality option. Learner-specific reviews are essentially absent.

### Cited Findings
- Artificial Analysis S2S table (read 2026-10-16), index / TTFA:
  - xAI: Grok Voice 3.0 86.9 / 0.81 s; Grok Voice 2.0 81.3 / 0.70 s; Grok Voice 1.0 73.7 / 1.25 s.
  - OpenAI: GPT-Live-1 (Astra, medium) 83.2 / 1.34 s; GPT-Live-1 (Sol, low) 80.1 / 1.24 s; GPT-Realtime-2.1 High 75.2 / 1.21 s; GPT-Realtime-2.1 Minimal 70.7 / 0.97 s; GPT-Realtime-2 (High) 73.6 / 1.14 s; GPT Realtime Mini (Oct '25) 56.1 / 0.81 s; GPT-Realtime-2.1 Mini Minimal 54.4 / 0.85 s.
  - Google: Gemini 3.8 Live Extended Thinking (High) 82.6 / 1.35 s; Gemini 3.8 Live 76.0 / 1.18 s; Gemini 3.1 Flash Live High 72.0 / 2.99 s.
  - Others: Amazon Nova 2.0 Sonic TTFA 1.14 s (no index); Qwen Audio 3.0 Realtime Plus 69.5 / 1.54 s.
  - The index is equal-weighted across speech reasoning, agentic performance, arena preference and task success. — [Artificial Analysis S2S](https://artificialanalysis.ai/speech-to-speech)
- Conflicting snapshot: the search summary of the same page gave the index as a weighted average with Gemini 3.8 Live Extended Thinking at the top (82.6). The fetched table shows Grok Voice 3.0 (86.9) and GPT-Live-1 Astra (83.2) higher. A June 2026 announcement had GPT-Realtime-2 (High) leading at 77.2%. The rankings move monthly. — [Artificial Analysis index announcement](https://artificialanalysis.ai/articles/announcing-the-artificial-analysis-speech-to-speech-index)
- Speech Agent Arena (Elo; snapshot undated): Gemini 3.1 Flash Live Minimal 1096, Gemini 3.8 Live 1083, GPT-Live-1 (Sol, low) 1053 (4th). — [AA Arena](https://artificialanalysis.ai/speech-to-speech/arena)
- GPT-Live-1 handles live turn-taking and hands deep reasoning to a backend model the developer chooses. Paired with GPT-6 Astra (medium), it completed 83.6% of Tau3 tasks versus 45.7% for GPT-Realtime-2.1. OpenAI's tweet: voice agents that "listen while they speak". — [unite.ai](https://www.unite.ai/openais-gpt-live-1-arrives-in-the-api-at-0-05-per-minute/); [OpenAI Devs on X](https://x.com/OpenAIDevs/status/2098099269551149398)
- GPT-Live-1 turn-taking latency is about 0.798 s (TechRepublic, read from OpenAI's chart). OpenAI says it scores 30 points higher than GPT-Realtime-2.1 on Full Duplex Bench. GPT-Realtime-2 TTFA is 1.12 s at minimal and 2.33 s at high reasoning. — [Micdrop](https://micdrop.dev/blog/gpt-live-vs-gpt-realtime-vs-gemini-live)
- Session limits:
  - GPT-Realtime-2.1: 60 min.
  - Gemini Live: about 10 min per connection, and 15 min for audio-only sessions without compression. `contextWindowCompression` "can extend sessions to an unlimited amount of time". Resumption tokens are valid for 2 h.
  - GPT-Live-1: capped at an unpublished length.
  — [Micdrop](https://micdrop.dev/blog/gpt-live-vs-gpt-realtime-vs-gemini-live); [Gemini session management](https://ai.google.dev/gemini-api/docs/live-api/session-management)
- Naturalness opinions (blogs, not learner tests):
  - andrew.ooo (May 2026) rates GPT-Realtime-2's voice "Excellent" and Gemini Live's "Very good".
  - Mindstudio says Gemini's multilingual audio sounds more natural in non-English languages.
  - Tom's Guide found Gemini Live more natural and energetic in a casual persona role-play, with ChatGPT stronger on facts (a consumer-app test).
  — [andrew.ooo](https://andrew.ooo/answers/gpt-realtime-2-vs-elevenlabs-vs-gemini-live-may-2026/); [mindstudio](https://www.mindstudio.ai/blog/real-time-ai-voice-models-compared-2025); [Tom's Guide](https://tomsguide.com/ai/i-challenged-gemini-live-vs-chatgpt-in-5-voice-challenges-there-was-one-clear-winner)

### Inferences
- For role-play practice, conversational flow (barge-in, back-channelling) matters more than tool calling. That favours GPT-Live-1 or Grok Voice 3.0 on quality, and Gemini 3.8 Live on price/quality.
- For a 30-minute session, Gemini needs reconnection plus context compression handled in code. OpenAI's 60-minute cap is simpler.
- The user dislikes ChatGPT's consumer voice mode, and GPT-Live-1 is the API version of that ChatGPT experience ("bring ChatGPT's natural back-and-forth to your app"). If what they dislike is the voice or the turn-taking style rather than the app wrapper, GPT-Live-1 may feel the same. That is worth a quick trial, alongside Gemini 3.8 Live and Grok.

### Gaps
- No Reddit, HN or learner reviews specific to language practice were found within the search budget.
- No independent barge-in or interruption benchmarks per vendor beyond Full Duplex Bench claims (OpenAI-reported).
- Hume EVI, ElevenLabs Agents and Deepgram agents have no Artificial Analysis index scores in the fetched table.

## 3. Persona control, voices, languages and accents

### Takeaway
All major S2S APIs take a system prompt or instructions, which is enough for professor, doctor and mechanic personas. Gemini 3.8 Live offers the most voices (30) and the broadest documented language list (97). OpenAI publishes no language list but supports German in practice through its multilingual models (unverified here). ElevenLabs has the largest voice and accent catalogue.

### Cited Findings
- GPT-Live-1 has 13 voices (default "marin") and a 128K context. GPT-Realtime-2.1 has 10 voices ("marin" and "cedar" recommended) and a 128K context. Gemini 3.8 Live has 30 prebuilt voices, a 131K input context, and 97 languages per Google's launch post. OpenAI publishes no language list. — [Micdrop](https://micdrop.dev/blog/gpt-live-vs-gpt-realtime-vs-gemini-live)
- GPT-Live-1 has "twelve real-time voices" covering more accents, dialects and languages. Custom voices are available only through OpenAI sales. Note the 12 vs 13 conflict with Micdrop. — [search summary of OpenAI announcement / datanorth](https://datanorth.ai/news/openai-launches-gpt-live-1-in-the-api)
- GPT-Live-1 delegates reasoning to a developer-chosen backend model. Persona and logic can live in that backend's prompt. — [unite.ai](https://www.unite.ai/openais-gpt-live-1-arrives-in-the-api-at-0-05-per-minute/)

### Inferences
- Accent choice for English practice (British/American/other) comes mainly from voice selection plus instructions ("speak with a Scottish accent"). How well S2S models follow accent instructions is not documented in what I found.
- German support should be tested directly. Gemini lists it among its 97 languages; OpenAI's coverage is assumed, not verified.

### Gaps
- No verified per-voice accent lists for any vendor.
- Hume EVI, Grok and Nova Sonic language lists were not checked. Nova Sonic historically had limited languages; verify German support.

## 4. Transcripts of both sides, and whether learner errors are kept

### Takeaway
OpenAI Realtime and Gemini Live both return user and assistant transcripts. The user-side transcript comes from a separate ASR pass, not from what the conversation model "heard". Gemini documents a VERBATIM default mode that keeps hesitations and restarts, and an opt-in SMART mode that polishes grammar. OpenAI documents no cleanup mode, but its gpt-transcribe uses earlier turns as context, and forum posts say to treat the transcript as "guidance", not exact. For a reliable mistakes report, the app should also transcribe the learner's mic audio itself.

### Cited Findings
- Gemini Live input transcription is configured with `input_audio_transcription` (`language_codes`, `custom_vocabulary`, `mode`). Results come as `interim_input_transcription` and `input_transcription`.
  - VERBATIM (default) "Produces an exact literal transcript of everything spoken", including fillers, repetitions and false starts.
  - SMART "Cleans up and structures the transcript for readability", resolves self-corrections and "Applies natural capitalization and punctuation polish".
  - The page names only `gemini-3.5-transcribe-live` for this feature. No word timestamps; streaming up to 10 min.
  — [Gemini live transcription docs](https://ai.google.dev/gemini-api/docs/live-api/live-transcribe)
- OpenAI realtime transcription:
  - `gpt-live-transcribe` is recommended for live transcription. It accepts `prompt`, `keywords`, `languages` (array) and `delay` (minimal…xhigh).
  - "When `gpt-transcribe` performs input transcription in a Realtime API session" it automatically uses earlier transcribed turns as context.
  - Connections are over WebSocket (server) or WebRTC (browser).
  — [OpenAI realtime transcription guide](https://developers.openai.com/api/docs/guides/realtime-transcription)
- OpenAI forum: one reply suggests treating realtime transcription as "guidance of input audio content rather than precisely what the model heard". Another developer steers it with a prompt like "Transcribe speech accurately as spoken". These are not official statements. — [OpenAI community: transcription errors](https://community.openai.com/t/transcription-errors-in-realtime-api/1367938)
- ElevenLabs realtime STT has a `no_verbatim` option that removes fillers and false starts. It mentions no grammar correction. — [ElevenLabs realtime STT API](https://elevenlabs.io/docs/api-reference/speech-to-text/v-1-speech-to-text-realtime)
- Gemini Live does not support speaker diarization in live sessions. — [Gemini live transcription docs](https://ai.google.dev/gemini-api/docs/live-api/live-transcribe)

### Inferences
- I found no vendor that documents keeping grammatical errors, as distinct from disfluencies. ASR models with language-model priors (Whisper-family, gpt-transcribe with prior-turn context) tend to normalise towards fluent text: "he go" may come out as "he goes". This is a general ASR property and is not measured here.
- Conclave already ships local Whisper. The cheapest faithful option is to record the user's mic track locally and transcribe it locally (or with Groq Whisper at $0.04/hr), with a prompt that asks for verbatim output. Then use the vendor's transcript only for display.
- Assistant-side transcripts are reliable: they are the model's own text output.

### Gaps
- Not tested: whether Gemini 3.8 Live speech-to-speech sessions accept the VERBATIM/SMART `mode` field. The docs page tied it to `gemini-3.5-transcribe-live`.
- Not confirmed: whether OpenAI bills the input transcription inside a realtime session separately, at the per-minute transcribe rates.
- No empirical test of grammar-error preservation exists for any vendor. This needs a small in-house test with deliberately wrong sentences.

## 5. Integration path from a Tauri desktop app

### Takeaway
OpenAI (WebRTC, WebSocket or SIP; ephemeral client secrets) and Gemini Live (WebSocket; ephemeral tokens) both suit a Tauri app. Keep the API key in Rust, have Rust mint a short-lived token, and let the Next.js webview connect directly over WebRTC (OpenAI) or WebSocket (Gemini) for low-latency mic and speaker audio. Alternatively, run the whole socket in Rust and reuse Conclave's existing cpal capture.

### Cited Findings
- GPT-Live-1 uses `v1/live/sessions` over WebSocket, WebRTC or SIP. GPT-Realtime-2.1 uses `v1/realtime`. Gemini 3.8 Live uses the Live API over WebSocket. — [Micdrop](https://micdrop.dev/blog/gpt-live-vs-gpt-realtime-vs-gemini-live)
- GPT-Live-1: streaming and function calling; concurrent sessions from 25 (Tier 1) to 500 (Tier 5); audio and text only (no image or video). — [unite.ai](https://www.unite.ai/openais-gpt-live-1-arrives-in-the-api-at-0-05-per-minute/); [explainx](https://www.explainx.ai/blog/gpt-live-1-api-openai-voice-agents-september-2026)
- OpenAI realtime transcription works over WebSocket for server pipelines and WebRTC for browser audio. — [OpenAI realtime transcription guide](https://developers.openai.com/api/docs/guides/realtime-transcription)
- The Gemini Live docs link a separate "Ephemeral tokens" page for client-side connections (not fetched). — [Gemini session management](https://ai.google.dev/gemini-api/docs/live-api/session-management)

### Inferences
- Simplest path: WebRTC in the webview (WebView2 on Windows supports `getUserMedia` and `RTCPeerConnection`), with the ephemeral key minted by a Tauri command. That gives built-in echo cancellation, which matters for barge-in on speakers rather than headphones.
- Managed platforms (ElevenLabs, Deepgram, Hume, Ultravox) ship web SDKs. They are easy to embed but add a per-minute platform fee and lock the persona logic into their dashboard.
- A DIY cascade needs VAD, turn-taking and barge-in written by hand. That is more code and higher latency, but Conclave already has VAD and Whisper.
- Network-call invariant: any vendor endpoint must be added to `docs/network-calls.md` and the no-phone-home allow-list.

### Gaps
- Not fetched: the OpenAI ephemeral client-secret docs and the Gemini ephemeral-token docs (exact TTL and endpoints).

## 6. Pronunciation assessment APIs

### Takeaway
Azure Pronunciation Assessment is by far the cheapest add-on: about $0.30/hr on top of real-time STT, per a third party; German support was not checked. Speechace and ELSA charge per 15 s of audio (roughly $0.03–0.10 per minute of learner speech) and are English-focused.

### Cited Findings
- **Azure:**
  - Pronunciation assessment is an add-on to real-time STT and cannot be used with Fast Transcription ($0.66/hr).
  - Reported rates: real-time STT $1/hr, plus $0.30/hr per feature for pronunciation assessment; batch includes it. A Microsoft Q&A answer instead says $1.32/hr, which looks unreliable.
  - The official page did not render numbers.
  — [Microsoft Q&A](https://learn.microsoft.com/en-in/answers/questions/5608069/pricing-and-usage-of-pronunciation-assessment-feat); [blocksentient review](https://blocksentient.com/review/microsoft-azure-speech-service/); [Azure Speech pricing](https://azure.microsoft.com/en-us/pricing/details/speech/)
- **Speechace:** Basic $40/mo, Pro $80/mo, Premium $125/mo. The fee works as credit against usage, with overage beyond it. Rates: $0.008 per 15 s (Basic) and $0.0125 per 15 s for spontaneous speech (Premium). — [Speechace API plans](https://www.speechace.com/api-plans/); [Speechace FAQ](https://api-docs.speechace.com/resources/faqs)
- **ELSA API:** Standard $0.008 per 15 s scripted and $0.02 unscripted; Premium $0.01 and $0.025. — [ELSA API](https://elsaspeak.com/en/elsa-api/)
- A competitor's (TonePerfect) September 2026 comparison estimates about $1.40 per 1,000 recordings on Azure versus $40 on Speechace. It is a biased source. — [TonePerfect](https://api.toneperfect.app/blog/pronunciation-assessment-api-pricing-compared/)

### Inferences
- Speechace Basic at $0.008/15 s is about $0.032/min of learner speech. ELSA unscripted at $0.02/15 s is about $0.08/min. With about 15 min of learner speech per 30-min session, that is about $0.50 (Speechace) to $1.20 (ELSA) per session. Azure would be a few cents.
- Unscripted pronunciation scoring is harder than scripted (reading a given text). A practical design: an optional "read this line" drill using scripted assessment, plus post-session grammar feedback from an LLM over the verbatim transcript.

### Gaps
- Not confirmed: the official Azure Pronunciation Assessment price, its German support, and whether it works on unscripted speech in Voice Live.
- Not checked: the German support of Speechace and ELSA (both appear English-focused).
