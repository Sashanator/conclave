//! Knowledge layer: tasks, calendar briefs, the feed, digests and Ask. Settings come first.
// Later knowledge tasks consume these constants and helpers; they are unused until then.
#![allow(dead_code)]

pub mod ask;
pub mod briefs;
pub mod calendar;
pub mod extract;
pub mod feed;
pub mod retrieve;
pub mod scheduler;
pub mod settings;
pub mod tasks;

/// LLM call timeouts, in seconds.
pub const EXTRACT_TIMEOUT_SECS: u64 = 90;
pub const DIGEST_TIMEOUT_SECS: u64 = 120;
pub const ASK_PLAN_TIMEOUT_SECS: u64 = 30;
pub const ASK_ANSWER_TIMEOUT_SECS: u64 = 90;

/// Calendar feed refresh and fetch limits.
pub const CALENDAR_REFRESH_MINS: u64 = 30;
pub const CALENDAR_FETCH_TIMEOUT_SECS: u64 = 20;
pub const CALENDAR_MAX_BYTES: usize = 5 * 1024 * 1024;
pub const CALENDAR_AHEAD_DAYS: i64 = 14;
pub const CALENDAR_BEHIND_DAYS: i64 = 1;

/// Timing for auto-titles, briefs and feed retention.
pub const AUTO_TITLE_LEAD_MINS: i64 = 10;
pub const BRIEF_LEAD_MINS: i64 = 60;
pub const FEED_RETENTION_DAYS: i64 = 30;

/// Length limits for names, tasks and calendar event titles.
pub const MAX_NAMES: usize = 20;
pub const MAX_NAME_CHARS: usize = 40;
pub const MAX_TASK_CHARS: usize = 500;
/// Items kept per extraction list, and the cap on the user lines sent to the model (cost).
pub const EXTRACT_MAX_ITEMS: usize = 50;
pub const EXTRACT_MAX_OWN_LINES_CHARS: usize = 20_000;
pub const MAX_EVENT_TITLE_CHARS: usize = 200;

/// Ask limits: keywords, excerpts, excerpt size and question size.
pub const ASK_MAX_KEYWORDS: usize = 12;
pub const ASK_MAX_EXCERPTS: usize = 30;
pub const ASK_MAX_EXCERPT_CHARS: usize = 40_000;
pub const ASK_MAX_QUESTION_CHARS: usize = 2000;

/// The head of each summary sent to a weekly digest.
pub const DIGEST_SUMMARY_HEAD_CHARS: usize = 800;
