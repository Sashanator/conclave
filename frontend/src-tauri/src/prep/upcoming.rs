//! Upcoming meeting preps.

use super::{store, MeetingPrep};
use crate::{
    knowledge::{
        briefs::event_type,
        calendar::{self, Occurrence},
        settings,
    },
    state::AppState,
    summary::templates::{self, TemplateEntry},
};
use chrono::{DateTime, Duration, Utc};
use log::warn;
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::HashSet;
use tauri::State;

pub(crate) const PREP_AHEAD_DAYS: i64 = 7;

pub(crate) fn moves(
    preps: &[MeetingPrep],
    linked: &HashSet<(String, String)>,
    occ: &[Occurrence],
    now: DateTime<Utc>,
) -> Vec<(i64, String)> {
    let keys: HashSet<_> = occ
        .iter()
        .map(|o| (o.uid.clone(), o.start.to_rfc3339()))
        .collect();
    preps
        .iter()
        .filter_map(|prep| {
            let key = (prep.event_uid.clone(), prep.event_start.clone());
            if keys.contains(&key)
                || linked.contains(&key)
                || occ.iter().any(|o| o.uid == prep.event_uid && o.recurring)
            {
                return None;
            }
            let start = DateTime::parse_from_rfc3339(&prep.event_start).ok()?;
            if now >= start - Duration::minutes(10) {
                return None;
            }
            let mut candidates = occ
                .iter()
                .filter(|o| o.uid == prep.event_uid && o.start > now);
            let target = candidates.next()?;
            if candidates.next().is_some() {
                return None;
            }
            let new_start = target.start.to_rfc3339();
            if preps.iter().any(|p| {
                p.id != prep.id && p.event_uid == prep.event_uid && p.event_start == new_start
            }) {
                return None;
            }
            Some((prep.id, new_start))
        })
        .collect()
}

