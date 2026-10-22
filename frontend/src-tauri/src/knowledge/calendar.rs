// The calendar link: a user-pasted iCal (secret) address, fetched and expanded into the next
// occurrences. The URL is a credential; it is stored by settings, never logged or returned.

use super::settings;
use super::{
    CALENDAR_AHEAD_DAYS, CALENDAR_BEHIND_DAYS, CALENDAR_FETCH_TIMEOUT_SECS, CALENDAR_MAX_BYTES,
    MAX_EVENT_TITLE_CHARS,
};
use chrono::{DateTime, Duration, LocalResult, NaiveDateTime, Offset, TimeZone, Utc};
use icalendar::parser::{read_calendar, unfold, Component, Property};
use log::{info, warn};
use rrule::{RRule, RRuleSet, Tz, Unvalidated};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, PoisonError, RwLock};
use std::time::Instant;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, Runtime};

const CACHE_FILE: &str = "calendar-cache.json";
const UNREADABLE: &str = "Calendar could not be read";
const NOT_CONNECTED: &str = "Calendar is not connected";
const CHANGED: &str = "Calendar changed during the sync";
/// A redirect chain longer than this is refused.
const MAX_REDIRECTS: usize = 5;
/// The parser recurses once per nested component. Real calendars nest 3.
const MAX_NESTING: usize = 16;
/// Stack for the parser thread: the parser recurses, and a feed must never overflow the caller's stack.
const PARSE_STACK_BYTES: usize = 64 * 1024 * 1024;
/// Wall-clock limit for one expansion. Work limits per expansion, in estimated rrule steps (about 1.5 us
/// each): per series (about 0.15 s) and for the whole feed (about 1.5 s), and the occurrences kept.
const FEED_DEADLINE: std::time::Duration = std::time::Duration::from_secs(2);
const MAX_SERIES_WORK: f64 = 100_000.0;
const MAX_FEED_WORK: f64 = 1_000_000.0;
const MAX_FEED_OCCURRENCES: usize = 5_000;
/// A longer UID is skipped; it is copied into every occurrence of its series.
const MAX_UID_CHARS: usize = 256;

/// One meeting instance on the calendar.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Occurrence {
    pub uid: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// What the settings page shows. The host only; the path and query are secrets.
#[derive(Serialize)]
pub struct CalendarStatus {
    pub connected: bool,
    pub host: Option<String>,
    pub last_ok_at: Option<String>,
    pub last_error: Option<String>,
    pub upcoming: usize,
}

/// The persisted part: the last good expansion and when it was made.
#[derive(Serialize, Deserialize, Default)]
struct Cache {
    last_ok_at: Option<String>,
    occurrences: Vec<Occurrence>,
}

#[derive(Default)]
struct State {
    cache: Cache,
    last_error: Option<String>,
}

static STATE: LazyLock<RwLock<State>> = LazyLock::new(|| RwLock::new(State::default()));

/// One VEVENT, read down to the fields expansion needs.
struct Event {
    uid: String,
    title: String,
    recurrence_id: Option<DateTime<Utc>>,
    cancelled: bool,
    start: DateTime<Tz>,
    end: Option<DateTime<Tz>>,
    rrule: Option<String>,
    exdates: Vec<DateTime<Tz>>,
}

/// Pure: parse an iCal body and expand occurrences in [from, to). Skips all-day, cancelled and EXDATE'd instances; applies RECURRENCE-ID overrides. Bounded by the time, work and occurrence limits above.
pub fn expand(
    ics: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Vec<Occurrence>, String> {
    // The parser recurses once per nested component. It runs on a thread with a large stack, so a
    // structure the guard misses cannot overflow the stack of the caller.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(PARSE_STACK_BYTES)
            .spawn_scoped(scope, || expand_here(ics, from, to))
            .map_err(|_| UNREADABLE.to_string())?
            .join()
            .unwrap_or_else(|_| Err(UNREADABLE.to_string()))
    })
}

fn expand_here(
    ics: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Vec<Occurrence>, String> {
    // The parser's error quotes the input, so only fixed messages leave the function.
    let unfolded = normalize(ics);
    if nesting_refused(&unfolded) {
        return Err(UNREADABLE.to_string());
    }
    let calendar = read_calendar(&unfolded).map_err(|_| UNREADABLE.to_string())?;
    let mut unknown_tz_warned = false;
    let events: Vec<Event> = calendar
        .components
        .iter()
        .filter(|c| c.name == "VEVENT")
        .filter_map(|c| event(c, &mut unknown_tz_warned))
        .collect();

    let cancelled_series: HashSet<String> = events
        .iter()
        .filter(|e| e.recurrence_id.is_none() && e.cancelled)
        .map(|e| e.uid.clone())
        .collect();
    let (masters, overrides): (Vec<Event>, Vec<Event>) =
        events.into_iter().partition(|e| e.recurrence_id.is_none());
    // A BTreeMap, so the order of the leftover overrides does not depend on hashing.
    let mut overrides: BTreeMap<(String, DateTime<Utc>), Event> = overrides
        .into_iter()
        .filter_map(|e| Some(((e.uid.clone(), e.recurrence_id?), e)))
        .collect();

    let inside = |t: DateTime<Utc>| t >= from && t < to;
    // Cheapest series first, in file order for ties, so a budget stop drops the expensive ones.
    // A series that starts at or after the window end has nothing inside the window.
    let mut series: Vec<(f64, &Event)> = masters
        .iter()
        .filter(|m| !m.cancelled && m.start.with_timezone(&Utc) < to)
        .map(|m| (work(m, to), m))
        .collect();
    series.sort_by(|a, b| a.0.total_cmp(&b.0));

    let deadline = Instant::now() + FEED_DEADLINE;
    let (mut out, mut spent, mut skipped, mut timed_out) = (Vec::new(), 0.0, 0usize, false);
    'series: for (cost, master) in series {
        if Instant::now() >= deadline {
            timed_out = true;
            break;
        }
        if cost > MAX_SERIES_WORK || spent + cost > MAX_FEED_WORK {
            skipped += 1;
            continue;
        }
        spent += cost;
        for start in starts(master, from, to) {
            if out.len() >= MAX_FEED_OCCURRENCES {
                break 'series;
            }
            let utc = start.with_timezone(&Utc);
            if !inside(utc) {
                continue;
            }
            match overrides.remove(&(master.uid.clone(), utc)) {
                Some(moved) => {
                    let shown = occurrence(&moved);
                    if !moved.cancelled && inside(shown.start) {
                        out.push(shown);
                    }
                }
                None => out.push(Occurrence {
                    uid: master.uid.clone(),
                    title: master.title.clone(),
                    start: utc,
                    end: (start + duration(master)).with_timezone(&Utc),
                }),
            }
        }
    }
    // An override whose instance this feed does not generate (a moved one, or a single edit) stands alone.
    for ((uid, _), moved) in overrides {
        if out.len() >= MAX_FEED_OCCURRENCES {
            break;
        }
        let shown = occurrence(&moved);
        if !moved.cancelled && !cancelled_series.contains(&uid) && inside(shown.start) {
            out.push(shown);
        }
    }
    if timed_out {
        warn!("calendar: expansion stopped at the time limit, the rest of the feed is not shown");
    }
    if skipped > 0 {
        warn!("calendar: {skipped} series skipped to keep the expansion bounded");
    }
    if out.len() >= MAX_FEED_OCCURRENCES {
        warn!("calendar: occurrence limit reached, the feed is cut short");
    }
    out.sort_by_key(|o| o.start);
    Ok(out)
}

