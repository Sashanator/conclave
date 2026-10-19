//! Hold/tap state machine for the Dictate hotkeys (spec §4).
//! Owned by the hook thread only; it has no locks, so it never blocks the hook.

use crate::hotkey::{modifier_bit, Hotkey, MOD_ALT, MOD_CTRL, MOD_WIN, VK_ESCAPE};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Main,
    Mode(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub hotkey: Hotkey,
    pub target: Target,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Start(Target),
    HandsFree(Target),
    Retarget(Target),
    Stop,
    Cancel,
    Abandon,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub action: Action,
    pub swallow: bool,
    pub mask_start_menu: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    Idle,
    /// `hotkey` is the combo that must stay down; `target` changes on Retarget.
    Armed {
        hotkey: Hotkey,
        target: Target,
        t0: u64,
    },
    HandsFree {
        target: Target,
    },
}

pub struct HotkeyMachine {
    bindings: Vec<Binding>,
    hold_threshold_ms: u64,
    /// Every key the OS reported down and not yet up.
    held: Vec<u16>,
    /// Keys whose down we swallowed; their up is swallowed too.
    swallow_ups: Vec<u16>,
    state: State,
}

impl HotkeyMachine {
    pub fn new(bindings: Vec<Binding>, hold_threshold_ms: u64) -> Self {
        Self {
            bindings,
            hold_threshold_ms,
            held: Vec::new(),
            swallow_ups: Vec::new(),
            state: State::Idle,
        }
    }

    pub fn set_bindings(&mut self, bindings: Vec<Binding>) {
        self.bindings = bindings;
    }

    pub fn on_key(&mut self, vk: u16, down: bool, now_ms: u64) -> Decision {
        let (action, swallow) = if !down {
            self.held.retain(|&k| k != vk);
            let swallow = match self.swallow_ups.iter().position(|&k| k == vk) {
                Some(i) => {
                    self.swallow_ups.swap_remove(i);
                    true
                }
                None => false,
            };
            (self.key_up(now_ms), swallow)
        } else if self.held.contains(&vk) {
            // Auto-repeat: the key is already down, so nothing changes.
            let swallow = modifier_bit(vk).is_none() && self.swallow_ups.contains(&vk);
            (Action::None, swallow)
        } else {
            self.held.push(vk);
            let (action, swallow) = self.key_down(vk, now_ms);
            if swallow {
                self.swallow_ups.push(vk);
            }
            (action, swallow)
        };
        let mask_start_menu = self.mask_for(&action);
        Decision {
            action,
            swallow,
            mask_start_menu,
        }
    }

    /// Drops keys the OS says are up (their key-up was missed, e.g. after Win+L).
    pub fn sync_held(&mut self, is_down: &dyn Fn(u16) -> bool) {
        self.held.retain(|&vk| is_down(vk));
        self.swallow_ups.retain(|&vk| is_down(vk));
    }

    /// Back to Idle. Held keys stay tracked, so a repeated down cannot re-arm.
    pub fn reset(&mut self) {
        self.state = State::Idle;
    }

    /// Returns the action and whether to swallow this key-down.
    fn key_down(&mut self, vk: u16, now_ms: u64) -> (Action, bool) {
        let non_modifier = modifier_bit(vk).is_none();
        match self.state.clone() {
            State::Idle => match self.completing() {
                Some(b) => {
                    let target = b.target.clone();
                    self.state = State::Armed {
                        hotkey: b.hotkey,
                        target: target.clone(),
                        t0: now_ms,
                    };
                    (Action::Start(target), non_modifier)
                }
                None => (Action::None, false),
            },
            State::Armed { hotkey, t0, .. } => {
                if let Some(b) = self.completing() {
                    if b.hotkey == hotkey {
                        return (Action::None, false);
                    }
                    let target = b.target.clone();
                    self.state = State::Armed {
                        hotkey,
                        target: target.clone(),
                        t0,
                    };
                    return (Action::Retarget(target), non_modifier);
                }
                if vk == VK_ESCAPE {
                    self.state = State::Idle;
                    return (Action::Cancel, true);
                }
                if self.within_threshold(t0, now_ms) {
                    self.state = State::Idle;
                    return (Action::Abandon, false);
                }
                (Action::None, false)
            }
            State::HandsFree { .. } => {
                if self.completing().is_some() {
                    self.state = State::Idle;
                    return (Action::Stop, non_modifier);
                }
                if vk == VK_ESCAPE {
                    self.state = State::Idle;
                    return (Action::Cancel, true);
                }
                (Action::None, false)
            }
        }
    }