pub(crate) async fn relocate(
    pool: &SqlitePool,
    occ: &[Occurrence],
    now: DateTime<Utc>,
) -> Result<usize, String> {
    let preps = store::list_all(pool).await?;
    // calendar_title_now supplies to_rfc3339(), stored unchanged in meetings.
    // A UID can have a NULL calendar_start; that row cannot link to a prep key.
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT DISTINCT calendar_uid, calendar_start FROM meetings WHERE calendar_uid IS NOT NULL",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let linked = rows
        .into_iter()
        .filter_map(|(uid, start)| start.map(|start| (uid, start)))
        .collect();
    let planned = moves(&preps, &linked, occ, now);
    let mut count = 0;
    for prep in &preps {
        if let Some((id, start)) = planned.iter().find(|(id, _)| *id == prep.id) {
            count += sqlx::query("UPDATE meeting_preps SET event_start = ? WHERE id = ? AND event_start = ? AND NOT EXISTS (SELECT 1 FROM meeting_preps WHERE event_uid = ? AND event_start = ?) AND NOT EXISTS (SELECT 1 FROM meetings WHERE calendar_uid = ? AND calendar_start = ?)")
                .bind(start).bind(id).bind(&prep.event_start).bind(&prep.event_uid).bind(start)
                .bind(&prep.event_uid).bind(&prep.event_start)
                .execute(pool).await.map_err(|e| e.to_string())?.rows_affected() as usize;
        }
    }
    Ok(count)
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpcomingEvent {
    pub uid: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub template_id: Option<String>,
    pub template_name: Option<String>,
    pub prep: Option<PrepBadge>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrepBadge {
    pub id: i64,
    pub approved: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OtherPrep {
    pub id: i64,
    pub event_uid: String,
    pub event_start: String,
    pub event_title: String,
    pub approved: bool,
    pub reason: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrepOverview {
    pub connected: bool,
    pub upcoming: Vec<UpcomingEvent>,
    pub other: Vec<OtherPrep>,
}

pub(crate) fn overview(
    preps: &[MeetingPrep],
    occ: &[Occurrence],
    templates: &[TemplateEntry],
    now: DateTime<Utc>,
    connected: bool,
) -> PrepOverview {
    let mut shown = HashSet::new();
    let mut upcoming = Vec::new();
    let mut occurrences: Vec<_> = occ.iter().collect();
    occurrences.sort_by_key(|o| o.start);
    for o in occurrences {
        let start = o.start.to_rfc3339();
        let prep = preps
            .iter()
            .find(|p| p.event_uid == o.uid && p.event_start == start);
        if !(o.end > now && (o.start < now + Duration::days(PREP_AHEAD_DAYS) || prep.is_some())) {
            continue;
        }
        let template = prep
            .and_then(|p| p.template_id.as_ref())
            .and_then(|id| templates.iter().find(|t| &t.id == id))
            .or_else(|| event_type(&o.title, templates));
        if let Some(p) = prep {
            shown.insert(p.id);
        }
        upcoming.push(UpcomingEvent {
            uid: o.uid.clone(),
            title: o.title.clone(),
            start,
            end: o.end.to_rfc3339(),
            template_id: template.map(|t| t.id.clone()),
            template_name: template.map(|t| t.template.name.clone()),
            prep: prep.map(|p| PrepBadge {
                id: p.id,
                approved: p.approved_at.is_some(),
            }),
        });
    }
    let other = preps
        .iter()
        .filter(|p| !shown.contains(&p.id))
        .map(|p| OtherPrep {
            id: p.id,
            event_uid: p.event_uid.clone(),
            event_start: p.event_start.clone(),
            event_title: p.event_title.clone(),
            approved: p.approved_at.is_some(),
            reason: if DateTime::parse_from_rfc3339(&p.event_start)
                .map_or(true, |start| start < now)
            {
                "past"
            } else {
                "not_in_calendar"
            }
            .into(),
        })
        .collect();
    PrepOverview {
        connected,
        upcoming,
        other,
    }
}

#[tauri::command]
pub async fn prep_overview(state: State<'_, AppState>) -> Result<PrepOverview, String> {
    let connected = settings::current().calendar_url.is_some();
    let occ = calendar::upcoming();
    let now = Utc::now();
    let pool = state.db_manager.pool();
    if connected && !occ.is_empty() {
        relocate(pool, &occ, now).await?;
    }
    let preps = store::list_all(pool).await?;
    let listing = tauri::async_runtime::spawn_blocking(templates::list_templates)
        .await
        .map_err(|e| format!("templates not read: {e}"))?;
    if !listing.errors.is_empty() {
        warn!(
            "prep: {} template file(s) could not be read",
            listing.errors.len()
        );
    }
    let templates = listing.templates;
    Ok(overview(&preps, &occ, &templates, now, connected))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::calendar::Occurrence;
    use crate::{
        database::repositories::test_pool,
        prep::{store, ChatMessage, MeetingPrep},
        summary::templates::{Template, TemplateEntry},
    };
    use chrono::{DateTime, Duration, Utc};
    use std::collections::HashSet;

    fn now() -> DateTime<Utc> {
        "2026-10-29T12:00:00+00:00".parse().unwrap()
    }

    fn occurrence(uid: &str, days: i64) -> Occurrence {
        let start = now() + Duration::days(days);
        Occurrence {
            uid: uid.into(),
            title: "Planning".into(),
            start,
            end: start + Duration::hours(1),
            recurring: false,
        }
    }

    fn prep(id: i64, uid: &str, days: i64) -> MeetingPrep {
        MeetingPrep {
            id,
            event_uid: uid.into(),
            event_start: occurrence(uid, days).start.to_rfc3339(),
            event_title: "Planning".into(),
            template_id: None,
            material: "Material".into(),
            chat: vec![ChatMessage {
                role: "user".into(),
                text: "Question".into(),
                at: now().to_rfc3339(),
                list_not_updated: false,
            }],
            items: vec![],
            approved_at: None,
            created_at: now().to_rfc3339(),
            updated_at: now().to_rfc3339(),
        }
    }

    async fn insert(pool: &sqlx::SqlitePool, p: &MeetingPrep) {
        sqlx::query("INSERT INTO meeting_preps (id, event_uid, event_start, event_title, material, chat_json, items_json, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, '[]', ?, ?)")
            .bind(p.id).bind(&p.event_uid).bind(&p.event_start).bind(&p.event_title).bind(&p.material)
            .bind(serde_json::to_string(&p.chat).unwrap()).bind(&p.created_at).bind(&p.updated_at)
            .execute(pool).await.unwrap();
    }

    #[test]
    fn one_off_moved_event_carries_its_prep() {
        let target = occurrence("u", 2);
        assert_eq!(
            moves(
                &[prep(1, "u", 1)],
                &HashSet::new(),
                &[target.clone()],
                now()
            ),
            vec![(1, target.start.to_rfc3339())]
        );
    }

    #[test]
    fn cancelled_weekly_instance_does_not_move() {
        let mut target = occurrence("u", 8);
        target.recurring = true;
        assert!(moves(&[prep(1, "u", 1)], &HashSet::new(), &[target], now()).is_empty());
    }

    #[test]
    fn prep_inside_recording_window_does_not_move() {
        for minutes in [5, 10] {
            let mut p = prep(1, "u", 1);
            p.event_start = (now() + Duration::minutes(minutes)).to_rfc3339();
            assert!(moves(&[p], &HashSet::new(), &[occurrence("u", 2)], now()).is_empty());
        }
    }

    #[test]
    fn ended_occurrence_with_prep_is_past_in_other() {
        let mut ended = occurrence("u", 0);
        ended.start = now() - Duration::hours(1);
        ended.end = ended.start + Duration::minutes(30);
        let mut p = prep(1, "u", 0);
        p.event_start = ended.start.to_rfc3339();
        let result = overview(&[p], &[ended], &[], now(), true);
        assert!(result.upcoming.is_empty());
        assert_eq!(result.other.len(), 1);
        assert_eq!(result.other[0].id, 1);
        assert_eq!(result.other[0].reason, "past");
    }

    #[test]
    fn recurring_does_not_move() {
        assert!(moves(
            &[prep(1, "u", 1)],
            &HashSet::new(),
            &[occurrence("u", 2), occurrence("u", 3)],
            now()
        )
        .is_empty());
    }

    #[test]
    fn linked_or_past_prep_does_not_move() {
        let p = prep(1, "u", 1);
        let linked = HashSet::from([(p.event_uid.clone(), p.event_start.clone())]);
        assert!(moves(&[p], &linked, &[occurrence("u", 2)], now()).is_empty());
        for days in [-1, 0] {
            assert!(moves(
                &[prep(1, "u", days)],
                &HashSet::new(),
                &[occurrence("u", 2)],
                now()
            )
            .is_empty());
        }
        let mut invalid = prep(1, "u", 1);
        invalid.event_start = "invalid".into();
        assert!(moves(&[invalid], &HashSet::new(), &[occurrence("u", 2)], now()).is_empty());
    }

    #[test]
    fn existing_occurrence_and_nonfuture_targets_do_not_move() {
        let p = prep(1, "u", 1);
        assert!(moves(
            &[p.clone()],
            &HashSet::new(),
            &[occurrence("u", 1), occurrence("u", 2)],
            now()
        )
        .is_empty());
        assert!(moves(
            &[p.clone()],
            &HashSet::new(),
            &[occurrence("u", 0), occurrence("u", -1)],
            now()
        )
        .is_empty());
        assert_eq!(
            moves(
                &[p],
                &HashSet::new(),
                &[occurrence("u", -1), occurrence("u", 2)],
                now()
            ),
            vec![(1, occurrence("u", 2).start.to_rfc3339())]
        );
    }

    #[tokio::test]
    async fn move_skips_a_slot_that_already_has_a_prep() {
        let pool = test_pool().await;
        let preps = vec![prep(1, "u", 1), prep(2, "u", 2)];
        for p in &preps {
            insert(&pool, p).await;
        }
        let occ = [occurrence("u", 2)];
        assert!(moves(&preps, &HashSet::new(), &occ, now()).is_empty());
        assert_eq!(relocate(&pool, &occ, now()).await, Ok(0));
        assert_eq!(store::list_all(&pool).await.unwrap(), preps);
    }

    #[tokio::test]
    async fn relocate_updates_the_row() {
        let pool = test_pool().await;
        let mut p = prep(1, "u", 1);
        insert(&pool, &p).await;
        let target = occurrence("u", 2);
        assert_eq!(relocate(&pool, &[target.clone()], now()).await, Ok(1));
        assert!(store::get(&pool, "u", &p.event_start)
            .await
            .unwrap()
            .is_none());
        p.event_start = target.start.to_rfc3339();
        assert_eq!(
            store::get(&pool, "u", &p.event_start).await.unwrap(),
            Some(p)
        );
    }

    #[tokio::test]
    async fn relocate_respects_database_meeting_links() {
        let pool = test_pool().await;
        let p = prep(1, "u", 1);
        insert(&pool, &p).await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, calendar_uid, calendar_start) VALUES ('m', 'Meeting', 't', 't', ?, ?)")
            .bind(&p.event_uid).bind(&p.event_start).execute(&pool).await.unwrap();
        assert_eq!(relocate(&pool, &[occurrence("u", 2)], now()).await, Ok(0));
        assert_eq!(store::list_all(&pool).await.unwrap(), vec![p]);
    }

    #[test]
    fn overview_lists_week_and_orphans() {
        let mut approved = prep(1, "saved", 10);
        approved.approved_at = Some(now().to_rfc3339());
        let mut invalid = prep(4, "invalid", 1);
        invalid.event_start = "invalid".into();
        let preps = vec![
            approved,
            prep(2, "orphan", 2),
            prep(3, "past", -1),
            invalid,
            prep(5, "cached-past", -2),
        ];
        let occ = vec![
            occurrence("saved", 10),
            occurrence("far", 10),
            occurrence("week", 3),
            occurrence("cached-past", -2),
            occurrence("boundary", 7),
            occurrence("expired", -1),
        ];
        let result = overview(&preps, &occ, &[], now(), true);
        assert!(result.connected);
        assert_eq!(
            result
                .upcoming
                .iter()
                .map(|e| e.uid.as_str())
                .collect::<Vec<_>>(),
            vec!["week", "saved"]
        );
        assert_eq!(
            result.upcoming[1].prep,
            Some(PrepBadge {
                id: 1,
                approved: true
            })
        );
        assert_eq!(result.upcoming[0].prep, None);
        assert!(!result.other.iter().find(|p| p.id == 5).unwrap().approved);
        assert_eq!(
            result
                .other
                .iter()
                .map(|p| (p.id, p.reason.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (2, "not_in_calendar"),
                (3, "past"),
                (4, "past"),
                (5, "past")
            ]
        );
        assert!(!overview(&[], &[], &[], now(), false).connected);
    }

    fn template(id: &str, name: &str, keywords: &[&str]) -> TemplateEntry {
        TemplateEntry {
            id: id.into(),
            template: serde_json::from_value::<Template>(serde_json::json!({
                "name": name, "title_keywords": keywords, "sections": []
            }))
            .unwrap(),
        }
    }

    #[test]
    fn overview_template_prefers_prep_choice() {
        let templates = vec![
            template("planning", "Planning", &["Planning"]),
            template("interview", "Interview", &[]),
        ];
        let mut p = prep(1, "u", 1);
        p.template_id = Some("interview".into());
        let mut occ = vec![
            occurrence("u", 1),
            occurrence("keyword", 2),
            occurrence("general", 3),
        ];
        occ[2].title = "Unmatched".into();
        let result = overview(&[p.clone()], &occ, &templates, now(), true);
        assert_eq!(result.upcoming[0].template_id.as_deref(), Some("interview"));
        assert_eq!(
            result.upcoming[0].template_name.as_deref(),
            Some("Interview")
        );
        assert_eq!(
            result.upcoming[1].template_name.as_deref(),
            Some("Planning")
        );
        assert_eq!(result.upcoming[2].template_name, None);
        assert_eq!(result.upcoming[2].template_id, None);
        p.template_id = Some("removed".into());
        assert_eq!(
            overview(&[p], &occ, &templates, now(), true).upcoming[0]
                .template_id
                .as_deref(),
            Some("planning")
        );
    }
}
