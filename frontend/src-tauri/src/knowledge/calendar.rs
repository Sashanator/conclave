// The calendar link: a user-pasted iCal (secret) address, fetched and expanded into the next
// occurrences. The URL is a credential; it is stored by settings, never logged or returned.

use super::settings;
use super::{
    CALENDAR_AHEAD_DAYS, CALENDAR_BEHIND_DAYS, CALENDAR_FETCH_TIMEOUT_SECS, CALENDAR_MAX_BYTES,
    MAX_EVENT_TITLE_CHARS,
};
use chrono::{DateTime, Duration, NaiveDateTime, TimeZone, Utc};
use icalendar::parser::{read_calendar, unfold, Component, Property};
use log::{info, warn};
use rrule::{Frequency, RRule, RRuleSet, Tz, Unvalidated};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::{LazyLock, PoisonError, RwLock};
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, Runtime};

const CACHE_FILE: &str = "calendar-cache.json";
/// Per series, per fetch. The window is at most a few weeks, so no real series comes close.
const MAX_INSTANCES: u16 = 1000;
/// A redirect chain longer than this is refused.
const MAX_REDIRECTS: usize = 5;
/// The parser recurses once per nested BEGIN, so a deeper feed could overflow the stack. Real calendars nest 3.
const MAX_NESTING: usize = 16;

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

