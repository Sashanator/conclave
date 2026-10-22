//! Keyword retrieval for Ask. Matching comes from `transcript::source_matches`; this module applies
//! the plan's filters, builds the excerpts and ranks them.

use super::{ASK_MAX_EXCERPTS, ASK_MAX_EXCERPT_CHARS, ASK_MAX_KEYWORDS};
use crate::database::models::DateTimeUtc;
use crate::database::repositories::transcript::source_matches;
use chrono::{DateTime, Local, NaiveDate, Utc};
use serde::{Deserialize, Deserializer};
use sqlx::SqlitePool;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// What the model asked for. Empty, missing or null fields mean "no filter".
#[derive(Deserialize, Debug, PartialEq, Default)]
pub struct SearchPlan {
    #[serde(default, deserialize_with = "null_as_empty")]
    pub keywords: Vec<String>,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub types: Vec<String>,
    pub from: Option<String>, /* YYYY-MM-DD */
    pub to: Option<String>,
    pub person: Option<String>,
}

fn null_as_empty<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
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
/// A text excerpt gets an equal share of the character budget, so a full set always fits.
const SUMMARY_SHARE_CHARS: usize = ASK_MAX_EXCERPT_CHARS / ASK_MAX_EXCERPTS;
/// Transcript lines around a matched or named line, on each side.
const CONTEXT_LINES: usize = 2;
/// Lines of a named person taken per meeting when there are no keywords.
const PERSON_LINES_PER_MEETING: usize = 3;

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

