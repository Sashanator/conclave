//! Pure Notion mapping: link parsing, schema reading, mapping check, page body. No I/O.
use std::fmt::Display;

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone};
use serde_json::{json, Value};

use super::{DbSchema, Mapping, PropInfo};

const BAD_LINK: &str = "That doesn't look like a Notion database link. In Notion: open the database as a full page, ••• → Copy link.";

/// A task ready to send, as read from the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct SendTask {
    pub id: String,
    pub text: String,
    pub at: Option<f64>,
    pub due_date: Option<String>,
    pub meeting_title: Option<String>,
    pub meeting_date: Option<String>,
    pub dest_id: String,
}

/// 32 hex digits (dashes ignored) → lowercase 8-4-4-4-12.
pub(crate) fn parse_uuid(s: &str) -> Option<String> {
    let h: String = s.chars().filter(|c| *c != '-').collect();
    if h.len() != 32 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let h = h.to_lowercase();
    Some(format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..]))
}

pub(crate) fn parse_database_id(link: &str) -> Result<String, String> {
    let link = link.trim();
    if let Some(id) = parse_uuid(link) {
        return Ok(id);
    }
    // Query ids are views/pages, never the database; title slugs end in `-<id>`.
    let path = link.split(['?', '#']).next().unwrap_or("");
    let seg: String = path.rsplit('/').next().unwrap_or("").chars().filter(|c| *c != '-').collect();
    let tail: String = seg.chars().rev().take(32).collect::<Vec<_>>().into_iter().rev().collect();
    parse_uuid(&tail).ok_or_else(|| BAD_LINK.to_string())
}

pub(crate) fn first_data_source(db: &Value) -> Option<String> {
    parse_uuid(db.pointer("/data_sources/0/id")?.as_str()?)
}

fn rich_text_plain(v: &Value) -> String {
    v.as_array()
        .map(|a| a.iter().filter_map(|r| r.get("plain_text").and_then(Value::as_str)).collect())
        .unwrap_or_default()
}

pub(crate) fn schema_from(database_id: &str, db: &Value, ds: &Value) -> DbSchema {
    let mut properties: Vec<PropInfo> = ds
        .get("properties")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(name, p)| {
                    let kind = p.get("type").and_then(Value::as_str).unwrap_or("").to_string();
                    let options = p
                        .pointer(&format!("/{kind}/options"))
                        .and_then(Value::as_array)
                        .map(|o| o.iter().filter_map(|x| x.get("name").and_then(Value::as_str).map(String::from)).collect())
                        .unwrap_or_default();
                    PropInfo { id: p.get("id").and_then(Value::as_str).unwrap_or("").into(), name: name.clone(), kind, options }
                })
                .collect()
        })
        .unwrap_or_default();
    properties.sort_by(|a, b| (a.kind != "title", &a.name).cmp(&(b.kind != "title", &b.name)));
    DbSchema {
        database_id: database_id.into(),
        data_source_id: ds.get("id").and_then(Value::as_str).map(String::from),
        title: rich_text_plain(db.get("title").unwrap_or(&Value::Null)),
        properties,
    }
}

fn kind_label(kind: &str) -> &str {
    match kind {
        "rich_text" => "text",
        "multi_select" => "multi-select",
        k => k,
    }
}

/// Problems with a saved mapping against the live schema, by property name. Empty means fine.
pub(crate) fn check_mapping(m: &Mapping, s: &DbSchema) -> Vec<String> {
    let mut out = Vec::new();
    let mut invalid = Vec::new();
    let mut check = |prop: &str, kind: &str, values: Vec<&str>| {
        let Some(p) = s.properties.iter().find(|p| p.name == prop) else {
            out.push(format!("“{prop}” is missing"));
            return;
        };
        if p.kind != kind {
            out.push(format!("“{prop}” is no longer a {} property", kind_label(kind)));
            return;
        }
        for v in values {
            if !p.options.iter().any(|o| o == v) {
                out.push(format!("“{prop}” has no option “{v}”"));
            }
        }
    };
    check(&m.title_prop, "title", vec![]);
    if let Some(p) = &m.due_prop {
        check(p, "date", vec![]);
    }
    if let Some(p) = &m.meeting_prop {
        check(p, "rich_text", vec![]);
    }
    for f in &m.fixed {
        let values = match (f.kind.as_str(), &f.value) {
            ("select" | "status", Value::String(v)) => vec![v.as_str()],
            ("multi_select", Value::Array(a)) if a.iter().all(Value::is_string) => a.iter().filter_map(Value::as_str).collect(),
            ("checkbox", Value::Bool(_)) => vec![],
            _ => {
                invalid.push(format!("“{}” has an invalid value", f.prop));
                continue;
            }
        };
        check(&f.prop, &f.kind, values);
    }
    out.extend(invalid);
    out
}