/// Unfolds, then turns every line ending into CRLF: the guard splits lines by str::lines and the parser by CRLF.
fn normalize(ics: &str) -> String {
    unfold(ics)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
}

/// Refuses a structure the parser could recurse on too deeply.
fn nesting_refused(unfolded: &str) -> bool {
    guard_depth(unfolded).is_none_or(|d| d > MAX_NESTING)
}

/// The deepest component nesting, or None for input the guard cannot count reliably. Every line must be
/// one the parser reads whole, so its components start and end where the lines say. The parser ends a
/// component by prefix, so `END:ABEGIN:A` closes one component and opens another from the rest of the
/// line: a BEGIN or END line holds a name and nothing else. Tags are matched case-insensitively, as the
/// parser does.
fn guard_depth(unfolded: &str) -> Option<usize> {
    let (mut depth, mut deepest) = (0usize, 0usize);
    for line in unfolded.lines() {
        if let Some(name) = strip_tag(line, "begin:") {
            if !name.chars().all(key_char) {
                return None;
            }
            depth += 1;
            deepest = deepest.max(depth);
        } else if let Some(name) = strip_tag(line, "end:") {
            if !name.chars().all(key_char) {
                return None;
            }
            depth = depth.saturating_sub(1);
        } else if line.contains(['"', ';', ':']) && !whole_property(line) {
            // A line with none of these has no parameter or value to run on; the parser reads it as bare
            // names or fails on it.
            return None;
        }
    }
    Some(deepest)
}

fn strip_tag<'a>(line: &'a str, tag: &str) -> Option<&'a str> {
    let head = line.get(..tag.len())?;
    head.eq_ignore_ascii_case(tag).then(|| &line[tag.len()..])
}

/// A character of a name or parameter key, as the parser reads them.
fn key_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | ',' | '/' | '_' | '-')
}

/// Whether the parser reads the line as one property ending at its ':', in the parser's grammar:
/// `name *(";" [spaces] key ["=" value]) ":"`. A value is quoted only when '"' follows '=' and a later
/// '"' closes it with something between; that '"' must be on this line, as the parser looks past line
/// ends for it. Any other value runs to the next ';' or ':', which must also be on this line.
fn whole_property(line: &str) -> bool {
    let mut rest = line
        .trim_start_matches([' ', '\t'])
        .trim_start_matches(key_char);
    while let Some(param) = rest.strip_prefix(';') {
        rest = param
            .trim_start_matches([' ', '\t'])
            .trim_start_matches(key_char);
        let Some(value) = rest.strip_prefix('=') else {
            continue;
        };
        rest = match value.strip_prefix('"').map(|quoted| quoted.find('"')) {
            Some(Some(end)) if end > 0 => &value[end + 2..],
            Some(None) => return false,
            _ => match value.find([';', ':']) {
                Some(end) => &value[end..],
                None => return false,
            },
        };
    }
    rest.starts_with(':')
}

fn occurrence(e: &Event) -> Occurrence {
    Occurrence {
        uid: e.uid.clone(),
        title: e.title.clone(),
        start: e.start.with_timezone(&Utc),
        end: (e.start + duration(e)).with_timezone(&Utc),
    }
}

fn duration(e: &Event) -> Duration {
    e.end.map_or(Duration::zero(), |end| end - e.start)
}

/// Estimated rrule steps from DTSTART to `to`: the span over the period, times the BY* fan-out. Monthly,
/// yearly and weekly rules with BY* parts are walked by day, so they are costed by day.
/// shortcut: an upper estimate, not a count; rrule's real cost per step varies by rule.
fn work(e: &Event, to: DateTime<Utc>) -> f64 {
    let Some(text) = &e.rrule else {
        return 1.0;
    };
    let span = (to - e.start.with_timezone(&Utc)).num_seconds().max(0) as f64;
    let (mut freq, mut interval, mut fanout, mut has_by) = (String::new(), 1.0, 1.0, false);
    for part in text.split(';') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_uppercase().as_str() {
            "FREQ" => freq = value.to_ascii_uppercase(),
            "INTERVAL" => {
                interval = value
                    .parse::<f64>()
                    .ok()
                    .filter(|i| *i >= 1.0)
                    .unwrap_or(1.0)
            }
            k if k.starts_with("BY") => {
                has_by = true;
                fanout *= value.split(',').count() as f64;
            }
            _ => {}
        }
    }
    let period = match freq.as_str() {
        "SECONDLY" => 1.0,
        "MINUTELY" => 60.0,
        "HOURLY" => 3_600.0,
        "DAILY" => 86_400.0,
        "WEEKLY" if has_by => 86_400.0,
        "WEEKLY" => 604_800.0,
        "MONTHLY" if has_by => 86_400.0,
        "MONTHLY" => 2_419_200.0,
        "YEARLY" if has_by => 86_400.0,
        "YEARLY" => 31_536_000.0,
        // No usable FREQ: rrule rejects the rule, so it costs nothing.
        _ => return 1.0,
    };
    // A series costs at least one period: a walk inside one period still generates the whole BY* fan-out.
    (span / (period * interval)).max(1.0) * fanout
}

