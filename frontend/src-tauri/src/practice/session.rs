//! Speaking Practice session state machine: who speaks when.
//! Pure: the runner feeds `Event`s in and carries out the returned `Effect`s.

use serde::Serialize;

use super::{MAX_SESSION_SECS, MIN_SPEECH_SECS, WARN_AT_SECS, WRAP_UP_AT_SECS};

#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
pub enum Phase {
    Loading,
    CharacterSpeaking,
    Thinking,
    UserTurn,
    Listening,
    Transcribing,
    Ended,
}

#[derive(Debug, PartialEq)]
pub enum Event {
    Ready,
    Tap,
    EndRequested,
    RecordingStarted,
    TurnTooLong,
    Tick { elapsed_secs: u64 },
    TurnTranscribed { text: String, speech_secs: f32 },
    ReplySentence { reply: u64, seq: u32, text: String },
    ReplyDone { reply: u64 },
    ReplyFailed { reply: u64 },
    SentencePlayed { reply: u64, seq: u32 },
}

#[derive(Debug, PartialEq)]
pub enum Effect {
    Phase(Phase),
    Notice(Notice),
    StartCapture,
    StopCaptureAndTranscribe,
    AskCharacter { reply: u64, wrap_up: bool },
    Speak { reply: u64, seq: u32, text: String },
    StopSpeaking,
    CancelReply { reply: u64 },
    SaveUserTurn { text: String },
    SaveCharacterTurn { text: String, interrupted: bool },
    End(EndReason),
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
pub enum Notice {
    DidNotCatch,
    TenMinutesLeft,
    TurnCapped,
    ReplyFailed,
    MeetingStarted,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EndReason {
    User,
    TimeCap,
    MeetingRecording,
}

pub struct Core {
    phase: Phase,
    /// Last reply id handed out. Ids start at 1.
    replies: u64,
    /// The reply in progress. Events for any other reply id are stale.
    current: Option<u64>,
    /// Sentences sent to `Speak` and not yet reported played, in order.
    pending: Vec<(u32, String)>,
    /// Text of the sentences that played for the current reply, in order.
    played: Vec<String>,
    reply_done: bool,
    warned: bool,
    wrap_up: bool,
}

impl Default for Core {
    fn default() -> Self {
        Self::new()
    }
}

impl Core {
    pub fn new() -> Self {
        Self {
            phase: Phase::Loading,
            replies: 0,
            current: None,
            pending: Vec::new(),
            played: Vec::new(),
            reply_done: false,
            warned: false,
            wrap_up: false,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn on_event(&mut self, ev: Event) -> Vec<Effect> {
        if self.phase == Phase::Ended {
            return match ev {
                Event::TurnTranscribed { text, .. } if !text.trim().is_empty() => {
                    vec![Effect::SaveUserTurn {
                        text: text.trim().to_string(),
                    }]
                }
                _ => Vec::new(),
            };
        }
        match ev {
            Event::Ready if self.phase == Phase::Loading => self.ask(),
            Event::Tap => match self.phase {
                Phase::UserTurn => {
                    self.phase = Phase::Listening;
                    vec![Effect::StartCapture, Effect::Phase(Phase::Listening)]
                }
                Phase::Listening => {
                    self.phase = Phase::Transcribing;
                    vec![
                        Effect::StopCaptureAndTranscribe,
                        Effect::Phase(Phase::Transcribing),
                    ]
                }
                // Barge-in: the user never waits for the character to finish.
                Phase::Thinking | Phase::CharacterSpeaking => {
                    let mut fx = self.stop_reply();
                    fx.push(Effect::StartCapture);
                    self.phase = Phase::Listening;
                    fx.push(Effect::Phase(Phase::Listening));
                    fx
                }
                _ => Vec::new(),
            },
            Event::TurnTooLong if self.phase == Phase::Listening => {
                self.phase = Phase::Transcribing;
                vec![
                    Effect::StopCaptureAndTranscribe,
                    Effect::Phase(Phase::Transcribing),
                    Effect::Notice(Notice::TurnCapped),
                ]
            }
            Event::TurnTranscribed { text, speech_secs } if self.phase == Phase::Transcribing => {
                let text = text.trim().to_string();
                if (speech_secs as f64) < MIN_SPEECH_SECS || text.is_empty() {
                    self.phase = Phase::UserTurn;
                    vec![
                        Effect::Notice(Notice::DidNotCatch),
                        Effect::Phase(Phase::UserTurn),
                    ]
                } else {
                    let mut fx = vec![Effect::SaveUserTurn { text }];
                    fx.extend(self.ask());
                    fx
                }
            }
            Event::ReplySentence { reply, seq, text } if self.is_current(reply) => {
                self.pending.push((seq, text.clone()));
                self.phase = Phase::CharacterSpeaking;
                vec![
                    Effect::Speak { reply, seq, text },
                    Effect::Phase(Phase::CharacterSpeaking),
                ]
            }
            Event::SentencePlayed { reply, seq } if self.is_current(reply) => {
                if let Some(i) = self.pending.iter().position(|(s, _)| *s == seq) {
                    let (_, text) = self.pending.remove(i);
                    self.played.push(text);
                }
                if self.reply_done && self.pending.is_empty() {
                    self.finish()
                } else {
                    Vec::new()
                }
            }
            Event::ReplyDone { reply } if self.is_current(reply) => {
                self.reply_done = true;
                if self.pending.is_empty() {
                    self.finish()
                } else {
                    Vec::new()
                }
            }
            Event::ReplyFailed { reply } if self.is_current(reply) => {
                // The stream is dead, so no CancelReply; queued sentences must still stop.
                let mut fx = vec![Effect::StopSpeaking];
                fx.extend(self.close_reply(true));
                fx.push(Effect::Notice(Notice::ReplyFailed));
                self.phase = Phase::UserTurn;
                fx.push(Effect::Phase(Phase::UserTurn));
                fx
            }
            Event::Tick { elapsed_secs } => {
                let t = elapsed_secs as f64;
                if t >= MAX_SESSION_SECS {
                    return self.end(EndReason::TimeCap);
                }
                let mut fx = Vec::new();
                if t >= WARN_AT_SECS && !self.warned {
                    self.warned = true;
                    fx.push(Effect::Notice(Notice::TenMinutesLeft));
                }
                if t >= WRAP_UP_AT_SECS {
                    self.wrap_up = true;
                }
                fx
            }
            Event::EndRequested => self.end(EndReason::User),
            Event::RecordingStarted => self.end(EndReason::MeetingRecording),
            _ => Vec::new(),
        }
    }

    fn is_current(&self, reply: u64) -> bool {
        self.current == Some(reply)
    }

    /// Starts the next character reply.
    fn ask(&mut self) -> Vec<Effect> {
        self.replies += 1;
        self.current = Some(self.replies);
        self.pending.clear();
        self.played.clear();
        self.reply_done = false;
        self.phase = Phase::Thinking;
        vec![
            Effect::AskCharacter {
                reply: self.replies,
                wrap_up: self.wrap_up,
            },
            Effect::Phase(Phase::Thinking),
        ]
    }

    /// The reply ran to its end: save what played and return the turn.
    fn finish(&mut self) -> Vec<Effect> {
        let mut fx: Vec<Effect> = self.close_reply(false).into_iter().collect();
        self.phase = Phase::UserTurn;
        fx.push(Effect::Phase(Phase::UserTurn));
        fx
    }

    /// Cancels the reply in progress, if any, and saves the part that played.
    fn stop_reply(&mut self) -> Vec<Effect> {
        let Some(reply) = self.current else {
            return Vec::new();
        };
        let mut fx = vec![Effect::CancelReply { reply }, Effect::StopSpeaking];
        fx.extend(self.close_reply(true));
        fx
    }

    /// Clears the current reply. Returns the save for its played sentences, if any.
    /// A reply that did not run to its end is saved as interrupted.
    fn close_reply(&mut self, interrupted: bool) -> Option<Effect> {
        self.current = None;
        self.pending.clear();
        self.reply_done = false;
        if self.played.is_empty() {
            return None;
        }
        let text = self.played.drain(..).collect::<Vec<_>>().join(" ");
        Some(Effect::SaveCharacterTurn { text, interrupted })
    }

    fn end(&mut self, reason: EndReason) -> Vec<Effect> {
        let mut fx = Vec::new();
        if self.phase == Phase::Listening {
            // That turn is saved when its transcript arrives; no reply follows.
            fx.push(Effect::StopCaptureAndTranscribe);
        }
        fx.extend(self.stop_reply());
        if reason == EndReason::MeetingRecording {
            fx.push(Effect::Notice(Notice::MeetingStarted));
        }
        fx.push(Effect::End(reason));
        fx.push(Effect::Phase(Phase::Ended));
        self.phase = Phase::Ended;
        fx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sentence(reply: u64, seq: u32, text: &str) -> Event {
        Event::ReplySentence {
            reply,
            seq,
            text: text.to_string(),
        }
    }

    fn played(reply: u64, seq: u32) -> Event {
        Event::SentencePlayed { reply, seq }
    }

    fn save_char(text: &str, interrupted: bool) -> Effect {
        Effect::SaveCharacterTurn {
            text: text.to_string(),
            interrupted,
        }
    }

    fn turn(text: &str, speech_secs: f32) -> Event {
        Event::TurnTranscribed {
            text: text.to_string(),
            speech_secs,
        }
    }

    /// Core in UserTurn: opening reply 1 spoken fully.
    fn at_user_turn() -> Core {
        let mut c = Core::new();
        c.on_event(Event::Ready);
        c.on_event(sentence(1, 0, "Hi there."));
        c.on_event(Event::ReplyDone { reply: 1 });
        c.on_event(played(1, 0));
        assert_eq!(c.phase(), Phase::UserTurn);
        c
    }

    /// Core in Listening, after one tap.
    fn listening() -> Core {
        let mut c = at_user_turn();
        c.on_event(Event::Tap);
        c
    }

    #[test]
    fn opening_asks_the_character() {
        let mut c = Core::new();
        assert_eq!(c.phase(), Phase::Loading);
        assert_eq!(
            c.on_event(Event::Ready),
            vec![
                Effect::AskCharacter {
                    reply: 1,
                    wrap_up: false
                },
                Effect::Phase(Phase::Thinking)
            ]
        );
        assert_eq!(c.phase(), Phase::Thinking);
    }

    #[test]
    fn sentences_are_spoken_then_the_turn_passes() {
        let mut c = Core::new();
        c.on_event(Event::Ready);
        assert_eq!(
            c.on_event(sentence(1, 0, "Hi.")),
            vec![
                Effect::Speak {
                    reply: 1,
                    seq: 0,
                    text: "Hi.".into()
                },
                Effect::Phase(Phase::CharacterSpeaking),
            ]
        );
        assert_eq!(
            c.on_event(sentence(1, 1, "How are you?")),
            vec![
                Effect::Speak {
                    reply: 1,
                    seq: 1,
                    text: "How are you?".into()
                },
                Effect::Phase(Phase::CharacterSpeaking),
            ]
        );
        assert!(c.on_event(Event::ReplyDone { reply: 1 }).is_empty());
        assert!(c.on_event(played(1, 0)).is_empty());
        assert_eq!(
            c.on_event(played(1, 1)),
            vec![
                save_char("Hi. How are you?", false),
                Effect::Phase(Phase::UserTurn)
            ]
        );
        assert_eq!(c.phase(), Phase::UserTurn);
    }

    #[test]
    fn empty_reply_passes_the_turn() {
        let mut c = Core::new();
        c.on_event(Event::Ready);
        assert_eq!(
            c.on_event(Event::ReplyDone { reply: 1 }),
            vec![Effect::Phase(Phase::UserTurn)]
        );
        assert_eq!(c.phase(), Phase::UserTurn);
    }

    #[test]
    fn tap_starts_and_ends_listening() {
        let mut c = at_user_turn();
        assert_eq!(
            c.on_event(Event::Tap),
            vec![Effect::StartCapture, Effect::Phase(Phase::Listening)]
        );
        assert_eq!(
            c.on_event(Event::Tap),
            vec![
                Effect::StopCaptureAndTranscribe,
                Effect::Phase(Phase::Transcribing)
            ]
        );
        assert_eq!(c.phase(), Phase::Transcribing);
    }

    #[test]
    fn turn_cap_ends_listening_with_notice() {
        let mut c = listening();
        assert_eq!(
            c.on_event(Event::TurnTooLong),
            vec![
                Effect::StopCaptureAndTranscribe,
                Effect::Phase(Phase::Transcribing),
                Effect::Notice(Notice::TurnCapped),
            ]
        );
        // The tap that raced the cap finds Transcribing and does nothing.
        assert!(c.on_event(Event::Tap).is_empty());
        assert!(c.on_event(Event::TurnTooLong).is_empty());
    }

    #[test]
    fn barge_in_cancels_and_saves_played_part() {
        let mut c = Core::new();
        c.on_event(Event::Ready);
        c.on_event(sentence(1, 0, "Hi."));
        c.on_event(sentence(1, 1, "Long story."));
        c.on_event(played(1, 0));
        assert_eq!(
            c.on_event(Event::Tap),
            vec![
                Effect::CancelReply { reply: 1 },
                Effect::StopSpeaking,
                save_char("Hi.", true),
                Effect::StartCapture,
                Effect::Phase(Phase::Listening),
            ]
        );
        assert_eq!(c.phase(), Phase::Listening);
        // The cancelled reply's later events are stale.
        assert!(c.on_event(played(1, 1)).is_empty());
        assert!(c.on_event(Event::ReplyDone { reply: 1 }).is_empty());
    }

    #[test]
    fn barge_in_before_any_sentence_saves_nothing() {
        let mut c = Core::new();
        c.on_event(Event::Ready);
        assert_eq!(
            c.on_event(Event::Tap),
            vec![
                Effect::CancelReply { reply: 1 },
                Effect::StopSpeaking,
                Effect::StartCapture,
                Effect::Phase(Phase::Listening),
            ]
        );
    }

    #[test]
    fn taps_while_loading_transcribing_or_ended_do_nothing() {
        let mut loading = Core::new();
        assert!(loading.on_event(Event::Tap).is_empty());
        assert_eq!(loading.phase(), Phase::Loading);

        let mut transcribing = listening();
        transcribing.on_event(Event::Tap);
        assert!(transcribing.on_event(Event::Tap).is_empty());
        assert_eq!(transcribing.phase(), Phase::Transcribing);

        let mut ended = Core::new();
        ended.on_event(Event::EndRequested);
        assert!(ended.on_event(Event::Tap).is_empty());
        assert_eq!(ended.phase(), Phase::Ended);
    }

    #[test]
    fn double_tap_never_starts_two_captures() {
        let mut c = at_user_turn();
        let mut starts = 0;
        for _ in 0..4 {
            starts += c
                .on_event(Event::Tap)
                .iter()
                .filter(|e| **e == Effect::StartCapture)
                .count();
        }
        assert_eq!(starts, 1);
    }

    #[test]
    fn short_or_blank_turn_is_not_sent() {
        let mut c = listening();
        c.on_event(Event::Tap);
        assert_eq!(
            c.on_event(turn("ok", 0.3)),
            vec![
                Effect::Notice(Notice::DidNotCatch),
                Effect::Phase(Phase::UserTurn)
            ]
        );
        assert_eq!(c.phase(), Phase::UserTurn);

        c.on_event(Event::Tap);
        c.on_event(Event::Tap);
        assert_eq!(
            c.on_event(turn("   ", 2.0)),
            vec![
                Effect::Notice(Notice::DidNotCatch),
                Effect::Phase(Phase::UserTurn)
            ]
        );
    }

    #[test]
    fn reply_failure_keeps_played_sentences_and_returns_the_turn() {
        let mut c = Core::new();
        c.on_event(Event::Ready);
        c.on_event(sentence(1, 0, "Hi."));
        c.on_event(played(1, 0));
        assert_eq!(
            c.on_event(Event::ReplyFailed { reply: 1 }),
            vec![
                Effect::StopSpeaking,
                save_char("Hi.", true),
                Effect::Notice(Notice::ReplyFailed),
                Effect::Phase(Phase::UserTurn),
            ]
        );
        assert_eq!(c.phase(), Phase::UserTurn);
    }

    #[test]
    fn reply_failure_stops_a_sentence_still_queued() {
        let mut c = Core::new();
        c.on_event(Event::Ready);
        c.on_event(sentence(1, 0, "Hi."));
        c.on_event(sentence(1, 1, "Still queued."));
        c.on_event(played(1, 0));
        assert_eq!(
            c.on_event(Event::ReplyFailed { reply: 1 }),
            vec![
                Effect::StopSpeaking,
                save_char("Hi.", true),
                Effect::Notice(Notice::ReplyFailed),
                Effect::Phase(Phase::UserTurn),
            ]
        );
        // The queued sentence is stopped, so its late playback report is stale.
        assert!(c.on_event(played(1, 1)).is_empty());
        assert_eq!(c.phase(), Phase::UserTurn);
    }

    #[test]
    fn tick_past_the_cap_from_user_turn_ends_without_warning() {
        let mut c = at_user_turn();
        assert_eq!(
            c.on_event(Event::Tick { elapsed_secs: 4000 }),
            vec![Effect::End(EndReason::TimeCap), Effect::Phase(Phase::Ended)]
        );
        assert_eq!(c.phase(), Phase::Ended);
    }

    #[test]
    fn ended_saves_a_short_non_blank_turn() {
        let mut c = at_user_turn();
        c.on_event(Event::EndRequested);
        assert_eq!(
            c.on_event(turn("ok", 0.2)),
            vec![Effect::SaveUserTurn { text: "ok".into() }]
        );
        assert!(c.on_event(turn("  ", 2.0)).is_empty());
    }

    #[test]
    fn warnings_and_wrap_up_at_50_and_55_minutes() {
        let mut c = at_user_turn();
        assert!(c.on_event(Event::Tick { elapsed_secs: 2999 }).is_empty());
        assert_eq!(
            c.on_event(Event::Tick { elapsed_secs: 3000 }),
            vec![Effect::Notice(Notice::TenMinutesLeft)]
        );
        assert!(c.on_event(Event::Tick { elapsed_secs: 3001 }).is_empty());
        assert!(c.on_event(Event::Tick { elapsed_secs: 3300 }).is_empty());
        c.on_event(Event::Tap);
        c.on_event(Event::Tap);
        assert_eq!(
            c.on_event(turn("Sure.", 2.0)),
            vec![
                Effect::SaveUserTurn {
                    text: "Sure.".into()
                },
                Effect::AskCharacter {
                    reply: 2,
                    wrap_up: true
                },
                Effect::Phase(Phase::Thinking),
            ]
        );
    }

    #[test]
    fn time_cap_ends_and_saves_the_turn_in_progress() {
        let mut c = listening();
        assert_eq!(
            c.on_event(Event::Tick { elapsed_secs: 3600 }),
            vec![
                Effect::StopCaptureAndTranscribe,
                Effect::End(EndReason::TimeCap),
                Effect::Phase(Phase::Ended),
            ]
        );
        assert_eq!(c.phase(), Phase::Ended);
        // The turn cut off by the cap is still saved when its transcript arrives.
        assert_eq!(
            c.on_event(turn("half an answer", 4.0)),
            vec![Effect::SaveUserTurn {
                text: "half an answer".into()
            }]
        );
        assert!(c.on_event(Event::Tick { elapsed_secs: 3700 }).is_empty());
    }

    #[test]
    fn meeting_recording_ends_with_notice() {
        let mut c = Core::new();
        c.on_event(Event::Ready);
        c.on_event(sentence(1, 0, "Hi."));
        c.on_event(played(1, 0));
        assert_eq!(
            c.on_event(Event::RecordingStarted),
            vec![
                Effect::CancelReply { reply: 1 },
                Effect::StopSpeaking,
                save_char("Hi.", true),
                Effect::Notice(Notice::MeetingStarted),
                Effect::End(EndReason::MeetingRecording),
                Effect::Phase(Phase::Ended),
            ]
        );
        assert!(c.on_event(Event::Tap).is_empty());
    }

    #[test]
    fn stale_reply_events_are_ignored() {
        let mut c = Core::new();
        c.on_event(Event::Ready);
        c.on_event(sentence(1, 0, "Hi."));
        c.on_event(Event::Tap); // barge-in: reply 1 is over
        c.on_event(Event::Tap); // stop the capture
        c.on_event(turn("Hello", 2.0));
        c.on_event(Event::ReplyFailed { reply: 1 }); // stale: reply 2 is the next ask
        assert_eq!(c.phase(), Phase::Thinking);
        assert!(c.on_event(sentence(1, 1, "late")).is_empty());
        assert!(c.on_event(played(1, 0)).is_empty());
        assert!(c.on_event(Event::ReplyDone { reply: 1 }).is_empty());
        assert!(c.on_event(Event::ReplyFailed { reply: 1 }).is_empty());
        assert_eq!(c.phase(), Phase::Thinking);
    }
}
