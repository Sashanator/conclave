//! The WH_KEYBOARD_LL hook thread. It creates and solely owns the `HotkeyMachine`.
use std::cell::RefCell;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{mpsc, Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HMODULE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, KillTimer, PeekMessageW, PostThreadMessageW, SetTimer, SetWindowsHookExW,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WM_APP,
    WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER,
};

use super::{can_rehook, is_altgr_fake_ctrl, is_injected};
use crate::hotkey::VK_MASK;
use crate::machine::{Action, Binding, HotkeyMachine};

const MSG_SET_BINDINGS: u32 = WM_APP + 1;
const MSG_RESET: u32 = WM_APP + 2;
/// Windows removes a low-level hook that misses its timeout without telling us, so re-install it periodically.
const REHOOK_INTERVAL_MS: u32 = 60_000;

struct HookState {
    machine: HotkeyMachine,
    on_action: Box<dyn Fn(Action) + Send>,
}

thread_local! {
    static STATE: RefCell<Option<HookState>> = const { RefCell::new(None) };
}

pub struct HookHandle {
    thread_id: u32,
    pending: Arc<Mutex<Option<Vec<Binding>>>>,
    join: Option<JoinHandle<()>>,
}

/// `on_action` runs on the hook thread, so it must only send to a channel:
/// Windows drops a low-level hook that takes more than about 300 ms.
pub fn start_hook(
    bindings: Vec<Binding>,
    hold_threshold_ms: u64,
    on_action: Box<dyn Fn(Action) + Send + 'static>,
) -> Result<HookHandle, String> {
    let pending = Arc::new(Mutex::new(None));
    let (ready_tx, ready_rx) = mpsc::channel();
    let slot = Arc::clone(&pending);
    let join = std::thread::Builder::new()
        .name("dictate-hook".into())
        .spawn(move || run(bindings, hold_threshold_ms, on_action, slot, ready_tx))
        .map_err(|e| format!("hook thread: {e}"))?;
    let ready = ready_rx
        .recv()
        .unwrap_or_else(|_| Err("hook thread exited".into()));
    match ready {
        Ok(thread_id) => Ok(HookHandle {
            thread_id,
            pending,
            join: Some(join),
        }),
        Err(e) => {
            let _ = join.join();
            Err(e)
        }
    }
}

impl HookHandle {
    /// False once the hook thread has exited, e.g. after a panic.
    pub fn is_alive(&self) -> bool {
        self.join.as_ref().is_some_and(|join| !join.is_finished())
    }

    /// Applied by the hook thread between events.
    pub fn set_bindings(&self, bindings: Vec<Binding>) {
        *self.pending.lock().unwrap_or_else(PoisonError::into_inner) = Some(bindings);
        post(self.thread_id, MSG_SET_BINDINGS);
    }