/// The instance starts of one event within [from, to). A non-recurring event is its DTSTART.
fn starts(e: &Event, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<DateTime<Tz>> {
    let Some(text) = &e.rrule else {
        return vec![e.start];
    };
    let Some(rule) = text
        .parse::<RRule<Unvalidated>>()
        .ok()
        .and_then(|rule| rule.validate(e.start).ok())
    else {
        warn!("calendar: skipped a recurrence rule that does not validate");
        return Vec::new();
    };
    RRuleSet::new(e.start)
        .rrule(rule)
        .set_exdates(e.exdates.clone())
        .after(from.with_timezone(&Tz::UTC))
        .before(to.with_timezone(&Tz::UTC))
        .all(u16::MAX)
        .dates
}

fn event<'a>(c: &'a Component<'a>, unknown_tz_warned: &mut bool) -> Option<Event> {
    let uid = prop(c, "UID")?.val.as_str().trim().to_string();
    if uid.chars().count() > MAX_UID_CHARS {
        return None;
    }
    let start = when(prop(c, "DTSTART")?, unknown_tz_warned)?;
    let mut exdates = Vec::new();
    for p in props(c, "EXDATE") {
        for value in p.val.as_str().split(',') {
            exdates.extend(at(p, value, unknown_tz_warned));
        }
    }
    let recurrence_id = match prop(c, "RECURRENCE-ID") {
        Some(p) => Some(when(p, unknown_tz_warned)?.with_timezone(&Utc)),
        None => None,
    };
    Some(Event {
        uid,
        title: clean_title(prop(c, "SUMMARY").map_or("", |p| p.val.as_str())),
        recurrence_id,
        cancelled: prop(c, "STATUS")
            .is_some_and(|p| p.val.as_str().trim().eq_ignore_ascii_case("CANCELLED")),
        start,
        end: prop(c, "DTEND").and_then(|p| when(p, unknown_tz_warned)),
        rrule: prop(c, "RRULE").map(|p| p.val.as_str().trim().to_string()),
        exdates,
    })
}

/// The first property with this name (names are passed in upper case, and matched case-insensitively).
fn prop<'a>(c: &'a Component<'a>, name: &'static str) -> Option<&'a Property<'a>> {
    props(c, name).next()
}

fn props<'a>(c: &'a Component<'a>, name: &'static str) -> impl Iterator<Item = &'a Property<'a>> {
    c.properties
        .iter()
        .filter(move |p| p.name.as_str().eq_ignore_ascii_case(name))
}

/// A DATE-TIME property. All-day (DATE) and unreadable values give None.
fn when(p: &Property<'_>, unknown_tz_warned: &mut bool) -> Option<DateTime<Tz>> {
    at(p, p.val.as_str(), unknown_tz_warned)
}

/// One DATE-TIME value of a property: `20261018T100000` in the TZID zone, or `...Z` in UTC.
fn at(p: &Property<'_>, text: &str, unknown_tz_warned: &mut bool) -> Option<DateTime<Tz>> {
    let text = text.trim();
    let (digits, zone) = match text.strip_suffix('Z') {
        Some(digits) => (digits, Tz::UTC),
        None => (text, zone(p, unknown_tz_warned)),
    };
    let naive = NaiveDateTime::parse_from_str(digits, "%Y%m%dT%H%M%S").ok()?;
    match zone.from_local_datetime(&naive) {
        LocalResult::Single(t) => Some(t),
        // A repeated hour (clocks go back): the first pass.
        LocalResult::Ambiguous(first, _) => Some(first),
        // A skipped hour (clocks go forward): RFC 5545 3.3.5 reads the time with the offset from before the gap.
        // Known limit: a recurring series whose DTSTART is in the gap keeps the shifted wall time for its later instances.
        LocalResult::None => {
            let before = zone
                .offset_from_utc_datetime(&naive.checked_sub_signed(Duration::days(1))?)
                .fix();
            let utc =
                naive.checked_sub_signed(Duration::seconds(i64::from(before.local_minus_utc())))?;
            Some(zone.from_utc_datetime(&utc))
        }
    }
}

/// The zone for a TZID name, or None if the zone database does not know it.
fn tzid_zone(name: &str) -> Option<Tz> {
    name.trim_matches('"')
        .parse::<chrono_tz::Tz>()
        .ok()
        .map(Tz::from)
}

/// The TZID zone. An unknown name (a Windows zone, say) is local time; it is logged once, without the name.
fn zone(p: &Property<'_>, unknown_tz_warned: &mut bool) -> Tz {
    let Some(name) = p
        .params
        .iter()
        .find(|param| param.key.as_str().eq_ignore_ascii_case("TZID"))
        .and_then(|param| param.val.as_ref())
    else {
        return Tz::LOCAL;
    };
    match tzid_zone(name.as_str()) {
        Some(zone) => zone,
        None => {
            if !*unknown_tz_warned {
                warn!("calendar: unknown TZID, treated as local time");
                *unknown_tz_warned = true;
            }
            Tz::LOCAL
        }
    }
}

/// Control characters stripped, trimmed, capped. The title is event data and is never logged.
fn clean_title(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_EVENT_TITLE_CHARS)
        .collect()
}

/// Https only, with a host. The error never contains the input.
pub(crate) fn validate_url(url: &str) -> Result<url::Url, String> {
    let parsed =
        url::Url::parse(url).map_err(|_| "Calendar link is not a valid address".to_string())?;
    if parsed.scheme() != "https" {
        return Err("Calendar link must start with https://".to_string());
    }
    if parsed.host_str().is_none() {
        return Err("Calendar link has no host".to_string());
    }
    Ok(parsed)
}

/// Validates the link, then fetches it. Only https is allowed here.
pub(crate) async fn fetch(url: &str) -> Result<String, String> {
    get_body(validate_url(url)?.as_str()).await
}

/// No Referer: a redirect must not carry the secret address (path and query) to the next host.
fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(CALENDAR_FETCH_TIMEOUT_SECS))
        .referer(false)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.url().scheme() == "https" && attempt.previous().len() < MAX_REDIRECTS {
                attempt.follow()
            } else {
                attempt.error("calendar redirect refused")
            }
        }))
        .build()
}

