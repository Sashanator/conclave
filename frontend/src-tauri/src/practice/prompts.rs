#![allow(dead_code)]

pub struct Persona<'a> {
    pub character: &'a str,
    pub situation: &'a str,
    pub plan: Option<&'a str>,
}

pub const OPENING_USER: &str =
    "(The learner has just joined. Open the scene in character with a short greeting and your first line.)";
pub const WRAP_UP_SYSTEM: &str =
    "About five minutes are left. Start bringing the scene to a natural close over your next replies.";

const RULES: &str = r#"Rules:
- Stay in role at all times.
- Speak in a spoken style, usually 1 to 3 sentences.
- No lists, no markdown, no headings, no stage directions.
- Ask one question at a time.
- Never say "please wait" or "let me think".
- When the learner makes a clear mistake, occasionally repeat it back correctly inside a natural reply, without stopping to teach.
- If a reply needs a moment, start with a short natural reaction, such as "Hmm, interesting choice…"."#;

/// The variety line only makes sense with a pool, so it is emitted only with a plan.
pub fn character_system(p: &Persona, variety_seed: u32) -> String {
    let mut s = format!(
        "You are role-playing a character in a spoken practice conversation. The learner talks to you by voice, and your replies are read aloud.\n\nCharacter: {}\nSituation: {}\n\n",
        p.character, p.situation
    );
    if let Some(plan) = p.plan {
        s.push_str(&format!(
            "Examiner plan (private: use it to choose your questions and facts; never reveal it or read it out):\n{plan}\n\nFor variety, start today with question {} of your pool and move through the rest in a different order.\n\n",
            1 + variety_seed % 5
        ));
    }
    s.push_str(RULES);
    s
}

pub fn plan_prompts(character: &str, situation: &str) -> (String, String) {
    let system = "You prepare a speaking-practice session for a learner. Read the attached material and write an examiner plan as plain text, with no markdown tables. Include:\n\
1. A pool of 15 to 20 questions the character could ask, numbered.\n\
2. The weak points an examiner would probe in the material.\n\
3. Likely follow-ups to the main questions.\n\
4. Key facts from the material that the character should know and may mention.\n\
Write the plan for the character playing the examiner. Do not write the learner's answers.";
    let user = format!(
        "Character: {character}\nSituation: {situation}\n\nWrite the plan from the attached material."
    );
    (system.to_string(), user)
}

const REPORT_SYSTEM: &str = r#"You review a spoken English practice session. Each turn is labelled with its number and speaker. The learner's turns are verbatim transcripts, fillers and false starts included. Find mistakes in the learner's turns only. Never flag the character's turns.

Reply with JSON only, no other text, in this shape:
{"grammar":[{"turn":2,"quote":"...","better":"...","note":"...","echoed":true}],"unnatural":[{"turn":2,"quote":"...","better":"...","note":"..."}],"clarity":[{"turn":2,"quote":"...","better":"...","note":"..."}]}

- turn: the number of the learner turn.
- quote: an exact, contiguous part of that learner turn, copied character for character. Do not fix, reword or join pieces.
- better: the improved version of the quote.
- note: one short line.
- echoed (grammar only): true if the character's next turn repeats the corrected form, otherwise false.
- Report only real problems. Use an empty list when there are none."#;

pub fn report_prompts(turns: &[(i64, &str, &str)]) -> (String, String) {
    let user = turns
        .iter()
        .map(|(idx, speaker, text)| format!("T{idx} [{speaker}]: {text}"))
        .collect::<Vec<_>>()
        .join("\n");
    (REPORT_SYSTEM.to_string(), user)
}

const ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "prof", "st", "vs", "etc", "e.g", "i.e",
];

/// Splits streamed text into sentences. A sentence end is decided only once the next word has arrived.
pub struct SentenceSplitter {
    buf: String,
}

impl SentenceSplitter {
    pub fn new() -> Self {
        Self { buf: String::new() }
    }

    pub fn push(&mut self, delta: &str) -> Vec<String> {
        self.buf.push_str(delta);
        let mut out = Vec::new();
        while let Some(end) = sentence_end(&self.buf) {
            let sentence = self.buf.drain(..end).collect::<String>();
            let sentence = sentence.trim();
            if !sentence.is_empty() {
                out.push(sentence.to_string());
            }
        }
        out
    }

    pub fn finish(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.buf);
        let rest = rest.trim();
        (!rest.is_empty()).then(|| rest.to_string())
    }
}