/// Pure: parse an iCal body and expand occurrences in [from, to). Skips all-day, cancelled and EXDATE'd instances; applies RECURRENCE-ID overrides.
pub fn expand(
    ics: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Vec<Occurrence>, String> {
    // The parser's error quotes the input, so only this fixed message leaves the function.
    let unfolded = unfold(ics);
    if nesting_too_deep(&unfolded) {
        return Err("Calendar could not be read".to_string());
    }
    let calendar =
        read_calendar(&unfolded).map_err(|_| "Calendar could not be read".to_string())?;
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
    let mut overrides: HashMap<(String, DateTime<Utc>), Event> = overrides
        .into_iter()
        .filter_map(|e| Some(((e.uid.clone(), e.recurrence_id?), e)))
        .collect();

    let mut out = Vec::new();
    for master in masters.iter().filter(|m| !m.cancelled) {
        for start in starts(master, from, to) {
            let utc = start.with_timezone(&Utc);
            match overrides.remove(&(master.uid.clone(), utc)) {
                Some(moved) if !moved.cancelled => out.push(occurrence(&moved)),
                Some(_) => {}
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
        if !moved.cancelled && !cancelled_series.contains(&uid) {
            out.push(occurrence(&moved));
        }
    }
    out.retain(|o| o.start >= from && o.start < to);
    out.sort_by_key(|o| o.start);
    Ok(out)
}

fn nesting_too_deep(unfolded: &str) -> bool {
    let mut depth = 0usize;
    for line in unfolded.lines() {
        if line.starts_with("BEGIN:") {
            depth += 1;
            if depth > MAX_NESTING {
                return true;
            }
        } else if line.starts_with("END:") {
            depth = depth.saturating_sub(1);
        }
    }
    false
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
    // Sub-hourly series would make rrule walk every instance since DTSTART; no calendar needs them.
    if matches!(rule.get_freq(), Frequency::Secondly | Frequency::Minutely) {
        warn!("calendar: skipped a series that repeats more often than hourly");
        return Vec::new();
    }
    RRuleSet::new(e.start)
        .rrule(rule)
        .set_exdates(e.exdates.clone())
        .after(from.with_timezone(&Tz::UTC))
        .before(to.with_timezone(&Tz::UTC))
        .all(MAX_INSTANCES)
        .dates
}

fn event<'a>(c: &'a Component<'a>, unknown_tz_warned: &mut bool) -> Option<Event> {
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
        uid: prop(c, "UID")?.val.as_str().trim().to_string(),
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
    zone.from_local_datetime(&naive).earliest()
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
    match name.as_str().trim_matches('"').parse::<chrono_tz::Tz>() {
        Ok(tz) => tz.into(),
        Err(_) => {
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

/// Timeout, size cap and status check. The URL is not checked here, so tests can use a local server.
async fn get_body(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(CALENDAR_FETCH_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.url().scheme() == "https" && attempt.previous().len() < MAX_REDIRECTS {
                attempt.follow()
            } else {
                attempt.error("calendar redirect refused")
            }
        }))
        .build()
        .map_err(|_| "Calendar client could not start".to_string())?;
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
        return Err("Calendar is not connected".to_string());
    };
    match fetch_and_expand(&url).await {
        Ok(occurrences) => {
            // A disconnect during the fetch must not bring the calendar back.
            if settings::current().calendar_url.is_none() {
                return Err("Calendar is not connected".to_string());
            }
            Ok(store(app, occurrences))
        }
        Err(problem) => {
            STATE
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .last_error = Some(problem.clone());
            Err(problem)
        }
    }
}

async fn fetch_and_expand(url: &str) -> Result<Vec<Occurrence>, String> {
    let ics = fetch(url).await?;
    let now = Utc::now();
    let occurrences = expand(
        &ics,
        now - Duration::days(CALENDAR_BEHIND_DAYS),
        now + Duration::days(CALENDAR_AHEAD_DAYS),
    )?;
    info!(
        "calendar: fetched, {} occurrences in the window",
        occurrences.len()
    );
    Ok(occurrences)
}

/// Makes the expansion the cache, and saves it. A save that fails keeps the cache in memory.
fn store<R: Runtime>(app: &AppHandle<R>, occurrences: Vec<Occurrence>) -> usize {
    let cache = Cache {
        last_ok_at: Some(Utc::now().to_rfc3339()),
        occurrences,
    };
    let count = cache.occurrences.len();
    if let Err(problem) = save(app, &cache) {
        warn!("calendar: cache not saved: {problem}");
    }
    let mut state = STATE.write().unwrap_or_else(PoisonError::into_inner);
    state.cache = cache;
    state.last_error = None;
    count
}

fn cache_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    app.path()
        .resolve(CACHE_FILE, BaseDirectory::AppData)
        .map_err(|e| e.to_string())
}

/// Writes to a temp file and renames it over the old one, so a crash cannot leave half a file.
fn save<R: Runtime>(app: &AppHandle<R>, cache: &Cache) -> Result<(), String> {
    let path = cache_path(app)?;
    let bytes = serde_json::to_vec(cache).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
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
    settings::set_calendar_url(&app, Some(url))?;
    let count = store(&app, occurrences);
    info!("calendar: connected, {count} occurrences in the window");
    Ok(status())
}

#[tauri::command]
pub async fn calendar_disconnect<R: Runtime>(app: AppHandle<R>) -> Result<CalendarStatus, String> {
    settings::set_calendar_url(&app, None)?;
    if let Err(problem) = save(&app, &Cache::default()) {
        warn!("calendar: cleared cache not saved: {problem}");
    }
    *STATE.write().unwrap_or_else(PoisonError::into_inner) = State::default();
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
        assert!(!err.contains("127.0.0.1"), "{err}");
        assert!(!err.contains("plain.ics"), "{err}");
    }

    #[test]
    fn deep_nesting_refused() {
        let mut body = String::new();
        for _ in 0..10_000 {
            body.push_str("BEGIN:VEVENT\r\n");
        }
        let err = expand(&body, utc(2026, 10, 19, 0, 0), utc(2026, 10, 22, 0, 0)).unwrap_err();
        assert_eq!(err, "Calendar could not be read");
    }

    #[test]
    fn corrupt_cache_is_empty_and_silent() {
        assert!(parse_cache(b"{\"occurrences\": [{\"uid\": \"secret-").is_none());
        assert!(parse_cache(b"[]").is_none());
    }
}
