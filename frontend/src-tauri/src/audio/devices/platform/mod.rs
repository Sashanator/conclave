// Platform-specific audio device implementations

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

// Re-export platform-specific functions
#[cfg(target_os = "windows")]
pub use windows::{configure_windows_audio, get_windows_device};

#[cfg(target_os = "macos")]
pub use macos::configure_macos_audio;

#[cfg(target_os = "linux")]
pub use linux::configure_linux_audio;

/// Index of the device to use for `base`: an exact name match wins over the first substring match.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn pick_device_index(names: &[String], base: &str) -> Option<usize> {
    names
        .iter()
        .position(|n| n == base)
        .or_else(|| names.iter().position(|n| n.contains(base)))
}

#[cfg(test)]
mod tests {
    use super::pick_device_index;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn exact_name_beats_an_earlier_substring_match() {
        let devices = names(&["Headset Microphone (Jabra)", "Microphone (Jabra)"]);
        assert_eq!(pick_device_index(&devices, "Microphone (Jabra)"), Some(1));
    }

    #[test]
    fn substring_is_the_fallback_when_no_exact_name() {
        let devices = names(&["Speakers", "Headset Microphone (Jabra)"]);
        assert_eq!(pick_device_index(&devices, "Microphone (Jabra)"), Some(1));
    }

    #[test]
    fn no_match_is_none() {
        let devices = names(&["Speakers", "Built-in Mic"]);
        assert_eq!(pick_device_index(&devices, "Microphone (Jabra)"), None);
    }
}
