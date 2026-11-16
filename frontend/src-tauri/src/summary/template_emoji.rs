// One-time emoji suggestion for a custom template. The reply is untrusted: it is validated as one emoji and
// applied only if the user has not set an emoji in the meantime. Every failure is returned to `suggest`'s
// caller, which logs it; the template file is only ever replaced through `save_template_in`.

use crate::knowledge::extract::UNTRUSTED;
use crate::summary::llm_target::complete;
use crate::summary::templates::{
    get_template_in, save_template_in, valid_emoji, HistorySource, SaveOrigin, Template,
};
use crate::summary::{template_assist_settings, templates};
use crate::usage::{self, Purpose};
use log::warn;
use std::path::Path;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

const TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) fn emoji_prompts(t: &Template) -> (String, String) {
    let system = format!(
        "You pick one emoji that represents a meeting-notes template. Reply with that single emoji and nothing else.\n{UNTRUSTED}"
    );
    let name = t.name.replace("</", "< /");
    let use_when = t.use_when.as_deref().unwrap_or("").replace("</", "< /");
    let user = format!("<name>\n{name}\n</name>\n<use_when>\n{use_when}\n</use_when>");
    (system, user)
}

/// The reply must be exactly one emoji token: no text, no markup, at most 32 bytes.
pub(crate) fn parse_emoji_reply(raw: &str) -> Option<String> {
    let s = raw.trim();
    (valid_emoji(s) && !s.contains('<')).then(|| s.to_string())
}

/// Re-reads the file and saves only if the emoji is still unset. Ok(false): nothing to do.
pub(crate) async fn apply_suggestion_in(dir: &Path, id: &str, emoji: &str) -> Result<bool, String> {
    let mut t = match get_template_in(dir, id) {
        Ok(t) => t,
        Err(_) if !dir.join(format!("{id}.json")).exists() => return Ok(false),
        Err(e) => return Err(e),
    };
    if t.emoji.is_some() {
        return Ok(false);
    }
    t.emoji = Some(emoji.to_string());
    let origin = SaveOrigin { source: HistorySource::Ai, note: "emoji suggestion".into() };
    save_template_in(dir, Some(id), &t, &origin)?;
    Ok(true)
}

async fn suggest_inner<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<(), String> {
    let t = templates::get_template(id)?;
    if t.emoji.is_some() {
        return Ok(());
    }
    let state = app.try_state::<crate::state::AppState>().ok_or("no database")?;
    let pool = state.db_manager.pool();
    let target = template_assist_settings::target(pool).await?;
    let app_data_dir = app.path().app_data_dir().ok();
    let (system, user) = emoji_prompts(&t);
    let c = tokio::time::timeout(
        TIMEOUT,
        complete(&target, &system, &user, Some(0.2), app_data_dir.as_ref()),
    )
    .await
    .map_err(|_| "timed out".to_string())??;
    usage::record(pool, target.provider.as_str(), &target.model, Purpose::TemplateAssist, None, None, c.usage.as_ref()).await;
    let emoji = parse_emoji_reply(&c.content).ok_or("reply was not a single emoji")?;
    let dir = templates::templates_dir().ok_or("Cannot find the app data folder")?;
    apply_suggestion_in(&dir, id, &emoji).await?;
    Ok(())
}

/// Background task: a failure is a warning only, so the next save retries.
pub(crate) async fn suggest<R: Runtime>(app: AppHandle<R>, id: String) {
    if let Err(e) = suggest_inner(&app, &id).await {
        warn!("template_emoji: no suggestion for '{id}': {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn custom(emoji: Option<&str>) -> Template {
        let mut t: Template = serde_json::from_str(
            r#"{"name":"Weekly","use_when":"team sync","sections":[{"title":"A","instruction":"B"}]}"#,
        )
        .unwrap();
        t.emoji = emoji.map(str::to_string);
        t
    }

    #[test]
    fn parses_only_one_emoji() {
        assert_eq!(parse_emoji_reply(" 📝 ").as_deref(), Some("📝"));
        for bad in ["📝 Notes", "Notes", "", "<b>x</b>", &"📝".repeat(10)] {
            assert_eq!(parse_emoji_reply(bad), None, "{bad}");
        }
    }

    #[test]
    fn prompts_hold_name_and_use_when() {
        let (s, u) = emoji_prompts(&custom(None));
        assert!(s.contains(UNTRUSTED));
        assert!(u.contains("Weekly") && u.contains("team sync"));
    }

    #[tokio::test]
    async fn applies_only_while_unset() {
        let dir = tempdir().unwrap();
        let o = SaveOrigin::default();
        let id = save_template_in(dir.path(), None, &custom(None), &o).unwrap();
        assert!(apply_suggestion_in(dir.path(), &id, "📝").await.unwrap());
        assert_eq!(get_template_in(dir.path(), &id).unwrap().emoji.as_deref(), Some("📝"));
        assert!(!apply_suggestion_in(dir.path(), &id, "🎯").await.unwrap());

        for set in ["", "🎯"] {
            let id = save_template_in(dir.path(), None, &custom(Some(set)), &o).unwrap();
            assert!(!apply_suggestion_in(dir.path(), &id, "📝").await.unwrap());
            assert_eq!(get_template_in(dir.path(), &id).unwrap().emoji.as_deref(), Some(set));
        }
        assert!(!apply_suggestion_in(dir.path(), "gone", "📝").await.unwrap());
    }
}