/// Timeout, size cap and status check. The URL is not checked here, so tests can use a local server.
async fn get_body(url: &str) -> Result<String, String> {
    let client = client().map_err(|_| "Calendar client could not start".to_string())?;
    let mut response = client.get(url).send().await.map_err(|e| fetch_error(&e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Calendar returned HTTP {}", status.as_u16()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| fetch_error(&e))? {
        if body.len() + chunk.len() > CALENDAR_MAX_BYTES {
            return Err(format!(
                "Calendar is larger than {} MB",
                CALENDAR_MAX_BYTES / (1024 * 1024)
            ));
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| "Calendar is not UTF-8 text".to_string())
}

/// reqwest's own messages quote the URL, so only these fixed texts leave this module.
fn fetch_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "Calendar request timed out"
    } else if e.is_redirect() {
        "Calendar redirected to an address that is not https"
    } else {
        "Calendar could not be reached"
    }
    .to_string()
}

/// The occurrences inside the window around `now`.
fn in_window(occurrences: &[Occurrence], now: DateTime<Utc>) -> Vec<Occurrence> {
    let from = now - Duration::days(CALENDAR_BEHIND_DAYS);
    let to = now + Duration::days(CALENDAR_AHEAD_DAYS);
    occurrences
        .iter()
        .filter(|o| o.start >= from && o.start < to)
        .cloned()
        .collect()
}

/// The cached occurrences inside the window around now, sorted by start.
pub fn upcoming() -> Vec<Occurrence> {
    let state = STATE.read().unwrap_or_else(PoisonError::into_inner);
    in_window(&state.cache.occurrences, Utc::now())
}

/// Fetch, expand, cache and persist. A failure keeps the last good cache and sets the error.
pub(crate) async fn refresh<R: Runtime>(app: &AppHandle<R>) -> Result<usize, String> {
    let Some(url) = settings::current().calendar_url else {
        return Err(NOT_CONNECTED.to_string());
    };
    match fetch_and_expand(&url).await {
        Ok(occurrences) => store(app, &url, occurrences).ok_or_else(|| CHANGED.to_string()),
        Err(problem) => {
            record_error(&url, problem.clone());
            Err(problem)
        }
    }
}

async fn fetch_and_expand(url: &str) -> Result<Vec<Occurrence>, String> {
    let ics = fetch(url).await?;
    let now = Utc::now();
    let from = now - Duration::days(CALENDAR_BEHIND_DAYS);
    let to = now + Duration::days(CALENDAR_AHEAD_DAYS);
    // Expansion is CPU work; it runs off the async runtime.
    let occurrences = tokio::task::spawn_blocking(move || expand(&ics, from, to))
        .await
        .map_err(|_| UNREADABLE.to_string())??;
    info!(
        "calendar: fetched, {} occurrences in the window",
        occurrences.len()
    );
    Ok(occurrences)
}

/// Whether `url` is still the connected calendar.
/// Lock order: the calendar state, then the settings cache. Settings never takes the calendar lock.
fn still_connected(url: &str) -> bool {
    settings::current().calendar_url.as_deref() == Some(url)
}

/// Makes the expansion the cache and saves it, only while `url` is still connected. Otherwise
/// nothing is written. The state lock is held across the check and the write.
fn store<R: Runtime>(app: &AppHandle<R>, url: &str, occurrences: Vec<Occurrence>) -> Option<usize> {
    let mut state = STATE.write().unwrap_or_else(PoisonError::into_inner);
    if !still_connected(url) {
        return None;
    }
    let cache = Cache {
        last_ok_at: Some(Utc::now().to_rfc3339()),
        occurrences,
    };
    let count = cache.occurrences.len();
    if let Err(problem) = save(app, &cache) {
        warn!("calendar: cache not saved: {problem}");
    }
    state.cache = cache;
    state.last_error = None;
    Some(count)
}

/// Records a failed sync, only for the calendar that failed.
fn record_error(url: &str, problem: String) {
    let mut state = STATE.write().unwrap_or_else(PoisonError::into_inner);
    if still_connected(url) {
        state.last_error = Some(problem);
    }
}

fn cache_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    app.path()
        .resolve(CACHE_FILE, BaseDirectory::AppData)
        .map_err(|e| e.to_string())
}

fn save<R: Runtime>(app: &AppHandle<R>, cache: &Cache) -> Result<(), String> {
    let bytes = serde_json::to_vec(cache).map_err(|e| e.to_string())?;
    write_atomic(&cache_path(app)?, &bytes)
}

static WRITE_COUNT: AtomicU64 = AtomicU64::new(0);

/// Writes to a temp file whose name no other write uses, then renames it over the old one, so a
/// crash cannot leave half a file and two writes cannot share a temp file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let n = WRITE_COUNT.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_file_name(format!("{CACHE_FILE}.{}.{n}.tmp", std::process::id()));
    let written = fs::write(&tmp, bytes).and_then(|()| fs::rename(&tmp, path));
    written.map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

/// Reads the persisted cache. A broken file gives None; its content is never logged.
fn parse_cache(bytes: &[u8]) -> Option<Cache> {
    serde_json::from_slice(bytes).ok()
}

/// A missing file is an empty cache. A broken one is logged without its content and also starts empty.
pub fn load_at_startup<R: Runtime>(app: &AppHandle<R>) {
    let bytes = cache_path(app).and_then(|path| match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    });
    let cache = match bytes {
        Ok(None) => Cache::default(),
        Ok(Some(bytes)) => parse_cache(&bytes).unwrap_or_else(|| {
            warn!("calendar: cache file unreadable, starting empty");
            Cache::default()
        }),
        Err(problem) => {
            warn!("calendar: cache file unreadable, starting empty: {problem}");
            Cache::default()
        }
    };
    info!(
        "calendar: cache loaded, {} occurrences",
        cache.occurrences.len()
    );
    STATE.write().unwrap_or_else(PoisonError::into_inner).cache = cache;
}

fn status() -> CalendarStatus {
    let url = settings::current().calendar_url;
    let state = STATE.read().unwrap_or_else(PoisonError::into_inner);
    CalendarStatus {
        connected: url.is_some(),
        host: url.as_deref().and_then(settings::host_of),
        last_ok_at: state.cache.last_ok_at.clone(),
        last_error: state.last_error.clone(),
        upcoming: in_window(&state.cache.occurrences, Utc::now()).len(),
    }
}

