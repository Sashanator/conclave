//! Pure helpers for Windows' microphone consent store. No Win32 calls, so they test on any OS.

/// HKCU-relative path of the microphone consent store.
pub const CONSENT_PATH: &str =
    r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

const BROWSERS: [&str; 4] = ["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe"];

/// One app that has used the microphone, as recorded by Windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicUser {
    /// Lowercase exe file name (desktop apps) or package family name (packaged apps).
    pub app: String,
    pub packaged: bool,
    pub in_use: bool,
}

/// Desktop-app keys are full exe paths with `\` replaced by `#`; packaged-app keys are package family names.
pub fn app_name_from_key(key: &str, packaged: bool) -> String {
    let name = if packaged { key } else { key.rsplit('#').next().unwrap_or(key) };
    name.to_lowercase()
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
    fn nonpackaged_key_becomes_lowercase_exe_name() {
        assert_eq!(app_name_from_key(r"C:#Program Files#Zoom#bin#Zoom.exe", false), "zoom.exe");
        assert_eq!(app_name_from_key(r"C:#Users#me#AppData#Roaming#Zoom#bin#Zoom.exe", false), "zoom.exe");
    }

    #[test]
    fn packaged_key_is_lowercased_family_name() {
        assert_eq!(app_name_from_key("MSTeams_8wekyb3d8bbwe", true), "msteams_8wekyb3d8bbwe");
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
        for b in ["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe"] {
            assert!(is_browser(b), "{b}");
        }
        assert!(!is_browser("zoom.exe"));
    }
}