    fn key_up(&mut self, now_ms: u64) -> Action {
        if let State::Armed { hotkey, target, t0 } = self.state.clone() {
            if !self.combo_down(&hotkey) {
                if self.within_threshold(t0, now_ms) {
                    self.state = State::HandsFree {
                        target: target.clone(),
                    };
                    return Action::HandsFree(target);
                }
                self.state = State::Idle;
                return Action::Stop;
            }
        }
        Action::None
    }

    fn within_threshold(&self, t0: u64, now_ms: u64) -> bool {
        now_ms.saturating_sub(t0) < self.hold_threshold_ms
    }

    /// Start-menu mask: Win is still down while the action is sent (spec §4).
    fn mask_for(&self, action: &Action) -> bool {
        let win_held = self.mods_held() & MOD_WIN != 0;
        win_held
            && matches!(
                action,
                Action::Start(_)
                    | Action::HandsFree(_)
                    | Action::Retarget(_)
                    | Action::Stop
                    | Action::Cancel
            )
    }

    fn mods_held(&self) -> u8 {
        self.held
            .iter()
            .filter_map(|&vk| modifier_bit(vk))
            .fold(0, |m, bit| m | bit)
    }

    /// The first binding that exactly matches the held keys, if any.
    fn completing(&self) -> Option<&Binding> {
        self.bindings
            .iter()
            .find(|b| self.matches_exactly(&b.hotkey))
    }

    fn matches_exactly(&self, h: &Hotkey) -> bool {
        if self.mods_held() != h.mods {
            return false;
        }
        let mut others = self.held.iter().filter(|&&vk| modifier_bit(vk).is_none());
        match h.key {
            None => others.next().is_none(),
            Some(key) => others.next() == Some(&key) && others.next().is_none(),
        }
    }

    fn combo_down(&self, h: &Hotkey) -> bool {
        (self.mods_held() & h.mods) == h.mods && h.key.map_or(true, |key| self.held.contains(&key))
    }
}