/// Excerpts that pass the plan's filters, ranked by distinct keywords hit, then newer meeting.
/// With keywords, each keyword's matches are excerpts. With filters only, the summaries of the matching
/// meetings come first, then (with `person`) that person's lines. A plan with neither returns nothing.
pub async fn retrieve(pool: &SqlitePool, plan: &SearchPlan) -> Result<Vec<Excerpt>, sqlx::Error> {
    let mut kws: Vec<String> = plan.keywords.iter().map(|k| k.trim().to_lowercase()).filter(|k| !k.is_empty()).collect();
    kws.sort();
    kws.dedup();
    let keywords: Vec<&str> = plan.keywords.iter().map(|k| k.trim()).filter(|k| !k.is_empty()).collect();
    let from = plan.from.as_deref().and_then(|d| d.parse::<NaiveDate>().ok());
    let to = plan.to.as_deref().and_then(|d| d.parse::<NaiveDate>().ok());
    let person = plan.person.as_deref().map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty());
    let filtered = !plan.types.is_empty() || from.is_some() || to.is_some() || person.is_some();
    if keywords.is_empty() && !filtered {
        return Ok(Vec::new());
    }

    let meetings = load_meetings(pool).await?;
    let speakers = match &person {
        Some(p) => person_speakers(pool, p).await?,
        None => HashMap::new(),
    };
    let filters = Filters { from, to, types: plan.types.clone(), person, speakers };
    let mut col = Collector::default();
    let mut lines: HashMap<String, Lines> = HashMap::new();
    if keywords.is_empty() {
        filter_only(pool, &meetings, &filters, &mut col, &mut lines).await?;
    } else {
        keyword_matches(pool, &keywords, &meetings, &filters, &mut col, &mut lines).await?;
    }

    // Score: distinct keywords found in the text or that led to the excerpt
    for s in &mut col.scored {
        let lower = s.excerpt.text.to_lowercase();
        s.score = kws.iter().filter(|k| s.hits.contains(*k) || lower.contains(k.as_str())).count();
    }
    col.scored.sort_by(|a, b| {
        b.score.cmp(&a.score)
            .then_with(|| a.rank.cmp(&b.rank))
            .then_with(|| b.when.cmp(&a.when))
            .then_with(|| a.excerpt.meeting_id.cmp(&b.excerpt.meeting_id))
            .then_with(|| a.excerpt.audio_start.partial_cmp(&b.excerpt.audio_start).unwrap_or(Ordering::Equal))
    });

    // Greedy by rank: an excerpt that would overflow the character budget is skipped, and smaller ones after it may still fit
    let mut out = Vec::new();
    let mut chars = 0;
    for s in col.scored {
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

/// Excerpts from every keyword's source matches. A title-only hit becomes that meeting's summary.
async fn keyword_matches(
    pool: &SqlitePool,
    keywords: &[&str],
    meetings: &HashMap<String, Meta>,
    filters: &Filters,
    col: &mut Collector,
    lines: &mut HashMap<String, Lines>,
) -> Result<(), sqlx::Error> {
    let mut title_hits: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for &keyword in keywords {
        let needle = keyword.to_lowercase();
        for m in source_matches(pool, keyword).await? {
            let Some(meta) = meetings.get(&m.meeting_id) else { continue };
            if !filters.meeting_ok(meta) {
                continue;
            }
            let text = match m.source {
                "title" => {
                    title_hits.entry(m.meeting_id.clone()).or_default().push(needle.clone());
                    continue;
                }
                "transcript" => {
                    let of = meeting_lines(pool, lines, &m.meeting_id).await?;
                    let Some(&pos) = m.transcript_id.as_ref().and_then(|t| of.pos.get(t)) else { continue };
                    if !filters.person_in_line(meta, of.all[pos].name.as_deref()) {
                        continue;
                    }
                    let (from, to) = context_window(of.all.len(), pos);
                    col.add(lines_excerpt(&m.meeting_id, meta, &of.all, from, to, pos), meta.when, std::slice::from_ref(&needle));
                    continue;
                }
                "summary" => {
                    if !filters.person_in_meeting(&m.meeting_id, meta) {
                        continue;
                    }
                    match summary_section(&m.text, &needle) {
                        Some(section) => cut_chars(&section, SUMMARY_SHARE_CHARS),
                        None => continue,
                    }
                }
                "notes" => {
                    if !filters.person_in_meeting(&m.meeting_id, meta) {
                        continue;
                    }
                    cut_chars(&m.text, SUMMARY_SHARE_CHARS)
                }
                _ => continue,
            };
            col.add(excerpt_of(&m.meeting_id, meta, m.source, text), meta.when, std::slice::from_ref(&needle));
        }
    }

    for (meeting_id, hits) in title_hits {
        let Some(meta) = meetings.get(&meeting_id) else { continue };
        if !filters.person_in_meeting(&meeting_id, meta) {
            continue;
        }
        // A summary already in the results takes the title hits; otherwise the whole summary is the excerpt
        match col.summary_of.get(&meeting_id).copied() {
            Some(idx) => col.scored[idx].hits.extend(hits),
            None => {
                if let Some(md) = meta.summary.as_deref() {
                    col.add(excerpt_of(&meeting_id, meta, "summary", cut_chars(md, SUMMARY_SHARE_CHARS)), meta.when, &hits);
                }
            }
        }
    }
    Ok(())
}

/// No keywords: the summaries of the meetings that pass the filters, and the lines of a named person.
async fn filter_only(
    pool: &SqlitePool,
    meetings: &HashMap<String, Meta>,
    filters: &Filters,
    col: &mut Collector,
    lines: &mut HashMap<String, Lines>,
) -> Result<(), sqlx::Error> {
    for (meeting_id, meta) in meetings {
        let Some(md) = meta.summary.as_deref() else { continue };
        if !filters.meeting_ok(meta) || !filters.person_in_meeting(meeting_id, meta) {
            continue;
        }
        col.add(excerpt_of(meeting_id, meta, "summary", cut_chars(md, SUMMARY_SHARE_CHARS)), meta.when, &[]);
    }
    for (meeting_id, idxs) in &filters.speakers {
        let Some(meta) = meetings.get(meeting_id) else { continue };
        if !filters.meeting_ok(meta) {
            continue;
        }
        let of = meeting_lines(pool, lines, meeting_id).await?;
        let picked: Vec<usize> = of.all.iter().enumerate()
            .filter(|(_, l)| l.idx.is_some_and(|i| idxs.contains(&i)))
            .map(|(p, _)| p)
            .take(PERSON_LINES_PER_MEETING)
            .collect();
        // Lines whose context windows share a line become one excerpt, so no line repeats
        let mut groups: Vec<(usize, usize)> = Vec::new();
        for p in picked {
            match groups.last_mut() {
                Some(g) if p <= g.1 + 2 * CONTEXT_LINES => g.1 = p,
                _ => groups.push((p, p)),
            }
        }
        for (first, last) in groups {
            let from = context_window(of.all.len(), first).0;
            let to = context_window(of.all.len(), last).1;
            let idx = col.add(lines_excerpt(meeting_id, meta, &of.all, from, to, first), meta.when, &[]);
            col.scored[idx].rank = 1;
        }
    }
    Ok(())
}

/// Excerpts found so far. The same excerpt from two keywords is kept once and carries both hits.
#[derive(Default)]
struct Collector {
    scored: Vec<Scored>,
    seen: HashMap<(String, String, String), usize>,
    /// The first summary excerpt of each meeting, which title hits merge into
    summary_of: HashMap<String, usize>,
}

impl Collector {
    fn add(&mut self, excerpt: Excerpt, when: DateTime<Utc>, hits: &[String]) -> usize {
        let anchor = excerpt.transcript_id.clone().unwrap_or_else(|| excerpt.text.clone());
        let key = (excerpt.meeting_id.clone(), excerpt.source.clone(), anchor);
        let idx = match self.seen.get(&key) {
            Some(&idx) => idx,
            None => {
                let idx = self.scored.len();
                if excerpt.source == "summary" {
                    self.summary_of.entry(excerpt.meeting_id.clone()).or_insert(idx);
                }
                self.seen.insert(key, idx);
                self.scored.push(Scored { excerpt, score: 0, rank: 0, when, hits: BTreeSet::new() });
                idx
            }
        };
        self.scored[idx].hits.extend(hits.iter().cloned());
        idx
    }
}

struct Scored {
    excerpt: Excerpt,
    score: usize,
    /// 0 for keyword matches and summaries; 1 for a named person's lines (filter-only plans), which sort after summaries
    rank: u8,
    when: DateTime<Utc>,
    /// Keywords that led to this excerpt, lowercase
    hits: BTreeSet<String>,
}

/// The plan's filters. `speakers` maps each meeting to the speaker indexes whose name contains `person`.
struct Filters {
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    types: Vec<String>,
    /// Lowercase
    person: Option<String>,
    speakers: HashMap<String, HashSet<i64>>,
}

impl Filters {
    /// The local date range and the template type.
    fn meeting_ok(&self, meta: &Meta) -> bool {
        if self.from.is_some_and(|f| meta.day < f) || self.to.is_some_and(|t| meta.day > t) {
            return false;
        }
        self.types.is_empty() || is_type(&self.types, meta)
    }

    /// The person is in the meeting: its title, its summary, or a speaker named so.
    fn person_in_meeting(&self, meeting_id: &str, meta: &Meta) -> bool {
        let Some(p) = &self.person else { return true };
        mentions(meta, p) || self.speakers.contains_key(meeting_id)
    }

    /// The line is the person's: its speaker is named so, or the meeting's title or summary names them.
    fn person_in_line(&self, meta: &Meta, speaker: Option<&str>) -> bool {
        let Some(p) = &self.person else { return true };
        speaker.is_some_and(|s| s.to_lowercase().contains(p)) || mentions(meta, p)
    }
}

/// Lowercase `p` appears in the meeting's title or summary.
fn mentions(meta: &Meta, p: &str) -> bool {
    meta.title.to_lowercase().contains(p) || meta.summary.as_deref().is_some_and(|s| s.to_lowercase().contains(p))
}

/// Meeting-level facts the filters and excerpts need.
struct Meta {
    title: String,
    when: DateTime<Utc>,
    /// The local calendar day of `when`
    day: NaiveDate,
    template_id: Option<String>,
    template_name: Option<String>,
    summary: Option<String>,
}

/// One transcript line of a meeting, in time order, with its speaker's saved name.
struct Line {
    id: String,
    start: Option<f64>,
    idx: Option<i64>,
    name: Option<String>,
    text: String,
}

/// A meeting's transcript lines in time order, and each line's position by transcript id.
struct Lines {
    all: Vec<Line>,
    pos: HashMap<String, usize>,
}

async fn load_meetings(pool: &SqlitePool) -> Result<HashMap<String, Meta>, sqlx::Error> {
    let rows: Vec<(String, String, DateTimeUtc, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT m.id, m.title, m.created_at, json_extract(s.result, '$.template_id'), json_extract(s.result, '$.template_name'), json_extract(s.result, '$.markdown')
         FROM meetings m LEFT JOIN summary_processes s ON s.meeting_id = m.id AND s.status = 'completed'",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id, title, created_at, template_id, template_name, summary)| {
        let when = created_at.0;
        let day = when.with_timezone(&Local).date_naive();
        (id, Meta { title, when, day, template_id, template_name, summary })
    }).collect())
}