/// "20 Nov 2026" in `tz`. DB naive times are UTC; a bare date is shown as is.
pub(crate) fn display_date<Tz: TimeZone>(raw: &str, tz: &Tz) -> Option<String>
where
    Tz::Offset: Display,
{
    let iso = raw.replacen(' ', "T", 1);
    let dt = DateTime::parse_from_rfc3339(&iso)
        .map(|d| d.with_timezone(tz))
        .ok()
        .or_else(|| {
            NaiveDateTime::parse_from_str(&iso, "%Y-%m-%dT%H:%M:%S%.f").ok().map(|n| chrono::Utc.from_utc_datetime(&n).with_timezone(tz))
        });
    if let Some(d) = dt {
        return Some(d.format("%-d %b %Y").to_string());
    }
    if raw.len() == 10 {
        return NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok().map(|d| d.format("%-d %b %Y").to_string());
    }
    None
}

fn cut(s: &str) -> String {
    // Notion counts UTF-16 units like JS `.length`.
    s.chars().scan(0, |n, c| { *n += c.len_utf16(); (*n <= 2000).then_some(c) }).collect()
}

fn text_run(s: &str) -> Value {
    json!({"type":"text","text":{"content":cut(s)}})
}

fn clock(at: f64) -> String {
    let s = at.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

pub(crate) fn page_body(data_source_id: &str, m: &Mapping, t: &SendTask) -> Value {
    let date = t.meeting_date.as_deref().and_then(|d| display_date(d, &chrono::Local));
    let mut props = serde_json::Map::new();
    props.insert(m.title_prop.clone(), json!({"title":[text_run(&t.text)]}));
    if let (Some(p), Some(d)) = (&m.due_prop, &t.due_date) {
        props.insert(p.clone(), json!({"date":{"start":d.chars().take(10).collect::<String>()}}));
    }
    if let Some(p) = &m.meeting_prop {
        let v = match (&t.meeting_title, &date) {
            (Some(title), Some(d)) => format!("{title}, {d}"),
            (Some(title), None) => title.clone(),
            _ => "Deleted meeting".into(),
        };
        props.insert(p.clone(), json!({"rich_text":[text_run(&v)]}));
    }
    for f in &m.fixed {
        let v = match (f.kind.as_str(), &f.value) {
            ("select", Value::String(v)) => json!({"select":{"name":v}}),
            ("status", Value::String(v)) => json!({"status":{"name":v}}),
            ("multi_select", Value::Array(a)) => {
                json!({"multi_select": a.iter().filter_map(Value::as_str).map(|n| json!({"name":n})).collect::<Vec<_>>()})
            }
            ("checkbox", Value::Bool(b)) => json!({"checkbox":b}),
            _ => continue,
        };
        props.insert(f.prop.clone(), v);
    }

    let mut tail = date.map(|d| format!(", {d}")).unwrap_or_default();
    if let Some(at) = t.at {
        tail.push_str(&format!(" at {}", clock(at)));
    }
    let runs = match &t.meeting_title {
        Some(title) => {
            let mut r = vec![text_run("From "), json!({"type":"text","text":{"content":cut(title)},"annotations":{"italic":true}})];
            if !tail.is_empty() {
                r.push(text_run(&tail));
            }
            r
        }
        None => vec![text_run(&format!("From a deleted meeting{tail}"))],
    };
    let mut body = json!({
        "parent": {"data_source_id": data_source_id},
        "properties": props,
        "children": [{"object":"block","type":"paragraph","paragraph":{"rich_text":runs}}],
    });
    if let Some(e) = &m.emoji {
        body["icon"] = json!({"type":"emoji","emoji":e});
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notion::FixedValue;
    use chrono::FixedOffset;

    const ID: &str = "1a2b3c4d5e6f47a8b9c0d1e2f3a4b5c6";
    const DASHED: &str = "1a2b3c4d-5e6f-47a8-b9c0-d1e2f3a4b5c6";
    const VIEW: &str = "aaaaaaaabbbbccccddddeeeeeeeeeeee";
    const PAGE: &str = "99999999888877776666555555555555";

    #[test]
    fn parses_database_links_and_ids() {
        for l in [
            format!("https://www.notion.so/myws/Job-tasks-{ID}?v={VIEW}&pvs=4"),
            format!("https://www.notion.so/{ID}"),
            format!("https://www.notion.so/myws/{ID}?v={VIEW}&p={PAGE}&pm=s"),
            format!("https://acme.notion.site/Job-tasks-{ID}?pvs=4"),
            format!("notion://www.notion.so/myws/Job-tasks-{ID}"),
            format!("  {DASHED}  "),
            ID.to_uppercase(),
        ] {
            assert_eq!(parse_database_id(&l).as_deref(), Ok(DASHED), "{l}");
        }
        for l in ["https://www.notion.so/myws/Job-tasks", "", "../../users/me"] {
            assert_eq!(parse_database_id(l), Err(BAD_LINK.to_string()), "{l}");
        }
    }

    fn schema_json() -> (Value, Value) {
        let db = json!({"title":[{"plain_text":"Job "},{"plain_text":"tasks"}],"data_sources":[{"id":ID}]});
        let ds = json!({"id":"ds-1","properties":{
            "Urgent":{"id":"u","name":"Urgent","type":"checkbox","checkbox":{}},
            "Tags":{"id":"t","name":"Tags","type":"multi_select","multi_select":{"options":[{"name":"Work"}]}},
            "Status":{"id":"s","name":"Status","type":"status","status":{"options":[{"name":"To do"},{"name":"Done"}]}},
            "Meeting":{"id":"m","name":"Meeting","type":"rich_text","rich_text":{}},
            "Due":{"id":"d","name":"Due","type":"date","date":{}},
            "Name":{"id":"n","name":"Name","type":"title","title":{}}}});
        (db, ds)
    }

    #[test]
    fn reads_schema_title_first() {
        let (db, ds) = schema_json();
        let s = schema_from(DASHED, &db, &ds);
        assert_eq!(s.title, "Job tasks");
        assert_eq!(s.data_source_id.as_deref(), Some("ds-1"));
        let names: Vec<_> = s.properties.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Name", "Due", "Meeting", "Status", "Tags", "Urgent"]);
        assert_eq!(s.properties[3].options, ["To do", "Done"]);
        assert_eq!(first_data_source(&db).as_deref(), Some(DASHED));
        assert_eq!(first_data_source(&json!({"data_sources":[{"id":"zz"}]})), None);
    }

    fn mapping() -> Mapping {
        Mapping {
            title_prop: "Name".into(),
            due_prop: Some("Due".into()),
            meeting_prop: Some("Meeting".into()),
            fixed: vec![
                FixedValue { prop: "Status".into(), kind: "status".into(), value: json!("To do") },
                FixedValue { prop: "Tags".into(), kind: "multi_select".into(), value: json!(["Work"]) },
                FixedValue { prop: "Urgent".into(), kind: "checkbox".into(), value: json!(true) },
            ],
            emoji: None,
            ai_fill: true,
        }
    }

    #[test]
    fn page_body_sets_the_emoji_icon() {
        let t = task();
        let mut m = mapping();
        m.emoji = Some("👷".into());
        assert_eq!(page_body("ds", &m, &t)["icon"], json!({"type":"emoji","emoji":"👷"}));
        m.emoji = None;
        assert!(page_body("ds", &m, &t).get("icon").is_none());
    }

    #[test]
    fn check_mapping_reports_by_name() {
        let (db, ds) = schema_json();
        let s = schema_from(DASHED, &db, &ds);
        assert!(check_mapping(&mapping(), &s).is_empty());
        let mut m = mapping();
        m.due_prop = Some("Deadline".into());
        m.meeting_prop = Some("Due".into());
        m.fixed[0].value = json!("Later");
        assert_eq!(
            check_mapping(&m, &s),
            ["“Deadline” is missing", "“Due” is no longer a text property", "“Status” has no option “Later”"]
        );
    }

    #[test]
    fn check_mapping_reports_values_of_the_wrong_type() {
        let (db, ds) = schema_json();
        let s = schema_from(DASHED, &db, &ds);
        let mut m = mapping();
        m.fixed[0].value = json!(true);
        m.fixed[1].value = json!([1]);
        m.fixed[2].kind = "relation".into();
        assert_eq!(
            check_mapping(&m, &s),
            ["“Status” has an invalid value", "“Tags” has an invalid value", "“Urgent” has an invalid value"]
        );
    }

    fn task() -> SendTask {
        SendTask {
            id: "1".into(),
            text: "Send the deck".into(),
            at: Some(754.0),
            due_date: Some("2026-11-26".into()),
            meeting_title: Some("Weekly sync".into()),
            meeting_date: Some("2026-11-20T10:00:00+00:00".into()),
            dest_id: "d".into(),
        }
    }

    #[test]
    fn page_body_maps_every_property_kind() {
        let b = page_body("ds-1", &mapping(), &task());
        assert_eq!(b["parent"], json!({"data_source_id":"ds-1"}));
        let p = &b["properties"];
        assert_eq!(p["Name"], json!({"title":[{"type":"text","text":{"content":"Send the deck"}}]}));
        assert_eq!(p["Due"], json!({"date":{"start":"2026-11-26"}}));
        assert_eq!(p["Status"], json!({"status":{"name":"To do"}}));
        assert_eq!(p["Tags"], json!({"multi_select":[{"name":"Work"}]}));
        assert_eq!(p["Urgent"], json!({"checkbox":true}));
        assert_eq!(p["Meeting"]["rich_text"][0]["text"]["content"], "Weekly sync, 20 Nov 2026");
        let runs = b["children"][0]["paragraph"]["rich_text"].as_array().unwrap();
        let text: String = runs.iter().map(|r| r["text"]["content"].as_str().unwrap()).collect();
        assert_eq!(text, "From Weekly sync, 20 Nov 2026 at 12:34");
        assert_eq!(runs[1]["annotations"]["italic"], true);
        assert!(runs[0].get("annotations").is_none());
    }

    #[test]
    fn page_body_without_due_or_meeting() {
        let mut t = task();
        t.due_date = None;
        t.meeting_title = None;
        t.meeting_date = None;
        t.at = None;
        let b = page_body("ds-1", &mapping(), &t);
        assert!(b["properties"].get("Due").is_none());
        assert_eq!(b["properties"]["Meeting"]["rich_text"][0]["text"]["content"], "Deleted meeting");
        assert_eq!(b["children"][0]["paragraph"]["rich_text"][0]["text"]["content"], "From a deleted meeting");
    }

    #[test]
    fn long_text_is_cut_to_2000_chars() {
        let mut t = task();
        t.text = "я".repeat(2500);
        let b = page_body("ds-1", &mapping(), &t);
        assert_eq!(b["properties"]["Name"]["title"][0]["text"]["content"].as_str().unwrap().chars().count(), 2000);
        // Notion counts UTF-16 units: an emoji is 2, and is never split.
        t.text = "😀".repeat(1500);
        let b = page_body("ds-1", &mapping(), &t);
        assert_eq!(b["properties"]["Name"]["title"][0]["text"]["content"].as_str().unwrap().chars().count(), 1000);
    }

    #[test]
    fn meeting_day_uses_the_local_zone() {
        let tz = FixedOffset::east_opt(3600).unwrap();
        assert_eq!(display_date("2026-11-20 23:30:00", &tz).as_deref(), Some("21 Nov 2026"));
        assert_eq!(display_date("2026-11-20T23:30:00+00:00", &tz).as_deref(), Some("21 Nov 2026"));
        assert_eq!(display_date("2026-11-20", &tz).as_deref(), Some("20 Nov 2026"));
        assert_eq!(display_date("t", &tz), None);
    }
}
