// Restricts Whisper's automatic language choice to the user's own languages.
// Short chunks are guessed wrongly on their own, so they keep the current language.

pub const SHORT_CHUNK_SECS: f32 = 1.5;
pub const SWITCH_MIN_SECS: f32 = 3.0;
pub const MIN_ALLOWED_MASS: f32 = 0.3;
pub const MIN_SHARE: f32 = 0.8;

pub struct LanguagePolicy {
    allowed: Vec<String>,
    current: Option<String>,
}

impl LanguagePolicy {
    pub fn new(allowed: Vec<String>) -> Self {
        Self { allowed, current: None }
    }

    pub fn is_active(&self) -> bool {
        !self.allowed.is_empty()
    }

    pub fn allowed(&self) -> &[String] {
        &self.allowed
    }

    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// Language for a chunk without running detection, or None when detection is needed.
    pub fn decide_without_detection(&self, duration_secs: f32) -> Option<String> {
        if self.allowed.len() == 1 {
            return Some(self.allowed[0].clone());
        }
        match &self.current {
            Some(current) if duration_secs < SHORT_CHUNK_SECS => Some(current.clone()),
            _ => None,
        }
    }

    /// `probs[i]` is Whisper's absolute probability of `allowed[i]`. Callers must check `is_active()`.
    pub fn choose(&mut self, probs: &[f32], duration_secs: f32) -> String {
        let mass: f32 = probs.iter().sum();
        let mut best = 0;
        for (i, p) in probs.iter().enumerate() {
            if *p > probs[best] {
                best = i;
            }
        }
        let share = if mass > 0.0 { probs[best] / mass } else { 0.0 };
        let confident = mass >= MIN_ALLOWED_MASS && share >= MIN_SHARE;
        let best_lang = self.allowed[best].clone();

        let chosen = if confident {
            best_lang.clone()
        } else {
            self.current.clone().unwrap_or_else(|| best_lang.clone())
        };
        if confident && duration_secs >= SWITCH_MIN_SECS {
            self.current = Some(best_lang);
        }
        chosen
    }
}

/// Whisper drops input under 1 s, so short chunks are padded with silence to 1.1 s.
/// Returns the audio and the number of silent samples added before it.
pub fn pad_to_min_duration(audio: Vec<f32>) -> (Vec<f32>, usize) {
    if audio.is_empty() || audio.len() >= 16_000 {
        return (audio, 0);
    }
    let before = (17_600 - audio.len()) / 2;
    let mut padded = vec![0.0; 17_600];
    padded[before..before + audio.len()].copy_from_slice(&audio);
    (padded, before)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(allowed: &[&str], current: Option<&str>) -> LanguagePolicy {
        let mut p = LanguagePolicy::new(allowed.iter().map(|s| s.to_string()).collect());
        p.current = current.map(|s| s.to_string());
        p
    }

    #[test]
    fn single_allowed_language_is_used_for_any_duration() {
        let p = policy(&["de"], None);
        assert_eq!(p.decide_without_detection(0.5), Some("de".to_string()));
        assert_eq!(p.decide_without_detection(30.0), Some("de".to_string()));
    }

    #[test]
    fn short_chunk_keeps_current_language() {
        let p = policy(&["ru", "de"], Some("ru"));
        assert_eq!(p.decide_without_detection(0.9), Some("ru".to_string()));
    }

    #[test]
    fn first_short_chunk_without_current_needs_detection() {
        let mut p = policy(&["ru", "de"], None);
        assert_eq!(p.decide_without_detection(0.9), None);
        assert_eq!(p.choose(&[0.015, 0.0], 0.9), "ru");
        assert_eq!(p.current(), None);
    }

    #[test]
    fn confident_long_chunk_switches_current_language() {
        // Gemini case: a long German chunk is clearly German
        let mut p = policy(&["ru", "de"], Some("ru"));
        assert_eq!(p.choose(&[0.015, 0.791], 7.9), "de");
        assert_eq!(p.current(), Some("de"));
    }

    #[test]
    fn low_probability_mass_keeps_current_language() {
        let mut p = policy(&["ru", "de"], Some("ru"));
        assert_eq!(p.choose(&[0.015, 0.004], 4.0), "ru");
        assert_eq!(p.current(), Some("ru"));
    }

    #[test]
    fn unclear_choice_keeps_current_language() {
        let mut p = policy(&["ru", "de"], Some("ru"));
        assert_eq!(p.choose(&[0.35, 0.30], 5.0), "ru");
        assert_eq!(p.current(), Some("ru"));
    }

    #[test]
    fn confident_short_chunk_decides_but_does_not_switch_current() {
        let mut p = policy(&["ru", "de"], Some("ru"));
        assert_eq!(p.choose(&[0.01, 0.9], 2.0), "de");
        assert_eq!(p.current(), Some("ru"));
    }

    #[test]
    fn pad_to_min_duration_centres_short_audio() {
        let (padded, _) = pad_to_min_duration(vec![1.0; 12_800]);
        assert_eq!(padded.len(), 17_600);
        assert_eq!(padded[2_399], 0.0);
        assert_eq!(padded[2_400], 1.0);
        assert_eq!(padded[2_400 + 12_799], 1.0);
        assert_eq!(padded[2_400 + 12_800], 0.0);
    }

    #[test]
    fn pad_to_min_duration_leaves_long_and_empty_audio_unchanged() {
        assert_eq!(pad_to_min_duration(vec![1.0; 16_000]), (vec![1.0; 16_000], 0));
        assert_eq!(pad_to_min_duration(Vec::new()), (Vec::new(), 0));
    }

    #[test]
    fn pad_reports_offset_before_audio() {
        let (padded, before) = pad_to_min_duration(vec![1.0; 8_000]);
        assert_eq!((padded.len(), before), (17_600, 4_800));
        let (padded, before) = pad_to_min_duration(vec![1.0; 16_000]);
        assert_eq!((padded.len(), before), (16_000, 0));
    }
}