/// Byte index just past the first sentence end in `s` that can be decided now.
fn sentence_end(s: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    for i in 0..chars.len() {
        let (pos, c) = chars[i];
        if !matches!(c, '.' | '!' | '?' | '…') {
            continue;
        }
        let mut j = i + 1;
        while j < chars.len() && is_closer(chars[j].1) {
            j += 1;
        }
        if j >= chars.len() || !chars[j].1.is_whitespace() {
            continue;
        }
        let mut k = j;
        while k < chars.len() && chars[k].1.is_whitespace() {
            k += 1;
        }
        if k == chars.len() {
            return None; // the next word has not streamed in yet
        }
        if chars[k].1.is_lowercase() {
            continue;
        }
        if c == '.' && is_abbreviation(&s[..pos]) {
            continue;
        }
        return Some(chars[j].0);
    }
    None
}

fn is_closer(c: char) -> bool {
    matches!(c, '"' | '\'' | '”' | '’' | ')' | ']')
}

/// `before` is the text ahead of a '.'. Checks the word that ends there.
fn is_abbreviation(before: &str) -> bool {
    let word = before.rsplit(char::is_whitespace).next().unwrap_or("");
    let word = word.trim_start_matches(|c: char| !c.is_alphanumeric());
    if ABBREVIATIONS.contains(&word.to_lowercase().as_str()) {
        return true;
    }
    let mut chars = word.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if c.is_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split_all(deltas: &[&str]) -> (Vec<String>, Option<String>) {
        let mut sp = SentenceSplitter::new();
        let mut out = Vec::new();
        for d in deltas {
            out.extend(sp.push(d));
        }
        (out, sp.finish())
    }

    #[test]
    fn splits_on_sentence_ends_across_deltas() {
        let mut sp = SentenceSplitter::new();
        assert_eq!(sp.push("Hmm, interesting. Why"), vec!["Hmm, interesting."]);
        assert!(sp.push(" did you?").is_empty());
        assert_eq!(sp.finish().as_deref(), Some("Why did you?"));
        assert_eq!(sp.finish(), None);
    }

    #[test]
    fn keeps_abbreviations_and_numbers() {
        let (out, rest) = split_all(&["Dr. Smith uses 3.5 GB, e.g. this. Next."]);
        assert_eq!(out, vec!["Dr. Smith uses 3.5 GB, e.g. this."]);
        assert_eq!(rest.as_deref(), Some("Next."));
    }

    #[test]
    fn char_by_char_matches_whole_text() {
        let text = "Dr. Smith uses 3.5 GB, e.g. this. Well… \"Really?\" Yes! Next.";
        let whole = split_all(&[text]);
        let chars: Vec<String> = text.chars().map(|c| c.to_string()).collect();
        let refs: Vec<&str> = chars.iter().map(String::as_str).collect();
        assert_eq!(split_all(&refs), whole);
    }

    #[test]
    fn ellipsis_and_quotes() {
        let (out, rest) = split_all(&["Well… \"Really?\" Yes."]);
        assert_eq!(out, vec!["Well…", "\"Really?\""]);
        assert_eq!(rest.as_deref(), Some("Yes."));
    }

    #[test]
    fn character_system_contains_rules_and_plan() {
        let with_plan = character_system(
            &Persona {
                character: "Anna, a sceptical hiring manager",
                situation: "A 20-minute phone screen",
                plan: Some("PLAN-MARKER: ask about the gap in 2023"),
            },
            0,
        );
        assert!(with_plan.contains("Anna, a sceptical hiring manager"));
        assert!(with_plan.contains("A 20-minute phone screen"));
        assert!(with_plan.contains("PLAN-MARKER: ask about the gap in 2023"));
        let lower = with_plan.to_lowercase();
        assert!(lower.contains("never say"));
        assert!(lower.contains("one question"));

        let without = character_system(
            &Persona {
                character: "Anna",
                situation: "Phone screen",
                plan: None,
            },
            0,
        );
        assert!(!without.contains("PLAN-MARKER"));
        assert!(!without.to_lowercase().contains("examiner plan"));
        assert!(!without.contains("question 1 of your pool"));
    }

    #[test]
    fn variety_line_depends_on_seed() {
        let persona = Persona {
            character: "Anna",
            situation: "Phone screen",
            plan: Some("Q1 ... Q20"),
        };
        let line = |seed| {
            let s = character_system(&persona, seed);
            s.lines()
                .find(|l| l.starts_with("For variety"))
                .unwrap()
                .to_string()
        };
        assert!(line(0).contains("question 1 of your pool"));
        assert!(line(1).contains("question 2 of your pool"));
        assert_eq!(line(0), line(5));
        assert_ne!(line(0), line(1));
    }

    #[test]
    fn report_prompt_numbers_turns_and_labels_speakers() {
        let (system, user) = report_prompts(&[
            (1, "character", "Tell me about yourself."),
            (2, "learner", "Um, I am engineer."),
        ]);
        assert!(user.contains("T1 [character]: Tell me about yourself."));
        assert!(user.contains("T2 [learner]: Um, I am engineer."));
        assert!(system.contains("\"turn\""));
        assert!(system.contains("exact, contiguous"));
    }
}