    pub fn reset(&self) {
        post(self.thread_id, MSG_RESET);
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let Some(join) = self.join.take() else {
            return;
        };
        for _ in 0..5 {
            // Joining only after the post succeeded (or the thread already ended) cannot hang.
            if post(self.thread_id, WM_QUIT) || join.is_finished() {
                let _ = join.join();
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        log::warn!("dictate: hook thread refused WM_QUIT, detaching it");
    }
}

impl Drop for HookHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// False when the message was not queued, e.g. because the hook thread has already exited.
fn post(thread_id: u32, msg: u32) -> bool {
    unsafe { PostThreadMessageW(thread_id, msg, WPARAM(0), LPARAM(0)) }.is_ok()
}

fn run(
    bindings: Vec<Binding>,
    hold_threshold_ms: u64,
    on_action: Box<dyn Fn(Action) + Send>,
    pending: Arc<Mutex<Option<Vec<Binding>>>>,
    ready: mpsc::Sender<Result<u32, String>>,
) {
    let thread_id = unsafe { GetCurrentThreadId() };
    // Creates this thread's message queue, so PostThreadMessageW can reach it.
    let mut msg = MSG::default();
    let _ = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE) };
    let module = match unsafe { GetModuleHandleW(PCWSTR::null()) } {
        Ok(m) => m,
        Err(e) => {
            let _ = ready.send(Err(format!("GetModuleHandleW failed: {e}")));
            return;
        }
    };
    // A slow hook thread is what makes Windows drop the hook, so run it at the highest priority.
    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL) };
    let mut hook = match install_hook(module) {
        Ok(hook) => Some(hook),
        Err(e) => {
            let _ = ready.send(Err(format!("keyboard hook failed: {e}")));
            return;
        }
    };
    // With no window, SetTimer ignores the id it is given and returns the one it made: keep that one.
    let rehook_timer = match unsafe { SetTimer(None, 0, REHOOK_INTERVAL_MS, None) } {
        0 => {
            log::warn!("dictate: re-hook timer not set, the keyboard hook is not re-checked");
            None
        }
        id => Some(id),
    };
    STATE.with(|s| {
        *s.borrow_mut() = Some(HookState {
            machine: HotkeyMachine::new(bindings, hold_threshold_ms),
            on_action,
        })
    });
    let _ = ready.send(Ok(thread_id));

    loop {
        // 0 means WM_QUIT, -1 an error; both end the loop.
        if unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 <= 0 {
            break;
        }
        match msg.message {
            MSG_SET_BINDINGS => {
                let next = pending
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
                if let Some(bindings) = next {
                    with_state(|st| st.machine.set_bindings(bindings));
                }
            }
            MSG_RESET => {
                with_state(|st| st.machine.reset());
            }
            WM_TIMER if rehook_timer == Some(msg.wParam.0) => {
                let idle = with_state(|st| st.machine.is_idle()).unwrap_or(false);
                // A missing hook is replaced at once: installing does not touch keys that are held.
                if hook.is_none() || can_rehook(idle, &key_is_down) {
                    // The new hook goes in before the old one comes out, so there is no gap. Both calls run
                    // before this thread pumps messages again, so no key reaches both hooks.
                    match install_hook(module) {
                        Ok(new) => {
                            if let Some(old) = hook.replace(new) {
                                let _ = unsafe { UnhookWindowsHookEx(old) };
                            }
                        }
                        Err(e) => log::warn!("dictate: keyboard hook re-install failed, will retry: {e}"),
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(id) = rehook_timer {
        let _ = unsafe { KillTimer(None, id) };
    }
    if let Some(hook) = hook {
        let _ = unsafe { UnhookWindowsHookEx(hook) };
    }
    STATE.with(|s| s.take());
}

fn install_hook(module: HMODULE) -> windows::core::Result<HHOOK> {
    unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), Some(module.into()), 0) }
}

fn with_state<R>(f: impl FnOnce(&mut HookState) -> R) -> Option<R> {
    STATE.with(|s| s.borrow_mut().as_mut().map(f))
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // A panic must not unwind out of an `extern "system"` fn: that aborts the app.
        let swallow = panic::catch_unwind(AssertUnwindSafe(|| {
            let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            handle_key(wparam.0 as u32, info)
        }));
        if matches!(swallow, Ok(true)) {
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Returns true when the key must be swallowed.
fn handle_key(msg: u32, info: &KBDLLHOOKSTRUCT) -> bool {
    let down = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
    let up = matches!(msg, WM_KEYUP | WM_SYSKEYUP);
    let vk = info.vkCode as u16;
    if !(down || up) || is_injected(info.flags.0) || is_altgr_fake_ctrl(vk, info.scanCode) {
        return false;
    }
    // KBDLLHOOKSTRUCT.time: the tick the machine's gap rules use (wraps).
    let now = info.time;
    let Some((sync, decision)) = with_state(|st| {
        let sync = if down {
            st.machine.sync_held(&key_is_down, now)
        } else {
            Action::None
        };
        (sync, st.machine.on_key(vk, down, now))
    }) else {
        return false;
    };
    if sync != Action::None {
        notify(sync);
    }
    // Win is still held here, so the mask stops Start from opening on Win-up.
    if decision.mask_start_menu {
        send_mask();
    }
    if decision.action != Action::None {
        notify(decision.action);
    }
    decision.swallow
}

fn notify(action: Action) {
    with_state(|st| (st.on_action)(action));
}

fn key_is_down(vk: u16) -> bool {
    unsafe { GetAsyncKeyState(vk as i32) < 0 }
}

fn send_mask() {
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(VK_MASK),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
}
