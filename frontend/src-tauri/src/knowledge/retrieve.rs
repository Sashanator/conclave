//! Keyword retrieval for Ask. Matching comes from `transcript::source_matches`; this module applies
//! the plan's filters, builds the excerpts and ranks them.

use super::{ASK_MAX_EXCERPTS, ASK_MAX_EXCERPT_CHARS, ASK_MAX_KEYWORDS};
use crate::database::repositories::transcript::source_matches;
use chrono::{DateTime, FixedOffset, Local, NaiveDate};
use serde::Deserialize;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};

/// What the model asked for. Empty or missing fields mean "no filter".
#[derive(Deserialize, Debug, PartialEq, Default)]
pub struct SearchPlan {
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub types: Vec<String>,
    pub from: Option<String>, /* YYYY-MM-DD */
    pub to: Option<String>,
    pub person: Option<String>,
}

/// One piece of meeting text for the model. `source` is "transcript", "summary" or "notes".
#[derive(Debug, Clone, PartialEq)]
pub struct Excerpt {
    pub id: String, /* "E1".. */
    pub meeting_id: String,
    pub meeting_title: String,
    pub date: String,
    pub transcript_id: Option<String>,
    pub audio_start: Option<f64>,
    pub source: String,
    pub text: String,
}

const KEYWORD_MAX_CHARS: usize = 60;

/// Parses the model's plan. The JSON may sit inside a code fence or prose, so the outermost object is used.
/// Keywords are trimmed, deduped case-insensitively, cut to 60 chars and capped; dates must be YYYY-MM-DD.
pub fn parse_plan(raw: &str) -> Result<SearchPlan, String> {
    let json = raw.find('{').zip(raw.rfind('}')).and_then(|(start, end)| raw.get(start..=end)).unwrap_or("");
    let mut plan: SearchPlan = serde_json::from_str(json).map_err(|_| "Search plan is not valid JSON".to_string())?;
    let mut seen = HashSet::new();
    plan.keywords = plan.keywords.iter()
        .map(|k| k.trim().chars().take(KEYWORD_MAX_CHARS).collect::<String>().trim_end().to_string())
        .filter(|k| !k.is_empty() && seen.insert(k.to_lowercase()))
        .take(ASK_MAX_KEYWORDS)
        .collect();
    plan.types = plan.types.iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
    plan.from = valid_day(plan.from.as_deref());
    plan.to = valid_day(plan.to.as_deref());
    plan.person = plan.person.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
    Ok(plan)
}

fn valid_day(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim).filter(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok()).map(str::to_string)
}

