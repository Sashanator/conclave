# Reliable post-conversation feedback on spoken English weak spots (as of Oct 2026)

Scope note: about 19 tool calls. Many product sources are 2025–2026 affiliate or review blogs. Academic sources are mostly 2022–2025. Each finding is flagged with its date where known.

## 1. Does ASR "correct" learner grammar and drop disfluencies? Which settings or models keep them verbatim?

### Takeaway
Yes. Stock Whisper drops fillers by default and sometimes normalises learner grammar. On learner speech, most Whisper errors are on exactly the features a feedback report needs (fillers, self-repairs, ungrammatical forms). Verbatim-tuned models (CrisperWhisper) or hesitation-style prompts reduce the problem but do not remove it. Prompting carries a risk of hallucination and repetition loops.

### Cited Findings
- **LearnerVoice (2024): the Whisper error profile on learner speech.**
  - Data: 50 h of spontaneous speech from 58 Korean-L1 English learners.
  - Vanilla Whisper WER: whisper-small.en 18.38%; whisper-large-v3 19.18%. The authors blame large-v3's multilingual nature, since L2 accents "may be recognized as other languages".
  - 54.2% of whisper-small.en errors were tied to learner-speech features: fillers 37.6%, self-repairs 17.1%, ungrammatical expressions 4.8%. These sub-figures sum to 59.5%, so the paper's numbers are internally inconsistent.
  - The taxonomy defines "ungrammatical expression" errors as ASR auto-correcting the speaker's grammar, for example "i seems to be affected" → "i seem to be affected". The paper says recent ASR systems "often automatically rectify the grammatical errors" (citing Knill et al. 2018). It does not quantify this for Whisper alone.
  - Fine-tuning on learner data cut WER to 10.26% (−44.2%). Learner-feature errors fell 48.1%, against 19.2% for other error types.
  - [LearnerVoice, arXiv 2407.04280](https://arxiv.org/html/2407.04280)
- **Grammar smoothing in elicited imitation (2025):** a study of Japanese learners of English found that Whisper appears to reduce grammatical infelicities and sometimes substitutes the likely word for what was said. This can hide learner errors. Source is a search snippet only; the full paper was not read. [ScienceDirect S2772766125000187](https://www.sciencedirect.com/science/article/pii/S2772766125000187)
- **Five commercial and open ASR systems on L2-ARCTIC (Mar 2025):**
  - Read speech, MER: Whisper 0.054, AssemblyAI 0.056.
  - Spontaneous speech: RevAI was best, with mean MER 0.063.
  - Disfluency handling "varied significantly across systems and disfluency types".
  - On prompting: Whisper has no option to include fillers. An initial prompt makes disfluencies more likely to appear, "however, it often causes Whisper to hallucinate and include words or expressions that are not in the audio".
  - [McGuire, arXiv 2503.06924](https://arxiv.org/abs/2503.06924); prompt quote via [search snippet of same paper](https://arxiv.org/pdf/2503.06924)
- **Whisper drops fillers by design:** its text normaliser removes "uh"/"um" during scoring, and the model tends to omit them. [HF discussion](https://huggingface.co/spaces/openai/whisper/discussions/30); [WhisperD, arXiv 2505.21551](https://arxiv.org/pdf/2505.21551)
- **Prompt technique in practice:**
  - A video tool seeds the decoder with a hesitation prompt ("Um, uh, er, hmm...") and sets `condition_on_previous_text=False` to keep um/uh. [skills.sh filler-removal](https://www.skills.sh/agamm/video-agent/filler-removal)
  - On the OpenAI forum, users report that instructions in the prompt have no effect. Examples of hesitation transcription "generally work", but output sometimes "spirals out into repetition" around the "ahh/hmm". [OpenAI community](https://community.openai.com/t/how-to-avoid-hallucinations-in-whisper-transcriptions/125300?page=2)
- **CrisperWhisper:**
  - A Whisper fine-tune for verbatim transcription with accurate word timestamps. It keeps fillers, repetitions, false starts and partial words, and reports near-perfect filled-pause detection.
  - 2.0 has a verbatim mode (default, emits [UH]/[UM] tags) and an "intended" (clean) mode.
  - In the vendor's own benchmark, disfluency F1 is: CrisperWhisper 2.0 87.8, 2.0 Pro 93.5, ElevenLabs Scribe v2 79.2. This is vendor-run and not independently verified.
  - Licence: CC-BY-NC-4.0 (non-commercial).
  - [arXiv 2408.16589](https://arxiv.org/html/2408.16589v1); [GitHub nyrahealth/CrisperWhisper](https://github.com/nyrahealth/CrisperWhisper); [HF CrisperWhisper 2.0](https://huggingface.co/modulsx/CrisperWhisper2.0_large); [PromptLayer licence note](https://www.promptlayer.com/models/crisperwhisper)
- **End-to-end spoken GEC:** Cambridge-line papers use Whisper directly from learner audio to corrected text, and argue that it handles disfluencies well. This is the opposite goal (correcting rather than preserving), but it is relevant as a pipeline option. [arXiv 2311.05550](https://arxiv.org/html/2311.05550); [arXiv 2506.18532](https://arxiv.org/html/2506.18532)

### Inferences
- Running whisper.cpp large-v3-turbo as-is will under-report fillers and some grammar errors. The report would then flatter the learner. Fillers make up the largest single error class.
- **Two-pass design:**
  - Use the existing live Whisper pass for the conversation.
  - After the session, re-transcribe the user's turns with a verbatim model or a hesitation prompt and `condition_on_previous_text=False`, then compute metrics on that.
  - Because the reference text is unknown, treat any "error" the LLM flags as needing audio evidence. One option is to show low-confidence tokens to the user rather than asserting them.
- large-v3 does worse than small.en on L2 accents in LearnerVoice. Forcing `language=en` for the learner's turns, instead of auto-detect, should matter for a Russian/German speaker. This matches the project's own memory note that short chunks are mis-detected as the wrong language.
- The CC-BY-NC licence on CrisperWhisper is fine for a personal app but blocks any commercial distribution.

### Gaps
- No study found that directly measures how often Whisper large-v3/turbo silently fixes learner grammar errors (as opposed to fillers). The LearnerVoice 4.8% figure is for whisper-small.en.
- No verified 2026 information was found on these verbatim options: Parakeet (NVIDIA) disfluency behaviour; Deepgram's `filler_words` option; Azure STT's verbatim or disfluency output; AssemblyAI's `disfluencies` flag. My prior knowledge says Deepgram and AssemblyAI have such flags, but this was not verified here.
- No GGML/whisper.cpp port of CrisperWhisper was confirmed.

## 2. Pronunciation assessment: local open source vs APIs

### Takeaway
Azure Pronunciation Assessment is the most complete documented API. It supports unscripted ("speaking") assessment, phoneme scores and IPA output, but its best features (prosody, phoneme names, syllables, spoken-phoneme n-best) are **en-US only**. Locally, GOP and wav2vec2/HuBERT models reach phone-level PCC of about 0.68–0.70 against human ratings on speechocean762. That is usable for "likely mispronounced word" hints but not for precise scores.

### Cited Findings
- **Azure Pronunciation Assessment** (doc updated 2026-07-03):
  - Locale support:
    - "Prosody assessment is only available in the en-US locale".
    - Syllable-level accuracy is en-US only.
    - Phoneme *names* are available for en-US (IPA), and for en-US and zh-CN (SAPI). "For other locales, you can only get the phoneme score".
    - Spoken-phoneme n-best is en-US only.
  - Modes and scoring:
    - Unscripted assessment means omitting `ReferenceText`, and it is priced differently from scripted.
    - For unscripted assessment, Azure uses a different STT model than Azure STT. Microsoft recommends first calling Azure STT to obtain the reference text, then running a scripted assessment.
    - Audio over 30 s needs continuous mode, which does not support `EnableMiscue`.
    - `PronScore` is a weighted mix of the Accuracy, Fluency, Completeness and Prosody scores.
  - [Microsoft Learn: how-to-pronunciation-assessment](https://learn.microsoft.com/en-us/azure/ai-services/speech-service/how-to-pronunciation-assessment)
- **Azure pricing:**
  - Pronunciation Assessment (prosody, grammar, vocabulary, topic) is listed as an "enhanced add-on" on the Speech pricing page, but no global figure was retrievable.
  - The China-region page shows ¥3.05/hour per feature. That figure may be dated or regional.
  - A 2024 Microsoft Q&A answer said content and prosody assessment were en-US only.
  - [Azure pricing (sandbox snapshot)](https://acom-sandbox.azure.net/en-us/pricing/details/cognitive-services/speech-services/); [Azure China pricing](https://azure.cn/en-us/pricing/details/cognitive-services); [MS Q&A](https://learn.microsoft.com/en-us/answers/a/1597409)
- **SpeechSuper:**
  - Third-party listings give pay-as-you-go pricing of $0.004 (word) to $0.008 (paragraph) per request, with a $20 monthly minimum.
  - Its own GitHub samples describe the API as "scripted", so unscripted support is unclear.
  - [callin.io](https://callin.io/ai-tools/speechsuper); [SpeechSuper GitHub samples](https://github.com/speechsuper/SpeechSuper-API-Samples)
- **Speechace:**
  - Aggregators list "from $40/month" (unverified).
  - Its docs claim coverage of A0–C2 speaking activities. No per-request price was found.
  - [G2](https://www.g2.com/products/speechace-api/pricing); [Speechace docs](https://api-docs.speechace.com/solutions/speaking-practice-for-language-learning)
- **Open-source and academic accuracy** (speechocean762, PCC against human scores):
  - GOP pretraining plus phone-level similarity: phone level 0.702, against 0.610 without GOP. [arXiv 2302.10444](https://arxiv.org/pdf/2302.10444)
  - Segmentation-free GOP (2025): the authors say the best phone-level PCC they knew of was 0.693. [arXiv 2507.16838](https://arxiv.org/pdf/2507.16838)
  - SSL fine-tuning (2022): HuBERT-Large holistic PCC 0.82 against a 0.63 GOP baseline, and wav2vec2-Large 0.78. The table labelling is ambiguous; the result may be for the KESL dataset, not speechocean762. [arXiv 2204.03863](https://arxiv.org/pdf/2204.03863)
  - Newer alignment-free GOP work (2026) and CTC-GOP with phonological knowledge (2025) exist. Numbers were not extracted. [arXiv 2603.25150](https://arxiv.org/pdf/2603.25150); [arXiv 2506.02080](https://arxiv.org/pdf/2506.02080)
  - A community wav2vec2-XLS-R-300M phone-CTC model self-reports phone PCC 0.705 and utterance PCC 0.712 (unverified). [HF model card](https://huggingface.co/Pransfrance/cmubye-phoneme-encoder)
  - Using Whisper confidence or alternative transcriptions as mispronunciation cues has been proposed. [Whisper for L2 speech scoring](https://www.researchgate.net/publication/385475322_Whisper_for_L2_speech_scoring); [ICNLSP 2023](https://aclanthology.org/2023.icnlsp-1.30.pdf)
- **App reliability complaints:**
  - 2026 reviews say AI apps sometimes mark clear accented speech as wrong, which makes corrections feel arbitrary. [unstar.app](https://unstar.app/blog/is-speak-elsa-talkpal-loora-learna-ai-english-apps-legit-2026)
  - A Praktika user says the app praised pronunciation it could not actually hear. [Trustpilot Praktika](https://www.trustpilot.com/review/praktika.ai)

### Inferences
- A practical design is **scripted-after-the-fact**:
  1. Take the learner's verbatim transcript, ideally user-confirmed, as `ReferenceText`.
  2. Run Azure scripted assessment per turn (each under 30 s), as Microsoft itself recommends for accuracy.
  3. Report the lowest-scoring words and phonemes.

  Cost per session is likely cents, but no global price was confirmed.
- For German, Azure gives phoneme *scores* without names and no prosody. Local and API options are both weak for German.
- A phone-level PCC of about 0.7 means local scoring should be framed as "possible problem words" and not as numeric grades. Russian-L1 learners have well-known target sounds (th, w/v, vowel length, final devoicing). This is my inference, not sourced.

### Gaps
- The global Azure pronunciation assessment price for 2026 (scripted vs unscripted) was not retrievable.
- Azure "content assessment" (grammar, vocabulary, topic): the current how-to page no longer documents content scores. Its status (retired or en-US only) was unconfirmed.
- ELSA API pricing and accuracy were not found.
- No verified accuracy numbers were found for Allosaurus, MFA-based pipelines, or Kaldi GOP on Russian-L1 speakers.
- No German open-source pronunciation scorer was found.

## 3. Fluency metrics from timestamps: standard CAF definitions

### Takeaway
CAF research splits utterance fluency into three groups:
- **Speed:** articulation rate and speech rate.
- **Breakdown:** pause frequency and length, with mid-clause pauses most diagnostic for L2.
- **Repair:** repetitions, self-corrections, false starts.

Pause thresholds in the literature range from 100 to 1000 ms. The de Jong line of work recommends a short threshold of about 250 ms, but that exact value was not confirmed from the source in this search.

### Cited Findings
- **Standard formulas** ([LANGSNAP de Jong slides](https://web-archive.southampton.ac.uk/langsnap.soton.ac.uk/linked_files/LANGSNAP_dejong.pdf)):
  - Speech rate = syllables ÷ total time.
  - Articulation rate = syllables ÷ speaking time, with pauses excluded.
  - Pause thresholds in the literature range from 100 ms to 1000 ms. The slide citing De Jong & Bosker's recommended threshold was truncated in the search result.
- **L2 pausing:** learners pause more often and longer *within* clauses. Mid-clause pausing reflects formulation difficulty, and the number of mid-clause pauses separated mid from high performance. [Cambridge core reader](https://www.cambridge.org/core/product/09701CB638E6DCEF65658006518D6B58/core-reader); [UCL AP2018](https://discovery.ucl.ac.uk/10066019/1/AP2018.pdf)
- **Factor structure (SEM):**
  - Speed fluency was represented by articulation rate and mean length of utterance.
  - Breakdown fluency was represented by mid-clause pause length and frequency, and end-of-clause pause length.
  - Articulation rate best reflected functional adequacy.
  - [Applied Psycholinguistics (Cambridge)](https://www.cambridge.org/core/services/aop-cambridge-core/content/view/6F3FFCF75492235A2EF95B4152E5558A/S014271642400016Xa.pdf/what-contributes-to-fluent-l2-speech-examining-cognitive-and-utterance-fluency-link-with-underlying-l2-collocational-processing-speed-and-accuracy.pdf); [Language Assessment Quarterly 2023](https://www.informahealthcare.com/doi/abs/10.1080/15434303.2023.2283839)
- **Validity caveat:** native speakers vary considerably in speech rate and pausing. [IRAL 2022](https://www.degruyterbrill.com/document/doi/10.1515/iral-2022-0125/html)

### Inferences
- Metrics computable from word timestamps in a verbatim transcript:
  - **Words or syllables per minute:** total, and articulation rate (excluding pauses ≥ threshold).
  - **Mean length of run:** syllables between pauses.
  - **Pauses per 100 words:** split mid-clause vs end-of-clause. Clause boundaries could be approximated by an LLM or punctuation.
  - **Mean pause duration.**
  - **Filled pauses per 100 words:** um/uh/э/ähm.
  - **Repairs per 100 words.**
- These metrics depend on fillers being transcribed (section 1) and on accurate word timestamps. Stock Whisper timestamps are coarse, which is CrisperWhisper's stated motivation.
- Compare the user against their own previous sessions, not native norms, given the native variability caveat.
- Russian-L1 speakers may produce L1 fillers such as "э", "ну" or "как бы". A filler lexicon should include them; this is an inference.

### Gaps
- The exact De Jong & Bosker (2013) recommended threshold (my recollection: about 250 ms) was not verified in this search.
- No source found on syllable counting for automated English speech rate (e.g. CMUdict vs a heuristic).

## 4. LLM grammar and error feedback on transcripts: prompts, reliability, CEFR estimation

### Takeaway
Prompt-only LLM CEFR rating is moderately accurate but inconsistent, and degrades for intermediate and advanced grammar. Fine-tuned models do much better. Grammar feedback works best with structured categories, few-shot examples, and output pairs ("incorrect phrase → corrected phrase"). Transcript-only methods cannot judge pronunciation or prosody.

### Cited Findings
- **EvalYaks (2024):** Mistral-7B fine-tuned on CEFR B2 speaking *transcripts* reached 96% "acceptable accuracy" with 0.35 levels of variation, about 3× better than the next model. Limitation: text only, with pronunciation and prosody not scored. [arXiv 2408.12226](https://arxiv.org/html/2408.12226v1)
- **Prompting vs fine-tuning:** a study of CEFR text classification found prompting alone gives "moderate performance and high variability", that models under 8B are inconsistent, and that fine-tuning markedly improves accuracy. [Univ. Barcelona repository](https://diposit.ub.edu/bitstreams/a939d13e-e9d6-42f0-9cc9-81601b991824/download)
- **CTSEG benchmark:** a CEFR-based grammar benchmark found models do well on beginner grammar items, but performance "deteriorates substantially" for intermediate and advanced items. Source is a search summary; the paper URL was not captured.
- **Comuniqa:** a speaking-skills LLM app uses few-shot prompting with 20 predefined error categories and examples. It returns a score plus incorrect and corrected phrases. No reliability validation was reported. [arXiv 2401.15595](https://arxiv.org/pdf/2401.15595)
- **ChatGPT spoken ratings:** a Kobe University study compares ChatGPT speaking evaluations with the ICNALE GRA (80 human raters). Earlier work found near-human CEFR *writing* ratings when the model was given examples. [Kobe Univ.](https://da.lib.kobe-u.ac.jp/da/kernel/0100487710/0100487710.pdf)
- **Vendor counterpoint:** Speaknow, a commercial assessment company, argues that LLM proficiency ratings are overconfident and inconsistent. Weigh it as vendor opinion. [speaknow.co](https://speaknow.co/?p=1137)
- **Spoken GEC as a research field:** it treats disfluency removal and correction jointly. [arXiv 2506.18532](https://arxiv.org/html/2506.18532)

### Inferences
- **Prompt pattern** that the evidence supports:
  - A fixed error taxonomy: tense/aspect, articles, prepositions, agreement, word order, word choice/collocation, unnatural phrasing, L1 calques from Russian or German.
  - Few-shot examples for each category.
  - Require an exact quoted span from the transcript plus the correction plus a short rule.
  - Instruct the model to ignore disfluencies and false starts as *grammar* errors; count them only in fluency.
  - Ask for "uncertain, may be ASR" flags.
  - Validate in code that each quoted span exists verbatim in the transcript, to drop hallucinated errors.
- Show CEFR as a rough band with a caveat, or as a trend across sessions, rather than as a hard level. Run it at temperature 0 with a large model, or average several runs.
- Aggregate recurring errors across the session ("articles missed 7×") rather than listing every slip. This matches the review complaint that per-turn corrections get skipped (section 5).

### Gaps
- No 2025–2026 study found on frontier LLMs (Claude, GPT-5.x) rating CEFR from *spoken* transcripts with ASR noise.
- No precision or recall numbers found for LLM grammar feedback on L2 speech transcripts.

## 5. Existing products and projects, user complaints, and in-conversation vs post-session corrections (incl. ChatGPT Advanced Voice Mode)

### Takeaway
The top complaints are:
- the AI interrupting learners mid-thought (ChatGPT AVM above all);
- corrections that are easy to skip or that let slips pass;
- no end-of-session error summary;
- arbitrary pronunciation judgements;
- robotic or shallow content.

Research does not show that immediate or delayed corrective feedback is clearly better. One study reports learners feel uncomfortable with immediate correction. Together, these support a mostly uninterrupted role-play followed by a post-session report, with optional light recasts.

### Cited Findings
- **ChatGPT Advanced/Live Voice:**
  - A language learner says it cuts in "while they are still forming a thought". Hold-to-talk was removed and the mode still interrupts too early. [OpenAI community 1386172](https://community.openai.com/t/chatgpt-live-voice-still-interupts-too-much-what-happened-to-hold-to-talk/1386172)
  - Another user says a 1-second pause triggers interruption, and several users request a manual hold button. [OpenAI community 962909](https://community.openai.com/t/feature-request-advanced-voice-mode-keeps-interrupting-me/962909)
  - OpenAI later said users can pause without being cut off and get "fewer interruptions". [PCWorld](https://www.pcworld.com/article/2649338/chatgpt-now-speaks-even-more-naturally-with-fewer-interruptions.html)
  - Over-censorship complaints. [OpenAI community 971322](https://community.openai.com/t/frustration-with-over-censorship-in-chatgpts-advanced-voice-mode/971322)
  - A teacher found it drifted beyond the supplied material, used vocabulary above learner level, and claimed to show on-screen text that did not appear. [jaetakeuchi substack](https://jaetakeuchi.substack.com/p/ai-voice-mode-for-language-learners)
  - Voice mode "interprets words rather than listening to sounds", so it is unreliable for pronunciation. [gotranscript](https://gotranscript.com/public/exploring-new-advanced-voice-mode-in-chatgpt)
  - Most of these complaints date from 2024–2025; the current state was not verified.
- **Praktika:**
  - Corrections sit under each answer and are easy to skip. The tutor lets small grammar slips pass, and there is no end-of-lesson error list. [copycatcafe 2026](https://copycatcafe.com/blog/praktika-review)
  - Trustpilot users report vocabulary arriving too fast and praise for pronunciation the app couldn't hear. [Trustpilot](https://www.trustpilot.com/review/praktika.ai)
- **TalkPal:**
  - Robotic voices, shallow content, and no grammar lessons or spaced repetition. [LanguaTalk 2026](https://languatalk.com/blog/talkpal-review/)
  - Corrections are reactive, and users ask to repeat mistakes before moving on. [Headway 2026](https://makeheadway.com/blog/talkpal-review/)
  - Occasionally inaccurate corrections. [Kippy](https://kippy.ai/blog/best-ai-language-learning-apps-comparison)
- **Speak:** recommended for beginners and English learners, but reviewers cite repetitive lessons and paywalled content. [Lingtuitive](https://lingtuitive.com/blog/best-ai-speaking-apps); [unstar.app](https://unstar.app/blog/is-speak-elsa-talkpal-loora-learna-ai-english-apps-legit-2026)
- **Loora:** rated 4.9 on iOS and praised for feedback, English-only. Little critical coverage was found. [Kippy](https://kippy.ai/blog/best-ai-language-learning-apps-comparison)
- **Conflict:** Kippy says Praktika is English-only, while Lingtuitive says it teaches other languages. [Kippy](https://kippy.ai/blog/best-ai-language-learning-apps-comparison); [Lingtuitive](https://lingtuitive.com/blog/best-ai-speaking-apps)
- **Open source:**
  - "Maya language tutor" (AMD hackathon) uses faster-whisper or whisperx plus local Ollama and can run fully offline. It supports English and Chinese only. No GitHub link was confirmed. [lablab.ai](https://lablab.ai/ai-hackathons/amd-developer-hackathon-act-ii/yolo-street/maya-language-tutor)
  - No mature open-source role-play-plus-feedback project was found.
- **Feedback timing research:**
  - A study of 30 learners of Spanish found no significant difference between immediate and delayed recasts; both beat no feedback. [Leuphana](https://fis.leuphana.de/en/publications/the-role-of-timing-and-corrective-feedback-in-l2-discussion-activ/)
  - A Persian study found immediate feedback better for intonation. Source is a search summary; full-text URL not captured.
  - Students "did not feel comfortable" with immediate correction, and "there is no evidence showing the superiority of one type over the other". [ERIC EJ1167213](https://files.eric.ed.gov/fulltext/EJ1167213.pdf)
  - Overview of feedback timing and grammar. [Cambridge Handbook of CF, timing chapter](https://resolve-he.cambridge.org/core/books/abs/cambridge-handbook-of-corrective-feedback-in-second-language-learning-and-teaching/corrective-feedback-timing-and-second-language-grammatical-development-research-theory-and-practice/B55C0024292324FBEC86F50C6375C669)
  - No meta-analysis specific to oral fluency was found.

### Inferences
What the feature must do better than ChatGPT AVM and the apps:
1. **Never cut the learner off.** Use long silence tolerance, push-to-talk or hold-to-talk, or an explicit "I'm done" key. Learners pause mid-clause by nature (section 3).
2. **Stay in role** and stay at the learner's level. The AI should not play a "too-polite" assistant.
3. **Do not interrupt to correct.** At most, add an optional natural recast in the character's reply.
4. **Give a post-session report** that aggregates recurring errors, quotes the learner's exact words, offers corrected and more natural alternatives, includes fluency metrics, and flags pronunciation only where the evidence is strong.
5. **Track trends across sessions.** No reviewed product was praised for this, and the Praktika and TalkPal reviews explicitly miss it.

### Gaps
- No Reddit threads were retrieved (search returned none). The views of r/languagelearning, r/EnglishLearning and r/LocalLLaMA are uncovered.
- No feature or complaint data found for ELSA AI or Gliglish.
- No user survey comparing in-conversation and post-session report preferences was found.
- No 2026 data on the current state of ChatGPT voice interruptions.