#[tauri::command]
pub async fn calendar_connect<R: Runtime>(
    app: AppHandle<R>,
    url: String,
) -> Result<CalendarStatus, String> {
    let url = url.trim().to_string();
    validate_url(&url)?;
    let occurrences = fetch_and_expand(&url).await?;
    settings::set_calendar_url(&app, Some(url.clone()))?;
    let count = store(&app, &url, occurrences).ok_or_else(|| CHANGED.to_string())?;
    info!("calendar: connected, {count} occurrences in the window");
    Ok(status())
}

#[tauri::command]
pub async fn calendar_disconnect<R: Runtime>(app: AppHandle<R>) -> Result<CalendarStatus, String> {
    settings::set_calendar_url(&app, None)?;
    {
        let mut state = STATE.write().unwrap_or_else(PoisonError::into_inner);
        if let Err(problem) = save(&app, &Cache::default()) {
            warn!("calendar: cleared cache not saved: {problem}");
        }
        *state = State::default();
    }
    info!("calendar: disconnected");
    Ok(status())
}

#[tauri::command]
pub async fn calendar_status() -> CalendarStatus {
    status()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};
    use icalendar::parser::Parameter;
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    fn ics(lines: &[&str]) -> String {
        lines.join("\r\n")
    }

    /// Serves one canned HTTP response to the first connection, then closes.
    async fn serve_once(response: Vec<u8>) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request).await;
                let _ = stream.write_all(&response).await;
                let _ = stream.shutdown().await;
            }
        });
        addr
    }

    #[test]
    fn expand_single_event() {
        let body = ics(&[
            "BEGIN:VCALENDAR",
            "BEGIN:VEVENT",
            "UID:single-1",
            "DTSTART:20261020T130000Z",
            "DTEND:20261020T140000Z",
            "SUMMARY:Design review",
            "END:VEVENT",
            "END:VCALENDAR",
        ]);
        let got = expand(&body, utc(2026, 10, 20, 0, 0), utc(2026, 10, 21, 0, 0)).unwrap();
        assert_eq!(
            got,
            vec![Occurrence {
                uid: "single-1".into(),
                title: "Design review".into(),
                start: utc(2026, 10, 20, 13, 0),
                end: utc(2026, 10, 20, 14, 0),
            }]
        );
    }

    #[test]
    fn expand_weekly_with_override_exdate_dst() {
        // Berlin switches from CEST to CET on 2026-10-25: 10:00 local is 08:00Z before, 09:00Z after.
        let body = ics(&[
            "BEGIN:VCALENDAR",
            "BEGIN:VEVENT",
            "UID:weekly-1",
            "DTSTART;TZID=Europe/Berlin:20261018T100000",
            "DTEND;TZID=Europe/Berlin:20261018T103000",
            "RRULE:FREQ=WEEKLY;UNTIL=20261201T000000Z",
            "EXDATE;TZID=Europe/Berlin:20261108T100000",
            "SUMMARY:Weekly sync",
            "END:VEVENT",
            "BEGIN:VEVENT",
            "UID:weekly-1",
            "RECURRENCE-ID;TZID=Europe/Berlin:20261101T100000",
            "DTSTART;TZID=Europe/Berlin:20261101T140000",
            "DTEND;TZID=Europe/Berlin:20261101T143000",
            "SUMMARY:Weekly sync (moved)",
            "END:VEVENT",
            "END:VCALENDAR",
        ]);
        let got = expand(&body, utc(2026, 10, 17, 0, 0), utc(2026, 11, 20, 0, 0)).unwrap();
        let want = vec![
            (
                "Weekly sync",
                utc(2026, 10, 18, 8, 0),
                utc(2026, 10, 18, 8, 30),
            ),
            (
                "Weekly sync",
                utc(2026, 10, 25, 9, 0),
                utc(2026, 10, 25, 9, 30),
            ),
            (
                "Weekly sync (moved)",
                utc(2026, 11, 1, 13, 0),
                utc(2026, 11, 1, 13, 30),
            ),
            (
                "Weekly sync",
                utc(2026, 11, 15, 9, 0),
                utc(2026, 11, 15, 9, 30),
            ),
        ];
        let got: Vec<_> = got.into_iter().map(|o| (o.title, o.start, o.end)).collect();
        let want: Vec<_> = want
            .into_iter()
            .map(|(t, s, e)| (t.to_string(), s, e))
            .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn cancelled_instance_skipped() {
        let body = ics(&[
            "BEGIN:VCALENDAR",
            "BEGIN:VEVENT",
            "UID:series-2",
            "DTSTART:20261019T090000Z",
            "DTEND:20261019T093000Z",
            "RRULE:FREQ=WEEKLY;COUNT=3",
            "SUMMARY:Standup",
            "END:VEVENT",
            "BEGIN:VEVENT",
            "UID:series-2",
            "RECURRENCE-ID:20261026T090000Z",
            "DTSTART:20261026T090000Z",
            "STATUS:CANCELLED",
            "SUMMARY:Standup",
            "END:VEVENT",
            "BEGIN:VEVENT",
            "UID:gone-3",
            "DTSTART:20261020T120000Z",
            "DTEND:20261020T130000Z",
            "STATUS:CANCELLED",
            "SUMMARY:Dropped",
            "END:VEVENT",
            "END:VCALENDAR",
        ]);
        let got = expand(&body, utc(2026, 10, 18, 0, 0), utc(2026, 11, 30, 0, 0)).unwrap();
        let starts: Vec<_> = got.iter().map(|o| o.start).collect();
        assert_eq!(
            starts,
            vec![utc(2026, 10, 19, 9, 0), utc(2026, 11, 2, 9, 0)]
        );
        assert!(got.iter().all(|o| o.title != "Dropped"));
    }

    #[test]
    fn sub_hourly_series_skipped() {
        let body = ics(&[
            "BEGIN:VCALENDAR",
            "BEGIN:VEVENT",
            "UID:ping-7",
            "DTSTART:20200101T090000Z",
            "DTEND:20200101T090100Z",
            "RRULE:FREQ=MINUTELY",
            "END:VEVENT",
            "END:VCALENDAR",
        ]);
        let got = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn all_day_ignored() {
        let body = ics(&[
            "BEGIN:VCALENDAR",
            "BEGIN:VEVENT",
            "UID:holiday-4",
            "DTSTART;VALUE=DATE:20261020",
            "DTEND;VALUE=DATE:20261021",
            "SUMMARY:Holiday",
            "END:VEVENT",
            "END:VCALENDAR",
        ]);
        let got = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap();
        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn unknown_tzid_treated_as_local() {
        // A Windows zone name is not in the zone database, so the zone is local, whatever this machine's zone is.
        assert!(tzid_zone("W. Europe Standard Time").is_none());
        assert!(tzid_zone("Europe/Berlin").is_some_and(|z| !z.is_local()));
        let mut p = Property::new_ref("DTSTART", "20261020T100000");
        p.params
            .push(Parameter::new_ref("TZID", Some("W. Europe Standard Time")));
        let mut warned = false;
        assert!(zone(&p, &mut warned).is_local());

        let body = ics(&[
            "BEGIN:VCALENDAR",
            "BEGIN:VEVENT",
            "UID:win-5",
            "DTSTART;TZID=W. Europe Standard Time:20261020T100000",
            "DTEND;TZID=W. Europe Standard Time:20261020T110000",
            "SUMMARY:Windows zone",
            "END:VEVENT",
            "END:VCALENDAR",
        ]);
        let got = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap();
        let local_start = Local
            .with_ymd_and_hms(2026, 10, 20, 10, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].start, local_start);
    }
    #[test]
    fn title_sanitised_and_capped() {
        let summary = format!("SUMMARY:  Team\\, sync\\nops{}", "x".repeat(300));
        let body = ics(&[
            "BEGIN:VCALENDAR",
            "BEGIN:VEVENT",
            "UID:title-6",
            "DTSTART:20261020T120000Z",
            "DTEND:20261020T130000Z",
            &summary,
            "END:VEVENT",
            "END:VCALENDAR",
        ]);
        let got = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap();
        let expected: String = format!("Team, syncops{}", "x".repeat(300))
            .chars()
            .take(MAX_EVENT_TITLE_CHARS)
            .collect();
        assert_eq!(got[0].title, expected);
        assert_eq!(got[0].title.chars().count(), MAX_EVENT_TITLE_CHARS);
    }

    #[test]
    fn validate_url_https_only() {
        assert!(validate_url("https://calendar.example.com/private/abc?token=t").is_ok());
        for bad in [
            "http://calendar.example.com/x",
            "file:///etc/hosts",
            "https://",
            "not a url",
        ] {
            let err = validate_url(bad).unwrap_err();
            assert!(!err.contains(bad), "{err}");
        }
    }

    #[tokio::test]
    async fn fetch_error_hides_url() {
        let addr = serve_once(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        )
        .await;
        let url = format!("http://{addr}/private/abc123/basic.ics?token=secret");
        let err = get_body(&url).await.unwrap_err();
        assert_eq!(err, "Calendar returned HTTP 404");
        for secret in ["abc123", "token", "secret", "private"] {
            assert!(!err.contains(secret), "{err}");
        }
    }

    #[tokio::test]
    async fn size_cap_enforced() {
        let body = vec![b'a'; 6 * 1024 * 1024];
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(&body);
        let addr = serve_once(response).await;
        let err = get_body(&format!("http://{addr}/basic.ics"))
            .await
            .unwrap_err();
        assert!(err.contains("larger"), "{err}");
    }

    #[tokio::test]
    async fn redirect_to_http_refused() {
        let addr = serve_once(
            b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/plain.ics\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        )
        .await;
        let err = get_body(&format!("http://{addr}/start.ics"))
            .await
            .unwrap_err();
        assert_eq!(err, "Calendar redirected to an address that is not https");
    }

    #[test]
    fn deep_nesting_refused() {
        // The parser matches BEGIN case-insensitively, so the guard must count lower case too.
        for tag in ["BEGIN:VEVENT\r\n", "begin:vevent\r\n"] {
            let err = expand(
                &tag.repeat(10_000),
                utc(2026, 10, 19, 0, 0),
                utc(2026, 10, 22, 0, 0),
            )
            .unwrap_err();
            assert_eq!(err, "Calendar could not be read");
        }
    }
    #[test]
    fn ancient_hourly_series_skipped_fast() {
        let body = ics(&[
            "BEGIN:VCALENDAR",
            "BEGIN:VEVENT",
            "UID:ancient-8",
            "DTSTART:10000101T000000Z",
            "DTEND:10000101T000100Z",
            "RRULE:FREQ=HOURLY",
            "END:VEVENT",
            "END:VCALENDAR",
        ]);
        let started = std::time::Instant::now();
        let got = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap();
        assert!(got.is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    #[test]
    fn feed_wide_occurrence_cap_holds() {
        let mut body = String::from("BEGIN:VCALENDAR\r\n");
        for i in 0..MAX_FEED_OCCURRENCES + 1000 {
            body.push_str(&format!(
                "BEGIN:VEVENT\r\nUID:many-{i}\r\nDTSTART:20261020T120000Z\r\nDTEND:20261020T130000Z\r\nEND:VEVENT\r\n"
            ));
        }
        body.push_str("END:VCALENDAR\r\n");
        let got = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap();
        assert_eq!(got.len(), MAX_FEED_OCCURRENCES);
    }

    #[test]
    fn long_uid_skipped() {
        let ok = "u".repeat(MAX_UID_CHARS);
        let long = "u".repeat(MAX_UID_CHARS + 1);
        let body = ics(&[
            "BEGIN:VCALENDAR",
            "BEGIN:VEVENT",
            &format!("UID:{ok}"),
            "DTSTART:20261020T120000Z",
            "DTEND:20261020T130000Z",
            "END:VEVENT",
            "BEGIN:VEVENT",
            &format!("UID:{long}"),
            "DTSTART:20261020T140000Z",
            "DTEND:20261020T150000Z",
            "END:VEVENT",
            "END:VCALENDAR",
        ]);
        let got = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].uid, ok);
    }

    #[test]
    fn dst_gap_start_shifted_forward() {
        // Berlin's clocks go from 02:00 to 03:00 on 2026-03-29. 02:30 reads with the offset from before the gap (+01:00).
        let body = ics(&[
            "BEGIN:VCALENDAR",
            "BEGIN:VEVENT",
            "UID:gap-9",
            "DTSTART;TZID=Europe/Berlin:20260329T023000",
            "DTEND;TZID=Europe/Berlin:20260329T040000",
            "SUMMARY:Early",
            "END:VEVENT",
            "END:VCALENDAR",
        ]);
        let got = expand(&body, utc(2026, 3, 28, 0, 0), utc(2026, 3, 30, 0, 0)).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].start, utc(2026, 3, 29, 1, 30));
        assert_eq!(got[0].end, utc(2026, 3, 29, 2, 0));
    }

    #[test]
    fn cache_write_leaves_one_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CACHE_FILE);
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn nesting_bypass_refused() {
        // Each line closes one component and opens another from the rest of the line, so the depth count
        // sees one BEGIN per pair. Every spelling is refused.
        let pairs = [
            "BEGIN:A\r\nEND:ABEGIN:A\r\n",
            "begin:a\r\nend:abegin:a\r\n",
            "BEGIN:VEVENTBEGIN:VEVENT\r\n",
        ];
        for pair in pairs {
            let body = format!("BEGIN:VCALENDAR\r\n{}", pair.repeat(20_000));
            let err = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap_err();
            assert_eq!(err, "Calendar could not be read", "{pair}");
        }
    }

    #[test]
    fn lines_spanning_params_refused() {
        // A parameter value may run across a line break in the parser, which swallows the END the guard counts.
        for pair in [
            "BEGIN:A\r\nX;P=\"a\r\nEND:A\":v\r\n",
            "BEGIN:A\r\nX;P=a\r\nEND:A\r\n",
        ] {
            let body = format!("BEGIN:VCALENDAR\r\n{}", pair.repeat(100_000));
            let err = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap_err();
            assert_eq!(err, "Calendar could not be read", "{pair}");
        }
    }

    #[test]
    fn param_quote_bypass_refused() {
        // A '"' only opens a quoted value right after '='; elsewhere it is literal, and a quoted value runs
        // to the next '"' even on a later line, swallowing the END the guard counts.
        for pair in [
            "BEGIN:A\r\nX;P=a\"b;Q=\"c:d\r\nEND:A\r\nY:\":v\r\n",
            "BEGIN:A\r\nX;P=a\";Q=\":\r\nEND:A\":v\r\n",
            "BEGIN:A\r\nX;P=a=\"b;Q=\"c:d\r\nEND:A\r\nY:\":v\r\n",
        ] {
            let body = format!("BEGIN:VCALENDAR\r\n{}", pair.repeat(60_000));
            let err = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap_err();
            assert_eq!(err, "Calendar could not be read", "{pair}");
        }
    }

    /// Depth of a parsed component; a top-level one has depth 1, as the guard counts it.
    fn tree_depth(c: &Component<'_>) -> usize {
        1 + c.components.iter().map(tree_depth).max().unwrap_or(0)
    }

    #[test]
    fn guard_depth_bounds_parser_depth() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        // The parser's depth and whether the guard counted, once the guard is checked not to count less.
        let check = |raw: &str| -> Option<(usize, bool)> {
            let text = normalize(raw);
            let calendar = read_calendar(&text).ok()?;
            let actual = calendar
                .components
                .iter()
                .map(tree_depth)
                .max()
                .unwrap_or(0);
            let counted = guard_depth(&text);
            if let Some(max) = counted {
                assert!(
                    actual <= max,
                    "{raw:?}: parser depth {actual} > guard depth {max}"
                );
            }
            Some((actual, counted.is_some()))
        };
        let (mut parsed, mut deep, mut run_on) = (0, 0, 0);

        // The parser wants matching END names, so build mostly well-formed trees, then add raw pieces that
        // break lines apart (a quote, a bare ':', a lone line break, an END with a different name).
        let noise = [
            "\"",
            ":",
            ";P=",
            "X",
            "v",
            "\r\n",
            "\n",
            "\r",
            "BEGIN:A",
            "END:A",
            "end:b",
            "X;P=\"a\r\n",
            "X;P=a\r\n",
        ];
        let names = ["A", "b", "VCALENDAR", "vevent"];
        for _ in 0..50_000 {
            let mut raw = String::new();
            let mut open: Vec<&str> = Vec::new();
            for _ in 0..1 + next() % 24 {
                match next() % 8 {
                    0 | 1 | 2 if open.len() < 6 => {
                        let n = names[next() % names.len()];
                        raw.push_str(&format!("BEGIN:{n}\r\n"));
                        open.push(n);
                    }
                    3 | 4 if !open.is_empty() => {
                        raw.push_str(&format!("END:{}\r\n", open.pop().unwrap()))
                    }
                    5 => raw.push_str("X;P=\"a, b\":v\r\n"),
                    6 => raw.push_str(noise[next() % noise.len()]),
                    _ => raw.push_str("X:v\r\n"),
                }
            }
            while let Some(n) = open.pop() {
                if next() % 8 != 0 {
                    raw.push_str(&format!("END:{n}\r\n"));
                }
            }
            if let Some((actual, true)) = check(&raw) {
                parsed += 1;
                deep += usize::from(actual > 1);
            }
        }

        // A content line that runs on swallows the END after it; a later line may close what it left open
        // (a quoted value), and the extra END lets the parser finish one level deeper than the lines say.
        let fragments = [
            ";P=", ";Q=\"", "\"", ":", "v", "c:d", "\"a\"", "a\"b", "\";Q=\"", "=", "=\"", "a=\"b",
            ",", ";", " ",
        ];
        let closers = ["X:\":v", "X;P=\":v", "\":v", "X:v", ""];
        for _ in 0..100_000 {
            let mut line = String::from("X");
            for _ in 0..1 + next() % 5 {
                line.push_str(fragments[next() % fragments.len()]);
            }
            let closer = closers[next() % closers.len()];
            let raw =
                format!("BEGIN:A\r\n{line}\r\nEND:A\r\n{closer}\r\nBEGIN:A\r\nEND:A\r\nEND:A\r\n");
            run_on += usize::from(check(&raw).is_some_and(|(actual, _)| actual == 2));
        }
        assert!(
            parsed > 2_500 && deep > 1_500 && run_on > 6_000,
            "weak generator: parsed {parsed}, nested {deep}, run-on lines {run_on}"
        );
    }

    #[test]
    fn google_style_feed_still_parses() {
        let body = ics(&[
            "BEGIN:VCALENDAR",
            "PRODID:-//Google Inc//Google Calendar 70.9054//EN",
            "BEGIN:VEVENT",
            "UID:g1@google.com",
            "DTSTART;TZID=Europe/Berlin:20261020T120000",
            "DTEND;TZID=Europe/Berlin:20261020T130000",
            "ATTENDEE;CUTYPE=INDIVIDUAL;CN=\"Doe, Jane\";X-NUM-GUESTS=0:mailto:jane@example.com",
            "ORGANIZER;CN=Boss:mailto:boss@example.com",
            "ATTENDEE;CN=\"a:b\";ROLE=REQ-PARTICIPANT:mailto:ab@example.com",
            "DESCRIPTION;ALTREP=\"http://x\":Notes",
            "X-FOO:",
            // A broken fold leaves a bare word, which the parser reads as a property without a value.
            "continued",
            "SUMMARY:Standup",
            "BEGIN:VALARM",
            "ACTION:DISPLAY",
            "END:VALARM",
            "END:VEVENT",
            "END:VCALENDAR",
            "",
        ]);
        let got = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].title, "Standup");
    }

    #[test]
    fn past_one_offs_do_not_fill_the_cap() {
        let mut body = String::from("BEGIN:VCALENDAR\r\n");
        for i in 0..6_000 {
            body.push_str(&format!(
                "BEGIN:VEVENT\r\nUID:past-{i}\r\nDTSTART:20200101T120000Z\r\nDTEND:20200101T130000Z\r\nEND:VEVENT\r\n"
            ));
        }
        body.push_str("BEGIN:VEVENT\r\nUID:next\r\nDTSTART:20261020T120000Z\r\nDTEND:20261020T130000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");
        let got = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].uid, "next");
    }

    #[test]
    fn yearly_birthdays_do_not_starve_a_weekly_standup() {
        let mut body = String::from("BEGIN:VCALENDAR\r\n");
        for i in 0..20 {
            body.push_str(&format!(
                "BEGIN:VEVENT\r\nUID:bday-{i}\r\nDTSTART:19500305T090000Z\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYMONTHDAY=5\r\nEND:VEVENT\r\n"
            ));
        }
        body.push_str("BEGIN:VEVENT\r\nUID:standup\r\nDTSTART:20261001T090000Z\r\nDTEND:20261001T093000Z\r\nRRULE:FREQ=WEEKLY;BYDAY=MO\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");
        let got = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 11, 2, 0, 0)).unwrap();
        assert!(got.iter().any(|o| o.uid == "standup"), "{got:?}");
    }

    #[test]
    fn never_matching_rule_stays_within_the_deadline() {
        // February 30th never occurs, so rrule searches on for every year since 1950.
        let mut body = String::from("BEGIN:VCALENDAR\r\n");
        for i in 0..200 {
            body.push_str(&format!(
                "BEGIN:VEVENT\r\nUID:never-{i}\r\nDTSTART:19500101T090000Z\r\nRRULE:FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=30\r\nEND:VEVENT\r\n"
            ));
        }
        body.push_str("END:VCALENDAR\r\n");
        let started = std::time::Instant::now();
        let got = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 11, 2, 0, 0)).unwrap();
        assert!(got.is_empty());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn bare_lf_cannot_hide_nesting() {
        // A bare LF hides END from the guard's line split but not from the parser, which would nest 100k deep.
        // Real nesting written with mixed endings must still be refused.
        let flat = format!(
            "BEGIN:VCALENDAR\r\n{}",
            "BEGIN:A\nX:v\nEND:A\r\n".repeat(100_000)
        );
        for body in [
            &flat,
            &format!(
                "BEGIN:VCALENDAR\r\n{}",
                "BEGIN:A\nBEGIN:A\r\n".repeat(100_000)
            ),
        ] {
            let err = expand(body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap_err();
            assert_eq!(err, "Calendar could not be read");
        }
    }

    #[test]
    fn lf_only_feed_parses_in_linear_time() {
        let mut body = String::from("BEGIN:VCALENDAR\n");
        let mut i = 0;
        while body.len() < 1_000_000 {
            body.push_str(&format!(
                "BEGIN:VEVENT\nUID:lf-{i}\nDTSTART:20261020T120000Z\nDTEND:20261020T130000Z\nEND:VEVENT\n"
            ));
            i += 1;
        }
        body.push_str("END:VCALENDAR\n");
        let started = std::time::Instant::now();
        let got = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap();
        assert!(!got.is_empty());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn short_span_dense_yearly_series_skipped() {
        // One second of span is one period's fraction, but the yearly walk generates every BY* combination
        // of the year. The series must cost a whole period, so it is skipped and the standup still shows.
        let list = |lo: u32, hi: u32| {
            (lo..=hi)
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        let rrule = format!(
            "RRULE:FREQ=YEARLY;BYMONTH={};BYMONTHDAY={};BYHOUR={};BYMINUTE={};BYSECOND={}",
            list(1, 12),
            list(1, 31),
            list(0, 23),
            list(0, 59),
            list(0, 59)
        );
        let body = ics(&[
            "BEGIN:VCALENDAR",
            "BEGIN:VEVENT",
            "UID:dense",
            "DTSTART:20261021T235959Z",
            "DTEND:20261022T000000Z",
            &rrule,
            "END:VEVENT",
            "BEGIN:VEVENT",
            "UID:standup",
            "DTSTART:20261020T120000Z",
            "DTEND:20261020T130000Z",
            "END:VEVENT",
            "END:VCALENDAR",
        ]);
        let got = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap();
        let uids: Vec<&str> = got.iter().map(|o| o.uid.as_str()).collect();
        assert_eq!(uids, vec!["standup"]);
    }

    #[test]
    fn corrupt_cache_is_empty_and_silent() {
        assert!(parse_cache(b"{\"occurrences\": [{\"uid\": \"secret-").is_none());
        assert!(parse_cache(b"[]").is_none());
    }
}