/// Excerpts for every keyword that passes the plan's filters, ranked by distinct keywords, then newer meeting.
/// Without keywords nothing matches, so the result is empty.
pub async fn retrieve(pool: &SqlitePool, plan: &SearchPlan) -> Result<Vec<Excerpt>, sqlx::Error> {
    let mut kws: Vec<String> = plan.keywords.iter().map(|k| k.trim().to_lowercase()).filter(|k| !k.is_empty()).collect();
    kws.sort();
    kws.dedup();
    if kws.is_empty() {
        return Ok(Vec::new());
    }
    let from = plan.from.as_deref().and_then(|d| d.parse::<NaiveDate>().ok());
    let to = plan.to.as_deref().and_then(|d| d.parse::<NaiveDate>().ok());
    let person = plan.person.as_deref().map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty());
    let meetings = if plan.types.is_empty() && person.is_none() { HashMap::new() } else { load_meetings(pool).await? };

    let mut lines: HashMap<String, Vec<Line>> = HashMap::new();
    let mut seen: HashSet<(String, &'static str, String)> = HashSet::new();
    let mut scored: Vec<Scored> = Vec::new();
    for keyword in plan.keywords.iter().map(|k| k.trim()).filter(|k| !k.is_empty()) {
        let needle = keyword.to_lowercase();
        for m in source_matches(pool, keyword).await? {
            let when = parse_created(&m.created_at);
            let day = when.map(|w| w.with_timezone(&Local).date_naive());
            if from.is_some() || to.is_some() {
                let Some(day) = day else { continue };
                if from.is_some_and(|f| day < f) || to.is_some_and(|t| day > t) {
                    continue;
                }
            }
            let meta = meetings.get(&m.meeting_id);
            if !plan.types.is_empty() && !meta.is_some_and(|x| is_type(&plan.types, x)) {
                continue;
            }
            let (text, speaker) = match m.source {
                "transcript" => {
                    if !lines.contains_key(&m.meeting_id) {
                        let loaded = meeting_lines(pool, &m.meeting_id).await?;
                        lines.insert(m.meeting_id.clone(), loaded);
                    }
                    let all = &lines[&m.meeting_id];
                    let Some(pos) = all.iter().position(|l| Some(&l.id) == m.transcript_id.as_ref()) else { continue };
                    let context = &all[pos.saturating_sub(2)..(pos + 3).min(all.len())];
                    (context.iter().map(format_line).collect::<Vec<_>>().join("\n"), all[pos].name.clone())
                }
                "summary" => match summary_section(&m.text, &needle) {
                    Some(section) => (section, None),
                    None => continue,
                },
                "notes" => (m.text.clone(), None),
                _ => continue, // A title match has no excerpt text of its own
            };
            if let Some(p) = &person {
                let by_speaker = speaker.as_deref().is_some_and(|s| s.to_lowercase().contains(p));
                let by_meeting = m.title.to_lowercase().contains(p)
                    || meta.and_then(|x| x.summary.as_deref()).is_some_and(|s| s.to_lowercase().contains(p));
                if !by_speaker && !by_meeting {
                    continue;
                }
            }
            let anchor = m.transcript_id.clone().unwrap_or_else(|| text.clone());
            if !seen.insert((m.meeting_id.clone(), m.source, anchor)) {
                continue;
            }
            scored.push(Scored {
                excerpt: Excerpt {
                    id: String::new(), meeting_id: m.meeting_id, meeting_title: m.title,
                    date: day.map(|d| d.to_string()).unwrap_or_default(), transcript_id: m.transcript_id,
                    audio_start: m.audio_start, source: m.source.into(), text,
                },
                score: 0,
                when,
            });
        }
    }

    for s in &mut scored {
        let lower = s.excerpt.text.to_lowercase();
        s.score = kws.iter().filter(|k| lower.contains(k.as_str())).count();
    }
    scored.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| b.when.cmp(&a.when)));

    // Greedy by rank: an excerpt that would overflow the character budget is skipped, and smaller ones after it may still fit
    let mut out = Vec::new();
    let mut chars = 0;
    for s in scored {
        if out.len() == ASK_MAX_EXCERPTS {
            break;
        }
        let n = s.excerpt.text.chars().count();
        if chars + n > ASK_MAX_EXCERPT_CHARS {
            continue;
        }
        chars += n;
        out.push(s.excerpt);
    }
    for (i, e) in out.iter_mut().enumerate() {
        e.id = format!("E{}", i + 1);
    }
    Ok(out)
}

struct Scored {
    excerpt: Excerpt,
    score: usize,
    when: Option<DateTime<FixedOffset>>,
}

/// One transcript line of a meeting, in time order, with its speaker's saved name.
struct Line {
    id: String,
    start: Option<f64>,
    idx: Option<i64>,
    name: Option<String>,
    text: String,
}

/// Per-meeting facts the filters need: the summary's template and its markdown.
struct Meta {
    template_id: Option<String>,
    template_name: Option<String>,
    summary: Option<String>,
}

fn parse_created(created_at: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(created_at).ok()
}

