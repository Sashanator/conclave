//! Hotkey strings ("Ctrl+Win+P") and the modifier/key codes the hook reports.

pub const MOD_CTRL: u8 = 1;
pub const MOD_ALT: u8 = 2;
pub const MOD_SHIFT: u8 = 4;
pub const MOD_WIN: u8 = 8;
pub const VK_ESCAPE: u16 = 0x1B;
/// Unassigned VK, sent while Win is held so Windows does not open Start on Win-up.
pub const VK_MASK: u16 = 0xE8;

/// `key` is a Windows virtual-key code and never a modifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hotkey {
    pub mods: u8,
    pub key: Option<u16>,
}

impl Hotkey {
    pub fn parse(s: &str) -> Result<Hotkey, String> {
        let (mut mods, mut key) = (0u8, None);
        for part in s.split('+') {
            let name = part.trim().to_ascii_lowercase();
            let bit = match name.as_str() {
                "ctrl" | "control" => MOD_CTRL,
                "alt" => MOD_ALT,
                "shift" => MOD_SHIFT,
                "win" | "windows" => MOD_WIN,
                _ => {
                    if key.is_some() {
                        return Err(format!("'{s}' has more than one key"));
                    }
                    key = Some(
                        parse_key(&name)
                            .ok_or_else(|| format!("'{}' is not an allowed key", part.trim()))?,
                    );
                    continue;
                }
            };
            if mods & bit != 0 {
                return Err(format!("'{s}' repeats a modifier"));
            }
            mods |= bit;
        }
        Ok(Hotkey { mods, key })
    }

    pub fn display(&self) -> String {
        let mut parts: Vec<String> = [
            (MOD_CTRL, "Ctrl"),
            (MOD_ALT, "Alt"),
            (MOD_SHIFT, "Shift"),
            (MOD_WIN, "Win"),
        ]
        .iter()
        .filter(|(bit, _)| self.mods & bit != 0)
        .map(|(_, name)| name.to_string())
        .collect();
        if let Some(key) = self.key {
            parts.push(key_name(key));
        }
        parts.join("+")
    }
}

pub fn modifier_bit(vk: u16) -> Option<u8> {
    match vk {
        0x11 | 0xA2 | 0xA3 => Some(MOD_CTRL),
        0x12 | 0xA4 | 0xA5 => Some(MOD_ALT),
        0x10 | 0xA0 | 0xA1 => Some(MOD_SHIFT),
        0x5B | 0x5C => Some(MOD_WIN),
        _ => None,
    }
}

/// `name` is already lower-case.
fn parse_key(name: &str) -> Option<u16> {
    if name == "space" {
        return Some(0x20);
    }
    if name.len() == 1 && name.as_bytes()[0].is_ascii_alphanumeric() {
        return Some(name.to_ascii_uppercase().as_bytes()[0] as u16);
    }
    let digits = name.strip_prefix('f')?;
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n: u16 = digits.parse().ok()?;
    (1..=24).contains(&n).then(|| 0x6F + n)
}

fn key_name(vk: u16) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x20 => "Space".to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => format!("VK{vk:02X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_round_trips() {
        assert_eq!(Hotkey::parse("win+ctrl").unwrap().display(), "Ctrl+Win");
        assert_eq!(Hotkey::parse("Ctrl+Win+p").unwrap().display(), "Ctrl+Win+P");
        assert!(Hotkey::parse("Ctrl").is_ok());
        assert!(Hotkey::parse("Ctrl+A+B").is_err());
        assert!(Hotkey::parse("").is_err());
        assert!(Hotkey::parse("Ctrl+Ctrl").is_err());
        assert!(Hotkey::parse("Ctrl+Tab").is_err());
    }
}