/// Speaker indexes per meeting whose saved name contains `person` (lowercase).
async fn person_speakers(pool: &SqlitePool, person: &str) -> Result<HashMap<String, HashSet<i64>>, sqlx::Error> {
    let rows: Vec<(String, i64, String)> =
        sqlx::query_as("SELECT meeting_id, idx, name FROM meeting_speakers WHERE name IS NOT NULL").fetch_all(pool).await?;
    let mut out: HashMap<String, HashSet<i64>> = HashMap::new();
    for (meeting_id, idx, name) in rows {
        if name.to_lowercase().contains(person) {
            out.entry(meeting_id).or_default().insert(idx);
        }
    }
    Ok(out)
}

fn is_type(types: &[String], meta: &Meta) -> bool {
    types.iter().any(|t| {
        let t = t.trim().to_lowercase();
        [&meta.template_id, &meta.template_name].iter().any(|v| v.as_deref().is_some_and(|v| v.to_lowercase() == t))
    })
}

/// The meeting's transcript lines, loaded once per call.
async fn meeting_lines<'a>(
    pool: &SqlitePool,
    cache: &'a mut HashMap<String, Lines>,
    meeting_id: &str,
) -> Result<&'a Lines, sqlx::Error> {
    if !cache.contains_key(meeting_id) {
        let rows: Vec<(String, Option<f64>, Option<i64>, Option<String>, String)> = sqlx::query_as(
            "SELECT t.id, t.audio_start_time, t.speaker_idx, ms.name, t.transcript FROM transcripts t
             LEFT JOIN meeting_speakers ms ON ms.meeting_id = t.meeting_id AND ms.idx = t.speaker_idx
             WHERE t.meeting_id = ? ORDER BY t.audio_start_time",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;
        let all: Vec<Line> = rows.into_iter().map(|(id, start, idx, name, text)| Line { id, start, idx, name, text }).collect();
        let pos = all.iter().enumerate().map(|(p, l)| (l.id.clone(), p)).collect();
        cache.insert(meeting_id.to_string(), Lines { all, pos });
    }
    Ok(&cache[meeting_id])
}