async fn load_meetings(pool: &SqlitePool) -> Result<HashMap<String, Meta>, sqlx::Error> {
    let rows: Vec<(String, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT m.id, json_extract(s.result, '$.template_id'), json_extract(s.result, '$.template_name'), json_extract(s.result, '$.markdown')
         FROM meetings m LEFT JOIN summary_processes s ON s.meeting_id = m.id AND s.status = 'completed'",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id, template_id, template_name, summary)| (id, Meta { template_id, template_name, summary })).collect())
}

fn is_type(types: &[String], meta: &Meta) -> bool {
    types.iter().any(|t| {
        let t = t.trim().to_lowercase();
        [&meta.template_id, &meta.template_name].iter().any(|v| v.as_deref().is_some_and(|v| v.to_lowercase() == t))
    })
}

async fn meeting_lines(pool: &SqlitePool, meeting_id: &str) -> Result<Vec<Line>, sqlx::Error> {
    let rows: Vec<(String, Option<f64>, Option<i64>, Option<String>, String)> = sqlx::query_as(
        "SELECT t.id, t.audio_start_time, t.speaker_idx, ms.name, t.transcript FROM transcripts t
         LEFT JOIN meeting_speakers ms ON ms.meeting_id = t.meeting_id AND ms.idx = t.speaker_idx
         WHERE t.meeting_id = ? ORDER BY t.audio_start_time",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id, start, idx, name, text)| Line { id, start, idx, name, text }).collect())
}

/// `[mm:ss] Name: text`. Speaker 0 is "You", an unnamed speaker is "Speaker n", a NULL speaker has no label.
fn format_line(line: &Line) -> String {
    let stamp = line.start.map(|s| {
        let t = s.max(0.0) as u64;
        format!("[{:02}:{:02}] ", t / 60, t % 60)
    }).unwrap_or_default();
    match line.idx {
        None => format!("{stamp}{}", line.text),
        Some(idx) => {
            let who = if idx == 0 { "You".to_string() } else { line.name.clone().unwrap_or_else(|| format!("Speaker {idx}")) };
            format!("{stamp}{who}: {}", line.text)
        }
    }
}

/// The markdown section, from a heading to the next heading, that holds `needle` (lowercase).
fn summary_section(markdown: &str, needle: &str) -> Option<String> {
    let mut sections: Vec<String> = Vec::new();
    for line in markdown.lines() {
        if line.starts_with('#') || sections.is_empty() {
            sections.push(String::new());
        }
        if let Some(current) = sections.last_mut() {
            current.push_str(line);
            current.push('\n');
        }
    }
    sections.into_iter().find(|s| s.to_lowercase().contains(needle)).map(|s| s.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::TranscriptSegment;
    use crate::database::repositories::test_pool;
    use crate::database::repositories::transcript::TranscriptsRepository;

    fn seg(text: &str, start: f64, speaker: Option<i64>) -> TranscriptSegment {
        TranscriptSegment {
            id: "x".into(), text: text.into(), timestamp: "t".into(),
            audio_start_time: Some(start), audio_end_time: Some(start + 1.0), duration: Some(1.0),
            mark: None, sentences: None, speaker_idx: speaker,
        }
    }

    /// Saves a meeting and dates it at noon UTC, so its local day is the same day in any zone within 12 hours.
    async fn meeting(pool: &SqlitePool, title: &str, day: &str, lines: Vec<TranscriptSegment>) -> String {
        let id = TranscriptsRepository::save_transcript(pool, title, &lines, None, &[], false).await.unwrap();
        sqlx::query("UPDATE meetings SET created_at = ? WHERE id = ?")
            .bind(format!("{day}T12:00:00+00:00")).bind(&id).execute(pool).await.unwrap();
        id
    }

    async fn name(pool: &SqlitePool, meeting_id: &str, idx: i64, name: &str) {
        sqlx::query("INSERT INTO meeting_speakers (meeting_id, idx, name, seconds) VALUES (?, ?, ?, 0.0)")
            .bind(meeting_id).bind(idx).bind(name).execute(pool).await.unwrap();
    }

    async fn summary(pool: &SqlitePool, meeting_id: &str, template_id: &str, template_name: &str, markdown: &str) {
        let result = serde_json::json!({ "markdown": markdown, "template_id": template_id, "template_name": template_name }).to_string();
        sqlx::query("INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, result) VALUES (?, 'completed', '2026-10-22T00:00:00Z', '2026-10-22T00:00:00Z', ?)")
            .bind(meeting_id).bind(result).execute(pool).await.unwrap();
    }

    fn plan(keywords: &[&str]) -> SearchPlan {
        SearchPlan { keywords: keywords.iter().map(|k| k.to_string()).collect(), ..Default::default() }
    }

    fn titles(hits: &[Excerpt]) -> Vec<String> {
        let mut t: Vec<String> = hits.iter().map(|e| e.meeting_title.clone()).collect();
        t.sort();
        t.dedup();
        t
    }

    #[test]
    fn parse_plan_fenced_and_capped() {
        let mut keywords = vec![" Задача ".to_string(), "задача".into(), "".into(), "бюджет".into(), "a".repeat(70)];
        keywords.extend((0..20).map(|i| format!("kw{i}")));
        let raw = format!("Plan:\n```json\n{}\n```", serde_json::json!({
            "keywords": keywords, "types": ["standup"], "from": "2026-10-01", "to": "2026-13-45", "person": "  Иван "
        }));
        let plan = parse_plan(&raw).unwrap();
        assert_eq!(plan.keywords.len(), ASK_MAX_KEYWORDS);
        assert_eq!(plan.keywords[0], "Задача");
        assert_eq!(plan.keywords[1], "бюджет");
        assert_eq!(plan.keywords[2], "a".repeat(60));
        assert_eq!(plan.keywords[3], "kw0");
        assert_eq!(plan.keywords[11], "kw8");
        assert_eq!(plan.types, vec!["standup"]);
        assert_eq!(plan.from.as_deref(), Some("2026-10-01"));
        assert_eq!(plan.to, None);
        assert_eq!(plan.person.as_deref(), Some("Иван"));
        assert!(parse_plan("not a plan").is_err());
    }

    #[tokio::test]
    async fn retrieve_ranks_by_distinct_keywords() {
        let pool = test_pool().await;
        meeting(&pool, "Old", "2026-10-01", vec![seg("Обсудили бюджет", 0.0, None)]).await;
        meeting(&pool, "Newer", "2026-10-21", vec![seg("Сроки сдвинули", 0.0, None)]).await;
        meeting(&pool, "New", "2026-10-20", vec![seg("Обсудили бюджет и сроки", 0.0, None)]).await;
        let hits = retrieve(&pool, &plan(&["бюджет", "сроки"])).await.unwrap();
        let order: Vec<&str> = hits.iter().map(|e| e.meeting_title.as_str()).collect();
        assert_eq!(order, ["New", "Newer", "Old"]);
        let ids: Vec<&str> = hits.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["E1", "E2", "E3"]);
    }

    #[tokio::test]
    async fn retrieve_filters_type_date_person() {
        let pool = test_pool().await;
        let sync = meeting(&pool, "Sync", "2026-10-05", vec![seg("бюджет обсудим", 0.0, Some(1))]).await;
        name(&pool, &sync, 1, "Анна").await;
        summary(&pool, &sync, "standard_meeting", "Standard meeting", "# Итоги\nВсе ок").await;
        let review = meeting(&pool, "Review", "2026-10-15", vec![seg("бюджет одобрен", 0.0, Some(2))]).await;
        name(&pool, &review, 2, "Борис").await;
        summary(&pool, &review, "weekly_review", "Weekly review", "# Итоги\nСогласовали план").await;
        let ivan = meeting(&pool, "Встреча с Иваном", "2026-10-12", vec![seg("бюджет по проекту", 0.0, Some(2))]).await;
        name(&pool, &ivan, 2, "Борис").await;
        let call = meeting(&pool, "Call", "2026-10-18", vec![seg("бюджет", 0.0, Some(1))]).await;
        name(&pool, &call, 1, "Анна").await;

        // Type matches the template id or name, case-insensitively
        let hits = retrieve(&pool, &SearchPlan { types: vec!["WEEKLY_REVIEW".into()], ..plan(&["бюджет"]) }).await.unwrap();
        assert_eq!(titles(&hits), ["Review"]);
        let hits = retrieve(&pool, &SearchPlan { types: vec!["standard meeting".into()], ..plan(&["бюджет"]) }).await.unwrap();
        assert_eq!(titles(&hits), ["Sync"]);

        // Dates are local days, inclusive
        let hits = retrieve(&pool, &SearchPlan { from: Some("2026-10-10".into()), to: Some("2026-10-15".into()), ..plan(&["бюджет"]) }).await.unwrap();
        assert_eq!(titles(&hits), ["Review", "Встреча с Иваном"]);

        // Person: speaker name, case-folded, or the meeting title
        let hits = retrieve(&pool, &SearchPlan { person: Some("АННА".into()), ..plan(&["бюджет"]) }).await.unwrap();
        assert_eq!(titles(&hits), ["Call", "Sync"]);
        let hits = retrieve(&pool, &SearchPlan { person: Some("иваном".into()), ..plan(&["бюджет"]) }).await.unwrap();
        assert_eq!(titles(&hits), ["Встреча с Иваном"]);
        let hits = retrieve(&pool, &SearchPlan { person: Some("анна".into()), from: Some("2026-10-10".into()), ..plan(&["бюджет"]) }).await.unwrap();
        assert_eq!(titles(&hits), ["Call"]);
    }

    #[tokio::test]
    async fn transcript_excerpt_has_context_and_speakers() {
        let pool = test_pool().await;
        let id = meeting(&pool, "Talk", "2026-10-05", vec![
            seg("Первая строка", 0.0, Some(0)),
            seg("Вторая строка", 65.0, Some(1)),
            seg("Ключевой бюджет здесь", 130.0, Some(2)),
            seg("Четвертая", 200.0, None),
            seg("Пятая", 210.0, Some(1)),
            seg("Шестая", 220.0, Some(1)),
        ]).await;
        name(&pool, &id, 1, "Анна").await;
        let hits = retrieve(&pool, &plan(&["бюджет"])).await.unwrap();
        assert_eq!(hits.len(), 1);
        let e = &hits[0];
        assert_eq!(e.source, "transcript");
        assert_eq!(e.audio_start, Some(130.0));
        assert!(e.transcript_id.is_some());
        assert_eq!(e.date, "2026-10-05");
        assert_eq!(e.text, "[00:00] You: Первая строка\n[01:05] Анна: Вторая строка\n[02:10] Speaker 2: Ключевой бюджет здесь\n[03:20] Четвертая\n[03:30] Анна: Пятая");
    }

    #[tokio::test]
    async fn caps_count_and_chars() {
        // Count cap: 35 matches, 30 kept with ids E1..E30
        let pool = test_pool().await;
        for i in 0..35 {
            meeting(&pool, &format!("M{i:02}"), "2026-10-05", vec![seg(&format!("бюджет {i}"), 0.0, None)]).await;
        }
        let hits = retrieve(&pool, &plan(&["бюджет"])).await.unwrap();
        assert_eq!(hits.len(), ASK_MAX_EXCERPTS);
        assert_eq!(hits[0].id, "E1");
        assert_eq!(hits[ASK_MAX_EXCERPTS - 1].id, "E30");

        // Char cap: each excerpt is about 2017 chars, so 19 fit in 40_000
        let pool = test_pool().await;
        for i in 0..25 {
            meeting(&pool, &format!("L{i:02}"), "2026-10-05", vec![seg(&format!("параметр {}", "я".repeat(2000)), 0.0, None)]).await;
        }
        let hits = retrieve(&pool, &plan(&["параметр"])).await.unwrap();
        let total: usize = hits.iter().map(|e| e.text.chars().count()).sum();
        assert!(total <= ASK_MAX_EXCERPT_CHARS);
        assert_eq!(hits.len(), 19);
    }

    #[tokio::test]
    async fn empty_plan_empty_result() {
        let pool = test_pool().await;
        meeting(&pool, "M", "2026-10-05", vec![seg("бюджет", 0.0, None)]).await;
        assert!(retrieve(&pool, &SearchPlan::default()).await.unwrap().is_empty());
        assert!(retrieve(&pool, &plan(&["   "])).await.unwrap().is_empty());
        // Filters alone have no keyword to match, so nothing is returned
        let filters_only = SearchPlan { person: Some("Анна".into()), types: vec!["Standard meeting".into()], ..Default::default() };
        assert!(retrieve(&pool, &filters_only).await.unwrap().is_empty());
    }
}
