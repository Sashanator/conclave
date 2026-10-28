//! Pure helpers for Windows' microphone consent store. No Win32 calls, so they test on any OS.

/// HKCU-relative path of the microphone consent store.
pub const CONSENT_PATH: &str =
    r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

const BROWSERS: [&str; 5] = ["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe"];

/// One app that has used the microphone, as recorded by Windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicUser {
    /// Lowercase exe file name (desktop apps) or package family name (packaged apps).
    pub app: String,
    /// Full exe path (desktop apps) or package family name (packaged apps). Tells apps with the same exe name apart.
    pub path: String,
    pub packaged: bool,
    pub in_use: bool,
}

/// Union registry usage with active capture sessions, preserving registry app identity on matching paths.
/// Paths are lowercase stable keys. Teams sessions reuse the first packaged Teams registry entry.
/// Inactive/expired sessions never add or clear usage.
pub fn merge_mic_users<'a>(
    registry: Vec<MicUser>,
    sessions: impl IntoIterator<Item = (&'a str, bool)>,
) -> Vec<MicUser> {
    let mut users: Vec<MicUser> = Vec::new();
    for mut user in registry {
        user.path = user.path.to_lowercase();
        if let Some(existing) = users.iter_mut().find(|u| u.path == user.path) {
            existing.in_use |= user.in_use;
        } else {
            users.push(user);
        }
    }
    for (path, active) in sessions {
        if !active {
            continue;
        }
        let path = path.to_lowercase();
        let app = path.rsplit('\\').next().unwrap_or(&path).to_string();
        if app == "ms-teams.exe" {
            if let Some(user) = users.iter_mut().find(|u| u.packaged && u.app.starts_with("msteams_")) {
                user.in_use = true;
                continue;
            }
        }
        if let Some(user) = users.iter_mut().find(|u| u.path == path) {
            user.in_use = true;
        } else {
            users.push(MicUser {
                app,
                path,
                packaged: false,
                in_use: true,
            });
        }
    }
    users
}

/// Desktop-app keys are full exe paths with `\` replaced by `#`; packaged-app keys are package family names.
pub fn app_name_from_key(key: &str, packaged: bool) -> String {
    let name = if packaged { key } else { key.rsplit('#').next().unwrap_or(key) };
    name.to_lowercase()
}

/// Full exe path for NonPackaged keys (`C:#dir#app.exe` → `C:\dir\app.exe`); the family name for packaged apps.
pub fn path_from_key(key: &str, packaged: bool) -> String {
    if packaged { key.to_string() } else { key.replace('#', "\\") }
}

/// FILETIME stamps: in use while the last start is newer than the last stop (stop == 0 means never stopped).
pub fn is_in_use(start: u64, stop: u64) -> bool {
    start != 0 && (stop == 0 || start > stop)
}