/// Rejects a modifier-only binding with fewer than two modifiers, a key binding
/// without Ctrl, Alt or Win (Shift alone would eat capitals), and duplicates.
pub fn validate_bindings(bindings: &[Binding]) -> Result<(), String> {
    for (i, b) in bindings.iter().enumerate() {
        let h = b.hotkey;
        match h.key {
            None if h.mods.count_ones() < 2 => {
                return Err(format!("'{}' needs at least two modifiers", h.display()));
            }
            Some(_) if h.mods & (MOD_CTRL | MOD_ALT | MOD_WIN) == 0 => {
                return Err(format!("'{}' needs Ctrl, Alt or Win", h.display()));
            }
            _ => {}
        }
        if bindings[..i].iter().any(|other| other.hotkey == h) {
            return Err(format!("'{}' is bound twice", h.display()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LCTRL: u16 = 0xA2;
    const LWIN: u16 = 0x5B;
    const RIGHT: u16 = 0x27;
    const P: u16 = 0x50;
    const ESC: u16 = 0x1B;
    const L: u16 = 0x4C;

    fn hk(s: &str) -> Hotkey {
        Hotkey::parse(s).unwrap()
    }

    fn bind(s: &str, target: Target) -> Binding {
        Binding {
            hotkey: hk(s),
            target,
        }
    }

    fn machine(bindings: Vec<Binding>) -> HotkeyMachine {
        HotkeyMachine::new(bindings, 300)
    }

    fn press(m: &mut HotkeyMachine, vk: u16, t: u64) -> Decision {
        m.on_key(vk, true, t)
    }

    fn release(m: &mut HotkeyMachine, vk: u16, t: u64) -> Decision {
        m.on_key(vk, false, t)
    }

    fn dec(action: Action, swallow: bool, mask_start_menu: bool) -> Decision {
        Decision {
            action,
            swallow,
            mask_start_menu,
        }
    }

    fn none() -> Decision {
        dec(Action::None, false, false)
    }

    #[test]
    fn validate_rules() {
        assert!(validate_bindings(&[bind("Ctrl", Target::Main)]).is_err());
        assert!(validate_bindings(&[bind("Shift+A", Target::Main)]).is_err());
        assert!(validate_bindings(&[bind("Ctrl+Win", Target::Main)]).is_ok());
        assert!(validate_bindings(&[bind("Ctrl+A", Target::Main)]).is_ok());
        assert!(validate_bindings(&[bind("Alt+Shift+F9", Target::Main)]).is_ok());
        assert!(validate_bindings(&[
            bind("Ctrl+Win", Target::Main),
            bind("Win+Ctrl", Target::Mode("x".into())),
        ])
        .is_err());
    }

    #[test]
    fn hold_starts_and_release_stops() {
        let mut m = machine(vec![bind("Ctrl+Win", Target::Main)]);
        assert_eq!(press(&mut m, LCTRL, 0), none());
        assert_eq!(
            press(&mut m, LWIN, 10),
            dec(Action::Start(Target::Main), false, true)
        );
        assert_eq!(release(&mut m, LWIN, 900), dec(Action::Stop, false, false));
    }

    #[test]
    fn release_ctrl_first_also_stops() {
        let mut m = machine(vec![bind("Ctrl+Win", Target::Main)]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(release(&mut m, LCTRL, 900), dec(Action::Stop, false, true));
        assert_eq!(release(&mut m, LWIN, 910), none());
    }

    #[test]
    fn quick_tap_goes_hands_free_then_tap_stops() {
        let mut m = machine(vec![bind("Ctrl+Win", Target::Main)]);
        press(&mut m, LCTRL, 0);
        assert_eq!(
            press(&mut m, LWIN, 0),
            dec(Action::Start(Target::Main), false, true)
        );
        assert_eq!(
            release(&mut m, LWIN, 100),
            dec(Action::HandsFree(Target::Main), false, false)
        );
        assert_eq!(release(&mut m, LCTRL, 150), none());
        press(&mut m, LCTRL, 2000);
        assert_eq!(press(&mut m, LWIN, 2000), dec(Action::Stop, false, true));
    }

    #[test]
    fn windows_shortcut_abandons() {
        let mut m = machine(vec![bind("Ctrl+Win", Target::Main)]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(press(&mut m, RIGHT, 50), dec(Action::Abandon, false, false));
    }

    #[test]
    fn stale_key_resynced() {
        let mut m = machine(vec![bind("Ctrl+Win", Target::Main)]);
        press(&mut m, L, 0);
        m.sync_held(&|_| false);
        press(&mut m, LCTRL, 10);
        assert_eq!(
            press(&mut m, LWIN, 10),
            dec(Action::Start(Target::Main), false, true)
        );
    }

    #[test]
    fn late_other_key_is_ignored() {
        let mut m = machine(vec![bind("Ctrl+Win", Target::Main)]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(press(&mut m, RIGHT, 400), none());
        assert_eq!(release(&mut m, LWIN, 500), dec(Action::Stop, false, false));
    }

    #[test]
    fn esc_cancels_and_is_swallowed() {
        let mut m = machine(vec![bind("Ctrl+Win", Target::Main)]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        release(&mut m, LWIN, 100);
        assert_eq!(press(&mut m, ESC, 200), dec(Action::Cancel, true, false));
        assert_eq!(release(&mut m, ESC, 210), dec(Action::None, true, false));
        assert_eq!(press(&mut m, ESC, 300), none());
    }

    #[test]
    fn retarget_to_superset_mode() {
        let mut m = machine(vec![
            bind("Ctrl+Win", Target::Main),
            bind("Ctrl+Win+P", Target::Mode("m".into())),
        ]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(
            press(&mut m, P, 50),
            dec(Action::Retarget(Target::Mode("m".into())), true, true)
        );
        assert!(release(&mut m, P, 60).swallow);
    }

    #[test]
    fn mode_binding_alone() {
        let mut m = machine(vec![bind("Ctrl+Win+P", Target::Mode("m".into()))]);
        press(&mut m, LCTRL, 0);
        assert_eq!(press(&mut m, LWIN, 0), none());
        assert_eq!(
            press(&mut m, P, 10),
            dec(Action::Start(Target::Mode("m".into())), true, true)
        );
    }

    #[test]
    fn autorepeat_ignored() {
        let mut m = machine(vec![bind("Ctrl+Win", Target::Main)]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(press(&mut m, LWIN, 50), none());
        assert_eq!(
            release(&mut m, LWIN, 100),
            dec(Action::HandsFree(Target::Main), false, false)
        );
    }

    #[test]
    fn reset_requires_release() {
        let mut m = machine(vec![bind("Ctrl+Win", Target::Main)]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        m.reset();
        assert_eq!(press(&mut m, LWIN, 50), none());
        release(&mut m, LCTRL, 60);
        release(&mut m, LWIN, 61);
        press(&mut m, LCTRL, 100);
        assert_eq!(
            press(&mut m, LWIN, 101),
            dec(Action::Start(Target::Main), false, true)
        );
    }
}
