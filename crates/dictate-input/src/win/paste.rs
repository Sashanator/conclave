//! The paste sequence (spec §10): save the clipboard, put the dictation on it privately, wait
//! for the user to let go of the modifiers, send Ctrl+V, then restore the clipboard if nobody
//! changed it meanwhile.
use std::mem::size_of;
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, RECT};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowRect,
    GetWindowThreadProcessId,
};

use super::clipboard;
use crate::paste_logic::{
    decide_paste, is_desktop_class, should_restore, CopiedReason, PasteOutcome, PasteTarget,
};

const KEYS_RELEASE_TIMEOUT: Duration = Duration::from_secs(2);
const KEY_POLL: Duration = Duration::from_millis(10);
/// VK_LSHIFT, VK_RSHIFT, VK_LCONTROL, VK_RCONTROL, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN.
const MODIFIER_VKS: [i32; 8] = [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C];
const VK_CONTROL: u16 = 0x11;
const VK_V: u16 = 0x56;

/// Closes a handle on drop.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

pub fn foreground_target(own_pid: u32) -> PasteTarget {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null()
        || hwnd == unsafe { GetShellWindow() }
        || hwnd == unsafe { GetDesktopWindow() }
        || is_desktop_window(hwnd)
    {
        return PasteTarget::NoWindow;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    // pid 0: the window was destroyed between the calls. There is no process to paste into.
    if pid == 0 {
        return PasteTarget::NoWindow;
    }
    if pid == own_pid {
        return PasteTarget::OwnWindow;
    }
    // Our own token failing counts as not elevated; a failed target query counts as elevated.
    let me_elevated = token_elevated(unsafe { GetCurrentProcess() }).unwrap_or(false);
    let blocked = !me_elevated && process_elevated(pid).unwrap_or(true);
    if blocked {
        PasteTarget::Elevated
    } else {
        PasteTarget::App
    }
}

/// The desktop is drawn by a WorkerW or Progman window, not by the shell window.
fn is_desktop_window(hwnd: HWND) -> bool {
    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    usize::try_from(len).is_ok_and(|len| is_desktop_class(&String::from_utf16_lossy(&buf[..len])))
}

fn process_elevated(pid: u32) -> windows::core::Result<bool> {
    let process =
        OwnedHandle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }?);
    token_elevated(process.0)
}

fn token_elevated(process: HANDLE) -> windows::core::Result<bool> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }?;
    let token = OwnedHandle(token);
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some(&mut elevation as *mut TOKEN_ELEVATION as *mut _),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    }?;
    Ok(elevation.TokenIsElevated != 0)
}

/// Centre of the foreground window, where the pill appears.
pub fn foreground_monitor_point() -> Option<(i32, i32)> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    Some(((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2))
}

/// True once no modifier is held. Never releases a key for the user.
pub fn wait_keys_released(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let held = MODIFIER_VKS
            .iter()
            .any(|&vk| unsafe { GetAsyncKeyState(vk) } < 0);
        if !held {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(KEY_POLL);
    }
}

pub fn send_ctrl_v() -> Result<(), String> {
    let key = |vk: u16, flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [
        key(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        key(VK_V, KEYBD_EVENT_FLAGS(0)),
        key(VK_V, KEYEVENTF_KEYUP),
        key(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(format!("SendInput inserted {sent} of {}", inputs.len()))
    }
}

/// Never blocks the hook thread: call it from a worker thread. Sleeps `restore_delay` before
/// restoring a paste, so the target app has read the clipboard first. `proceed` is asked right before
/// Ctrl+V; false sends nothing and the text stays copied.
pub fn paste_text(
    text: &str,
    own_pid: u32,
    restore_delay: Duration,
    proceed: &dyn Fn() -> bool,
) -> PasteOutcome {
    let saved = match clipboard::save() {
        Ok(saved) => Some(saved),
        Err(e) => {
            log::warn!(
                "dictate: clipboard save failed, the previous clipboard is not restored: {e}"
            );
            None
        }
    };
    let clipboard_set = match clipboard::set_private_text(text, saved.as_ref()) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("dictate: clipboard set failed: {e}");
            false
        }
    };
    let seq_after_set = clipboard::sequence_number();
    // The target is read after the wait, so an Alt+Tab during the wait is seen.
    let keys_released = clipboard_set && wait_keys_released(KEYS_RELEASE_TIMEOUT);
    let target = foreground_target(own_pid);
    let may_send = clipboard_set && keys_released && target == PasteTarget::App;
    let clipboard_changed = may_send && clipboard::sequence_number() != seq_after_set;
    // The key wait can run for seconds: Dictate may have been turned off meanwhile.
    let turned_off = may_send && !clipboard_changed && !proceed();
    let ctrl_v_sent = may_send
        && !clipboard_changed
        && !turned_off
        && match send_ctrl_v() {
            Ok(()) => true,
            Err(e) => {
                log::warn!("dictate: Ctrl+V not sent: {e}");
                false
            }
        };
    let outcome = if turned_off {
        PasteOutcome::Copied(CopiedReason::NoTarget)
    } else {
        decide_paste(
            target,
            clipboard_set,
            keys_released,
            clipboard_changed,
            ctrl_v_sent,
        )
    };
    log::info!("dictate: paste {outcome:?}, {} chars", text.chars().count());
    if outcome == PasteOutcome::Pasted {
        thread::sleep(restore_delay);
    }
    if let Some(saved) = saved {
        if should_restore(outcome, seq_after_set, clipboard::sequence_number()) {
            match clipboard::restore(&saved, seq_after_set) {
                Ok(true) => {}
                Ok(false) => log::info!("dictate: clipboard changed since the paste, not restored"),
                Err(e) => log::warn!("dictate: clipboard restore failed: {e}"),
            }
        }
    }
    outcome
}