pub fn is_browser(app: &str) -> bool {
    BROWSERS.contains(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_teams_and_capture_session_are_one_detected_user() {
        use crate::classify::{classify, CallApp};
        let session = r"C:\Program Files\WindowsApps\MSTeams_26000.1.0_x64__8wekyb3d8bbwe\ms-teams.exe";
        for in_use in [false, true] {
            let users = merge_mic_users(vec![MicUser {
                app: "msteams_8wekyb3d8bbwe".into(), path: "MSTeams_8wekyb3d8bbwe".into(),
                packaged: true, in_use,
            }], [(session, true)]);
            assert_eq!(users.len(), 1, "packaged Teams and its session must merge");
            assert!(users[0].packaged);
            assert!(users[0].in_use);
            let detected: Vec<_> = users.iter().filter_map(|u| classify(u, &[])).collect();
            assert_eq!(detected.len(), 1);
            assert_eq!(detected[0].app, CallApp::Teams);
        }
    }

    #[test]
    fn session_only_teams_is_still_detected() {
        use crate::classify::{classify, CallApp};
        let users = merge_mic_users(vec![], [(r"C:\Teams\ms-teams.exe", true)]);
        assert_eq!(users.len(), 1);
        assert!(!users[0].packaged);
        assert!(users[0].in_use);
        assert_eq!(classify(&users[0], &[]).unwrap().app, CallApp::Teams);
    }

    #[test]
    fn session_only_user_counts_and_inactive_sessions_are_ignored() {
        let path = r"C:\Zoom\bin\Zoom.exe";
        let users = merge_mic_users(vec![], [(path, true), (r"C:\Other\Zoom.exe", false)]);
        assert_eq!(users, vec![MicUser {
            app: "zoom.exe".into(), path: path.to_lowercase(), packaged: false, in_use: true,
        }]);
        assert!(merge_mic_users(vec![], [(path, false)]).is_empty());
    }

    #[test]
    fn merge_deduplicates_paths_case_insensitively_and_preserves_registry_identity() {
        let registry = MicUser {
            app: "zoom.exe".into(), path: r"C:\Zoom\Zoom.exe".into(), packaged: false, in_use: false,
        };
        let users = merge_mic_users(vec![registry.clone()], [
            (r"c:\zoom\ZOOM.EXE", true), (r"C:\ZOOM\Zoom.exe", true),
        ]);
        assert_eq!(users, vec![MicUser { path: r"c:\zoom\zoom.exe".into(), in_use: true, ..registry }]);
        assert_eq!(merge_mic_users(vec![], [
            (r"C:\Zoom\Zoom.exe", true), (r"c:\zoom\ZOOM.EXE", true),
        ]).len(), 1);
        assert_eq!(merge_mic_users(vec![], [
            (r"C:\Zoom\Zoom.exe", true), (r"C:\Other\Zoom.exe", true),
        ]).len(), 2);
    }

    #[test]
    fn registry_only_users_still_work_and_inactive_sessions_do_not_clear_them() {
        let registry = vec![MicUser {
            app: "zoom.exe".into(), path: r"C:\Zoom\Zoom.exe".into(), packaged: false, in_use: true,
        }];
        let expected = vec![MicUser { path: r"c:\zoom\zoom.exe".into(), ..registry[0].clone() }];
        assert_eq!(merge_mic_users(registry.clone(), []), expected);
        assert_eq!(merge_mic_users(registry, [(r"c:\zoom\zoom.exe", false)]), expected);
    }

    #[test]
    fn nonpackaged_key_becomes_lowercase_exe_name() {
        assert_eq!(app_name_from_key(r"C:#Program Files#Zoom#bin#Zoom.exe", false), "zoom.exe");
        assert_eq!(app_name_from_key(r"C:#Users#me#AppData#Roaming#Zoom#bin#Zoom.exe", false), "zoom.exe");
    }

    #[test]
    fn packaged_key_is_lowercased_family_name() {
        assert_eq!(app_name_from_key("MSTeams_8wekyb3d8bbwe", true), "msteams_8wekyb3d8bbwe");
    }

    #[test]
    fn nonpackaged_key_decodes_to_full_path() {
        assert_eq!(
            path_from_key(r"C:#Program Files#Zoom#bin#Zoom.exe", false),
            r"C:\Program Files\Zoom\bin\Zoom.exe"
        );
    }

    #[test]
    fn same_exe_name_in_two_folders_gives_two_paths() {
        let a = path_from_key(r"C:#Users#me#AppData#Local#meetily#meetily.exe", false);
        let b = path_from_key(r"C:#Program Files#Conclave#meetily.exe", false);
        assert_ne!(a, b);
        assert_eq!(app_name_from_key(r"C:#Program Files#Conclave#meetily.exe", false), "meetily.exe");
    }

    #[test]
    fn packaged_key_path_is_the_family_name() {
        assert_eq!(path_from_key("Microsoft.Teams_8wekyb3d8bbwe", true), "Microsoft.Teams_8wekyb3d8bbwe");
    }

    #[test]
    fn in_use_rules() {
        assert!(!is_in_use(0, 0), "never used");
        assert!(is_in_use(100, 0), "started, not stopped");
        assert!(is_in_use(200, 100), "restarted after last stop");
        assert!(!is_in_use(100, 200), "stopped after start");
        assert!(!is_in_use(100, 100), "equal stamps = stopped");
    }

    #[test]
    fn browsers_are_recognised() {
        for b in ["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe"] {
            assert!(is_browser(b), "{b}");
        }
        assert!(!is_browser("zoom.exe"));
    }
}
