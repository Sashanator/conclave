//! Hold/tap state machine for the Dictate hotkeys (spec §4).
//! Owned by the hook thread only; it has no locks, so it never blocks the hook.

use crate::hotkey::{modifier_bit, Hotkey, MOD_ALT, MOD_CTRL, MOD_SHIFT, MOD_WIN, VK_ESCAPE};

/// Largest gap between two downs of a key that is still auto-repeat. Windows'
/// keyboard delay is at most 1000 ms, and keyboards time their own repeat with
/// about ±20% tolerance, so 1300 ms leaves room for that.
const MAX_REPEAT_GAP_MS: u32 = 1300;

/// Milliseconds from `from` to `to` on the wrapping KBDLLHOOKSTRUCT.time tick.
/// A stamp that steps backwards reads as 0, not as a ~49-day gap.
fn elapsed_ms(from: u32, to: u32) -> u32 {
    let gap = to.wrapping_sub(from);
    if gap > i32::MAX as u32 {
        0
    } else {
        gap
    }
}

/// True when, while Idle, a down of a swallowed key `now - last_down` ms after its
/// last down is a new press (its key-up was lost) rather than auto-repeat.
pub fn is_fresh_press(last_down: u32, now: u32) -> bool {
    elapsed_ms(last_down, now) > MAX_REPEAT_GAP_MS
}

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
        t0: u32,
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
    /// Swallowed keys with the time of their last down; their up is swallowed too.
    swallow_ups: Vec<(u16, u32)>,
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

    /// True when no dictation is armed or hands-free.
    pub fn is_idle(&self) -> bool {
        matches!(self.state, State::Idle)
    }

    /// `now` is KBDLLHOOKSTRUCT.time (ms, wrapping).
    pub fn on_key(&mut self, vk: u16, down: bool, now: u32) -> Decision {
        let (action, swallow) = if !down {
            self.held.retain(|&k| k != vk);
            let swallow = match self.swallow_ups.iter().position(|&(k, _)| k == vk) {
                Some(i) => {
                    self.swallow_ups.swap_remove(i);
                    true
                }
                None => false,
            };
            (self.key_up(now), swallow)
        } else {
            if let Some(i) = self.swallow_ups.iter().position(|&(k, _)| k == vk) {
                // During a session every down of a swallowed key is a repeat.
                if matches!(self.state, State::Idle) && is_fresh_press(self.swallow_ups[i].1, now) {
                    // The key-up of the swallowed down was lost: this is a new press.
                    self.swallow_ups.swap_remove(i);
                    self.held.retain(|&k| k != vk);
                } else {
                    self.swallow_ups[i].1 = now;
                }
            }
            if self.held.contains(&vk) {
                // Auto-repeat: the key is already down, so nothing changes.
                let swallow = modifier_bit(vk).is_none() && self.is_swallowed(vk);
                (Action::None, swallow)
            } else {
                self.held.push(vk);
                let (action, swallow) = self.key_down(vk, now);
                if swallow {
                    self.swallow_ups.push((vk, now));
                }
                (action, swallow)
            }
        };
        let mask_start_menu = self.mask_for(&action);
        Decision {
            action,
            swallow,
            mask_start_menu,
        }
    }

    /// Drops keys the OS says are up (their key-up was missed, e.g. after Win+L).
    /// An `Armed` session whose combo is no longer down is cancelled: its
    /// key-up will never arrive, so the session would otherwise run on.
    /// Once Idle, swallowed keys whose last down is stale are forgotten too.
    #[must_use]
    pub fn sync_held(&mut self, is_down: &dyn Fn(u16) -> bool, now: u32) -> Action {
        self.retain_held(is_down);
        let action = match self.state.clone() {
            State::Armed { hotkey, .. } if !self.combo_down(&hotkey) => {
                self.state = State::Idle;
                Action::Cancel
            }
            _ => Action::None,
        };
        if matches!(self.state, State::Idle) {
            self.swallow_ups
                .retain(|&(_, t)| elapsed_ms(t, now) <= MAX_REPEAT_GAP_MS);
            self.retain_held(is_down);
        }
        action
    }

    /// A swallowed key never updates the OS async state, so keep it until its up.
    fn retain_held(&mut self, is_down: &dyn Fn(u16) -> bool) {
        let swallow_ups = &self.swallow_ups;
        self.held
            .retain(|&vk| swallow_ups.iter().any(|&(k, _)| k == vk) || is_down(vk));
    }

    /// Back to Idle. Held keys stay tracked, so a repeated down cannot re-arm.
    pub fn reset(&mut self) {
        self.state = State::Idle;
    }

    /// Returns the action and whether to swallow this key-down.
    fn key_down(&mut self, vk: u16, now: u32) -> (Action, bool) {
        let non_modifier = modifier_bit(vk).is_none();
        match self.state.clone() {
            State::Idle => match self.completing(vk) {
                Some(b) => {
                    let target = b.target.clone();
                    self.state = State::Armed {
                        hotkey: b.hotkey,
                        target: target.clone(),
                        t0: now,
                    };
                    (Action::Start(target), non_modifier)
                }
                None => (Action::None, false),
            },
            State::Armed { hotkey, t0, .. } => {
                if let Some(b) = self.completing(vk) {
                    if b.hotkey == hotkey {
                        return (Action::None, false);
                    }
                    // After the threshold, another binding is just an other key.
                    if self.within_threshold(t0, now) {
                        let target = b.target.clone();
                        self.state = State::Armed {
                            hotkey,
                            target: target.clone(),
                            t0,
                        };
                        return (Action::Retarget(target), non_modifier);
                    }
                }
                if vk == VK_ESCAPE {
                    self.state = State::Idle;
                    return (Action::Cancel, true);
                }
                if self.within_threshold(t0, now) {
                    self.state = State::Idle;
                    return (Action::Abandon, false);
                }
                (Action::None, false)
            }
            State::HandsFree { .. } => {
                if self.completing(vk).is_some() {
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

    fn key_up(&mut self, now: u32) -> Action {
        if let State::Armed { hotkey, target, t0 } = self.state.clone() {
            if !self.combo_down(&hotkey) {
                if self.within_threshold(t0, now) {
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

    fn within_threshold(&self, t0: u32, now: u32) -> bool {
        u64::from(elapsed_ms(t0, now)) < self.hold_threshold_ms
    }

    fn is_swallowed(&self, vk: u16) -> bool {
        self.swallow_ups.iter().any(|&(k, _)| k == vk)
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

    /// The first binding that exactly matches the held keys and is completed by
    /// the key-down `vk`. A binding with a key completes only on that key, so a
    /// stale swallowed key cannot complete a binding when a modifier goes down.
    fn completing(&self, vk: u16) -> Option<&Binding> {
        self.bindings
            .iter()
            .find(|b| self.matches_exactly(&b.hotkey) && b.hotkey.key.map_or(true, |key| key == vk))
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
            None if h.mods == MOD_CTRL | MOD_SHIFT || h.mods == MOD_ALT | MOD_SHIFT => {
                return Err(format!(
                    "'{}' is Windows' keyboard-layout switch",
                    h.display()
                ));
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

    fn press(m: &mut HotkeyMachine, vk: u16, t: u32) -> Decision {
        m.on_key(vk, true, t)
    }

    fn release(m: &mut HotkeyMachine, vk: u16, t: u32) -> Decision {
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
        assert!(validate_bindings(&[bind("Shift+F9", Target::Main)]).is_err());
        assert!(validate_bindings(&[bind("Alt+Shift", Target::Main)]).is_err());
        assert!(validate_bindings(&[bind("Ctrl+Shift", Target::Main)]).is_err());
        assert!(validate_bindings(&[bind("Ctrl+Alt", Target::Main)]).is_ok());
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
        assert_eq!(m.sync_held(&|_| false, 0), Action::None);
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

    #[test]
    fn sync_cancels_stale_hold() {
        let mut m = machine(vec![bind("Ctrl+Win", Target::Main)]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(m.sync_held(&|_| false, 0), Action::Cancel);
        press(&mut m, LCTRL, 900);
        assert_eq!(
            press(&mut m, LWIN, 900),
            dec(Action::Start(Target::Main), false, true)
        );
    }

    #[test]
    fn sync_keeps_session_when_combo_still_down() {
        let mut m = machine(vec![bind("Ctrl+Win", Target::Main)]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(m.sync_held(&|_| true, 0), Action::None);
        assert_eq!(release(&mut m, LWIN, 900), dec(Action::Stop, false, false));
    }

    #[test]
    fn esc_while_armed_cancels_and_is_swallowed() {
        let mut m = machine(vec![bind("Ctrl+Win", Target::Main)]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(press(&mut m, ESC, 50), dec(Action::Cancel, true, true));
        assert_eq!(release(&mut m, ESC, 60), dec(Action::None, true, false));
    }

    #[test]
    fn other_binding_after_threshold_is_ignored() {
        let mut m = machine(vec![
            bind("Ctrl+Win", Target::Main),
            bind("Ctrl+Win+P", Target::Mode("m".into())),
        ]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(press(&mut m, P, 400), none());
    }

    #[test]
    fn right_and_generic_modifiers_complete() {
        let mut m = machine(vec![bind("Ctrl+Win", Target::Main)]);
        press(&mut m, 0xA3, 0);
        assert_eq!(
            press(&mut m, 0x5C, 0),
            dec(Action::Start(Target::Main), false, true)
        );
        release(&mut m, 0x5C, 500);
        release(&mut m, 0xA3, 510);
        press(&mut m, 0x11, 600);
        assert_eq!(
            press(&mut m, 0x5B, 600),
            dec(Action::Start(Target::Main), false, true)
        );
    }

    #[test]
    fn sync_keeps_swallowed_keys() {
        // Swallowed keys read as up to GetAsyncKeyState, so sync must not drop them.
        const SPACE: u16 = 0x20;
        let mut m = machine(vec![bind("Ctrl+Win+Space", Target::Main)]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(
            press(&mut m, SPACE, 10),
            dec(Action::Start(Target::Main), true, true)
        );
        let modifiers_only = |vk: u16| vk == LCTRL || vk == LWIN;
        assert_eq!(m.sync_held(&modifiers_only, 10), Action::None);
        assert_eq!(press(&mut m, SPACE, 400), dec(Action::None, true, false));
        assert_eq!(release(&mut m, SPACE, 900), dec(Action::Stop, true, true));
    }

    #[test]
    fn stale_swallowed_key_does_not_complete() {
        const SPACE: u16 = 0x20;
        let mut m = machine(vec![bind("Ctrl+Win+Space", Target::Main)]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(
            press(&mut m, SPACE, 10),
            dec(Action::Start(Target::Main), true, true)
        );
        // Win+L lost the ups: the OS now reports everything up.
        assert_eq!(m.sync_held(&|_| false, 10), Action::Cancel);
        // Ctrl+Win with the stale Space must not start a dictation.
        assert_eq!(press(&mut m, LCTRL, 900), none());
        assert_eq!(press(&mut m, LWIN, 900), none());
        assert_eq!(press(&mut m, SPACE, 910), dec(Action::None, true, false));
        assert_eq!(release(&mut m, SPACE, 920), dec(Action::None, true, false));
        assert_eq!(
            press(&mut m, SPACE, 930),
            dec(Action::Start(Target::Main), true, true)
        );
    }

    #[test]
    fn hands_free_stop_then_held_key_repeats_do_not_restart() {
        const SPACE: u16 = 0x20;
        let mut m = machine(vec![bind("Ctrl+Win+Space", Target::Main)]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(
            press(&mut m, SPACE, 10),
            dec(Action::Start(Target::Main), true, true)
        );
        assert_eq!(
            release(&mut m, SPACE, 100),
            dec(Action::HandsFree(Target::Main), true, true)
        );
        // Ctrl+Win are still held: the next Space press stops the session.
        assert_eq!(press(&mut m, SPACE, 2000), dec(Action::Stop, true, true));
        // Auto-repeat of that Space must not start a new session.
        assert_eq!(press(&mut m, SPACE, 2030), dec(Action::None, true, false));
        assert_eq!(press(&mut m, SPACE, 2060), dec(Action::None, true, false));
        assert_eq!(release(&mut m, SPACE, 2100), dec(Action::None, true, false));
    }

    #[test]
    fn releasing_retarget_key_keeps_session() {
        let mut m = machine(vec![
            bind("Ctrl+Win", Target::Main),
            bind("Ctrl+Win+P", Target::Mode("m".into())),
        ]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        press(&mut m, P, 50);
        assert_eq!(release(&mut m, P, 900), dec(Action::None, true, false));
        assert_eq!(release(&mut m, LWIN, 950), dec(Action::Stop, false, false));
    }

    #[test]
    fn swallowed_repeat_within_gap_stays_swallowed() {
        const SPACE: u16 = 0x20;
        let mut m = machine(vec![bind("Ctrl+Win+Space", Target::Main)]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(
            press(&mut m, SPACE, 10),
            dec(Action::Start(Target::Main), true, true)
        );
        assert_eq!(press(&mut m, SPACE, 40), dec(Action::None, true, false));
    }

    #[test]
    fn lost_up_then_fresh_press_is_not_swallowed() {
        const A: u16 = 0x41;
        let mut m = machine(vec![bind("Ctrl+A", Target::Main)]);
        press(&mut m, LCTRL, 0);
        assert_eq!(
            press(&mut m, A, 10),
            dec(Action::Start(Target::Main), true, false)
        );
        // Ctrl goes up (Stop); A's key-up is lost.
        assert_eq!(release(&mut m, LCTRL, 500), dec(Action::Stop, false, false));
        // A press 5 s later is a new press, not auto-repeat.
        assert_eq!(press(&mut m, A, 5000), none());
        assert_eq!(release(&mut m, A, 5010), none());
        assert!(m.swallow_ups.is_empty());
        press(&mut m, LCTRL, 5100);
        assert_eq!(
            press(&mut m, A, 5110),
            dec(Action::Start(Target::Main), true, false)
        );
    }

    #[test]
    fn fresh_press_rule() {
        // Repeat intervals are far below the 1300 ms cut-off.
        assert!(!is_fresh_press(0, 30));
        assert!(!is_fresh_press(0, 1300));
        assert!(is_fresh_press(0, 1301));
        // A stamp that steps backwards is not a ~49-day gap.
        assert!(!is_fresh_press(1000, 999));
        assert!(is_fresh_press(0, 5000));
    }

    #[test]
    fn fresh_press_rule_across_tick_wrap() {
        assert!(!is_fresh_press(u32::MAX - 100, 400));
        assert!(is_fresh_press(u32::MAX - 100, 1400));
    }

    #[test]
    fn repeat_and_hold_threshold_across_tick_wrap() {
        const SPACE: u16 = 0x20;
        let mut m = machine(vec![bind("Ctrl+Win+Space", Target::Main)]);
        press(&mut m, LCTRL, u32::MAX - 60);
        press(&mut m, LWIN, u32::MAX - 60);
        assert_eq!(
            press(&mut m, SPACE, u32::MAX - 20),
            dec(Action::Start(Target::Main), true, true)
        );
        assert_eq!(press(&mut m, SPACE, 30), dec(Action::None, true, false));
        // 61 ms after the start, across the wrap: still within the hold threshold.
        assert_eq!(
            release(&mut m, SPACE, 40),
            dec(Action::HandsFree(Target::Main), true, true)
        );
    }

    #[test]
    fn lost_up_after_wrap_is_a_fresh_press() {
        const SPACE: u16 = 0x20;
        let mut m = machine(vec![bind("Ctrl+Win+Space", Target::Main)]);
        press(&mut m, SPACE, u32::MAX - 100);
        assert_eq!(press(&mut m, SPACE, 1400), none());
        assert!(m.swallow_ups.is_empty());
    }

    #[test]
    fn release_stamped_before_arming_is_still_hands_free() {
        // Out-of-order stamp: 1 ms before the arming down. Must not read as a ~49-day hold.
        let mut m = machine(vec![bind("Ctrl+Win", Target::Main)]);
        press(&mut m, LCTRL, 0);
        assert_eq!(
            press(&mut m, LWIN, 1000),
            dec(Action::Start(Target::Main), false, true)
        );
        assert_eq!(
            release(&mut m, LWIN, 999),
            dec(Action::HandsFree(Target::Main), false, false)
        );
    }

    #[test]
    fn repeat_stamped_before_last_down_stays_swallowed() {
        const SPACE: u16 = 0x20;
        let mut m = machine(vec![bind("Ctrl+Win+Space", Target::Main)]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(
            press(&mut m, SPACE, 500),
            dec(Action::Start(Target::Main), true, true)
        );
        assert_eq!(press(&mut m, SPACE, 499), dec(Action::None, true, false));
    }

    #[test]
    fn slow_repeat_while_armed_stays_swallowed() {
        // A session that is running treats every down of a swallowed key as repeat.
        const SPACE: u16 = 0x20;
        let mut m = machine(vec![bind("Ctrl+Win+Space", Target::Main)]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(
            press(&mut m, SPACE, 10),
            dec(Action::Start(Target::Main), true, true)
        );
        assert_eq!(press(&mut m, SPACE, 2010), dec(Action::None, true, false));
        assert_eq!(m.swallow_ups, vec![(SPACE, 2010)]);
        assert_eq!(release(&mut m, SPACE, 2100), dec(Action::Stop, true, true));
    }

    #[test]
    fn lost_ups_after_retarget_do_not_block_next_hotkey() {
        // Win+L loses every up after Ctrl+Win+P retargeted: P must not stay held.
        let mut m = machine(vec![
            bind("Ctrl+Win", Target::Main),
            bind("Ctrl+Win+P", Target::Mode("m".into())),
        ]);
        press(&mut m, LCTRL, 0);
        press(&mut m, LWIN, 0);
        assert_eq!(
            press(&mut m, P, 10),
            dec(Action::Retarget(Target::Mode("m".into())), true, true)
        );
        assert_eq!(m.sync_held(&|_| false, 10_010), Action::Cancel);
        assert_eq!(press(&mut m, LCTRL, 10_100), none());
        assert_eq!(
            press(&mut m, LWIN, 10_100),
            dec(Action::Start(Target::Main), false, true)
        );
    }
}