fn excerpt_of(meeting_id: &str, meta: &Meta, source: &str, text: String) -> Excerpt {
    Excerpt {
        id: String::new(),
        meeting_id: meeting_id.to_string(),
        meeting_title: meta.title.clone(),
        date: meta.day.to_string(),
        transcript_id: None,
        audio_start: None,
        source: source.to_string(),
        text,
    }
}

/// The lines `from..to` of a meeting as one excerpt. `anchor` is the line it is about, for id and start.
fn lines_excerpt(meeting_id: &str, meta: &Meta, all: &[Line], from: usize, to: usize, anchor: usize) -> Excerpt {
    let text = all[from..to].iter().map(format_line).collect::<Vec<_>>().join("\n");
    Excerpt {
        transcript_id: Some(all[anchor].id.clone()),
        audio_start: all[anchor].start,
        ..excerpt_of(meeting_id, meta, "transcript", text)
    }
}

/// The half-open range of lines around `pos`, clipped to the meeting.
fn context_window(len: usize, pos: usize) -> (usize, usize) {
    (pos.saturating_sub(CONTEXT_LINES), (pos + CONTEXT_LINES + 1).min(len))
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

/// The first `max` characters, cut on a character boundary.
fn cut_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
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
        // Explicit nulls mean "no value"
        let nulls = parse_plan(r#"{"keywords": null, "types": null, "from": null, "person": null}"#).unwrap();
        assert_eq!(nulls, SearchPlan::default());
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

        // Filter-only summaries are cut to their share, so 30 fit the budget
        let pool = test_pool().await;
        for i in 0..35 {
            let id = meeting(&pool, &format!("S{i:02}"), "2026-10-05", vec![seg("x", 0.0, None)]).await;
            summary(&pool, &id, "standard_meeting", "Standard meeting", &"я".repeat(3000)).await;
        }
        let hits = retrieve(&pool, &SearchPlan { from: Some("2026-10-01".into()), ..Default::default() }).await.unwrap();
        let total: usize = hits.iter().map(|e| e.text.chars().count()).sum();
        assert_eq!(hits.len(), ASK_MAX_EXCERPTS);
        assert!(total <= ASK_MAX_EXCERPT_CHARS);
    }

    #[tokio::test]
    async fn empty_plan_empty_result() {
        let pool = test_pool().await;
        meeting(&pool, "M", "2026-10-05", vec![seg("бюджет", 0.0, None)]).await;
        assert!(retrieve(&pool, &SearchPlan::default()).await.unwrap().is_empty());
        assert!(retrieve(&pool, &plan(&["   "])).await.unwrap().is_empty());
        // Filters alone return summaries, and this meeting has none
        let filters_only = SearchPlan { person: Some("Анна".into()), types: vec!["Standard meeting".into()], ..Default::default() };
        assert!(retrieve(&pool, &filters_only).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn filter_only_plan_returns_summaries() {
        let pool = test_pool().await;
        let sync = meeting(&pool, "Sync", "2026-10-05", vec![seg("обсудили план", 0.0, Some(1))]).await;
        name(&pool, &sync, 1, "Анна").await;
        summary(&pool, &sync, "standard_meeting", "Standard meeting", "# Итоги\nВсе ок").await;
        let review = meeting(&pool, "Review", "2026-10-15", vec![seg("Пункт", 0.0, Some(2))]).await;
        name(&pool, &review, 2, "Борис").await;
        summary(&pool, &review, "weekly_review", "Weekly review", "# Итоги\nСогласовали").await;
        // No summary, so a filter-only plan cannot return it
        let ivan = meeting(&pool, "Встреча с Иваном", "2026-10-12", vec![seg("Иван говорит", 0.0, Some(2))]).await;
        name(&pool, &ivan, 2, "Борис").await;
        let call = meeting(&pool, "Call", "2026-10-18", vec![seg("Ок", 0.0, Some(1))]).await;
        name(&pool, &call, 1, "Анна").await;
        summary(&pool, &call, "standard_meeting", "Standard meeting", "# Итоги\nАнна ответила").await;

        // Type only: that template's summary
        let hits = retrieve(&pool, &SearchPlan { types: vec!["weekly review".into()], ..Default::default() }).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].source.as_str(), hits[0].text.as_str()), ("summary", "# Итоги\nСогласовали"));

        // Dates only: every summary in range, newest meeting first
        let hits = retrieve(&pool, &SearchPlan { from: Some("2026-10-01".into()), to: Some("2026-10-31".into()), ..Default::default() }).await.unwrap();
        let order: Vec<(&str, &str)> = hits.iter().map(|e| (e.meeting_title.as_str(), e.source.as_str())).collect();
        assert_eq!(order, [("Call", "summary"), ("Review", "summary"), ("Sync", "summary")]);
        let ids: Vec<&str> = hits.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["E1", "E2", "E3"]);

        // Person only: the summaries of meetings they are in, and their lines with context
        let hits = retrieve(&pool, &SearchPlan { person: Some("АННА".into()), ..Default::default() }).await.unwrap();
        let order: Vec<(&str, &str)> = hits.iter().map(|e| (e.meeting_title.as_str(), e.source.as_str())).collect();
        // Summaries first, newest meeting first; then the person's lines
        assert_eq!(order, [("Call", "summary"), ("Sync", "summary"), ("Call", "transcript"), ("Sync", "transcript")]);
    }

    #[tokio::test]
    async fn title_only_keyword_yields_summary() {
        let pool = test_pool().await;
        // The keyword is in the title only, so its summary is the excerpt
        let a = meeting(&pool, "Бюджетный план", "2026-10-21", vec![seg("Пустая", 0.0, None)]).await;
        summary(&pool, &a, "standard_meeting", "Standard meeting", "# Итоги\nСогласовали план").await;
        meeting(&pool, "Квартал", "2026-10-20", vec![seg("квартал и бюджет", 0.0, None)]).await;
        meeting(&pool, "Созвон", "2026-10-01", vec![seg("бюджет", 0.0, None)]).await;
        // "квартал и бюджет" scores 2; the title hit scores 1 and is newer than "Созвон", which also scores 1
        let hits = retrieve(&pool, &plan(&["бюджет", "квартал"])).await.unwrap();
        let order: Vec<&str> = hits.iter().map(|e| e.meeting_title.as_str()).collect();
        assert_eq!(order, ["Квартал", "Бюджетный план", "Созвон"]);
        assert_eq!((hits[1].source.as_str(), hits[1].text.as_str()), ("summary", "# Итоги\nСогласовали план"));
    }

    #[tokio::test]
    async fn title_match_summary_cut_to_share() {
        let pool = test_pool().await;
        let id = meeting(&pool, "Бюджет", "2026-10-05", vec![seg("Пусто", 0.0, None)]).await;
        summary(&pool, &id, "standard_meeting", "Standard meeting", &format!("# Итоги\n{}", "я".repeat(3000))).await;
        let hits = retrieve(&pool, &plan(&["бюджет"])).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, "summary");
        assert_eq!(hits[0].text.chars().count(), ASK_MAX_EXCERPT_CHARS / ASK_MAX_EXCERPTS);
        assert!(hits[0].text.starts_with("# Итоги"));
    }

    #[tokio::test]
    async fn summary_section_cut_to_share() {
        let pool = test_pool().await;
        let id = meeting(&pool, "Sync", "2026-10-05", vec![seg("Пусто", 0.0, None)]).await;
        summary(&pool, &id, "standard_meeting", "Standard meeting", &format!("бюджет {}", "я".repeat(3000))).await;
        let hits = retrieve(&pool, &plan(&["бюджет"])).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, "summary");
        assert_eq!(hits[0].text.chars().count(), SUMMARY_SHARE_CHARS);
        assert!(hits[0].text.starts_with("бюджет"));
    }

    #[tokio::test]
    async fn legacy_created_at_keeps_its_day() {
        let pool = test_pool().await;
        let id = meeting(&pool, "Legacy", "2026-10-05", vec![seg("бюджет", 0.0, None)]).await;
        sqlx::query("UPDATE meetings SET created_at = '2026-10-05 12:00:00' WHERE id = ?").bind(&id).execute(&pool).await.unwrap();
        let hits = retrieve(&pool, &SearchPlan { from: Some("2026-10-05".into()), to: Some("2026-10-05".into()), ..plan(&["бюджет"]) }).await.unwrap();
        assert_eq!(titles(&hits), ["Legacy"]);
        assert_eq!(hits[0].date, "2026-10-05");
    }

    #[tokio::test]
    async fn person_lines_capped_and_merged() {
        let pool = test_pool().await;
        let old = meeting(&pool, "Old", "2026-10-01", vec![seg("Первая", 0.0, Some(1)), seg("Вторая", 5.0, Some(1))]).await;
        name(&pool, &old, 1, "Анна").await;
        summary(&pool, &old, "standard_meeting", "Standard meeting", "# Итоги\nКоротко").await;
        let mid = meeting(&pool, "Mid", "2026-10-05", vec![seg("Третья", 0.0, Some(1)), seg("Четвертая", 5.0, Some(1))]).await;
        name(&pool, &mid, 1, "Анна").await;
        summary(&pool, &mid, "standard_meeting", "Standard meeting", "# Итоги\nКоротко").await;
        let forty: Vec<TranscriptSegment> = (0..40).map(|i| seg(&format!("строка {i}"), i as f64, Some(1))).collect();
        let new = meeting(&pool, "New", "2026-10-10", forty).await;
        name(&pool, &new, 1, "Анна").await;
        summary(&pool, &new, "standard_meeting", "Standard meeting", "# Итоги\nКоротко").await;

        let hits = retrieve(&pool, &SearchPlan { person: Some("анна".into()), ..Default::default() }).await.unwrap();
        // Every meeting is represented, and the newest contributes a few lines, not all forty
        assert_eq!(titles(&hits), ["Mid", "New", "Old"]);
        let new_text: String = hits.iter().filter(|e| e.meeting_title == "New" && e.source == "transcript").map(|e| e.text.as_str()).collect::<Vec<_>>().join("\n");
        assert!(new_text.contains("строка 0"));
        assert!(!new_text.contains("строка 39"));
        // Overlapping context windows are merged, so no line appears twice
        let lines: Vec<&str> = hits.iter().filter(|e| e.source == "transcript").flat_map(|e| e.text.lines()).collect();
        let unique: HashSet<&str> = lines.iter().copied().collect();
        assert_eq!(unique.len(), lines.len());
    }
}
