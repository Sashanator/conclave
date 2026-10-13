//! Thin Win32 layer: consent-store reads, change notifications, window titles, capture sessions.
use crate::consent::{app_name_from_key, is_in_use, path_from_key, MicUser};
use std::cell::Cell;
use std::io;
use windows::core::{Interface, BOOL, PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Media::Audio::{
    eCapture, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
    MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};
use windows::Win32::System::Registry::{RegNotifyChangeKeyValue, HKEY, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME};
use windows::Win32::System::Threading::{
    CreateEventW, OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

fn entry(key: &RegKey, name: &str, packaged: bool) -> Option<MicUser> {
    let sub = key.open_subkey(name).ok()?;
    let start: u64 = sub.get_value("LastUsedTimeStart").unwrap_or(0);
    let stop: u64 = sub.get_value("LastUsedTimeStop").unwrap_or(0);
    Some(MicUser {
        app: app_name_from_key(name, packaged),
        path: path_from_key(name, packaged),
        packaged,
        in_use: is_in_use(start, stop),
    })
}

/// Reads every app recorded under `HKCU\<path>` (packaged subkeys + `NonPackaged\*`).
pub fn read_mic_users(path: &str) -> io::Result<Vec<MicUser>> {
    let root = RegKey::predef(HKEY_CURRENT_USER).open_subkey(path)?;
    let mut users = Vec::new();
    for name in root.enum_keys().filter_map(Result::ok) {
        if name.eq_ignore_ascii_case("NonPackaged") {
            // An unreadable NonPackaged key must not hide the packaged entries.
            if let Ok(np) = root.open_subkey(&name) {
                users.extend(np.enum_keys().filter_map(Result::ok).filter_map(|n| entry(&np, &n, false)));
            }
        } else if let Some(u) = entry(&root, &name, true) {
            users.push(u);
        }
    }
    Ok(users)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    Changed,
    Timeout,
}

/// Wakes when anything under the key changes (values or subkeys), or after a timeout.
pub struct ChangeWatcher {
    key: RegKey,
    event: HANDLE,
    /// False only after a failed re-arm; `wait` then re-arms before waiting.
    armed: Cell<bool>,
}

// The registry key and event handle are only used from the thread that calls `wait`.
unsafe impl Send for ChangeWatcher {}

impl ChangeWatcher {
    pub fn open(path: &str) -> io::Result<Self> {
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(path)?;
        let event = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }.map_err(io::Error::other)?;
        let watcher = Self { key, event, armed: Cell::new(false) };
        watcher.arm()?;
        Ok(watcher)
    }

    /// Registrations are one-shot, and arming while one is pending adds another wait operation: arm only when needed.
    fn arm(&self) -> io::Result<()> {
        self.armed.set(false);
        let hkey = HKEY(self.key.raw_handle() as _);
        let err = unsafe {
            RegNotifyChangeKeyValue(hkey, true, REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_CHANGE_NAME, Some(self.event), true)
        };
        if err.is_err() {
            return Err(io::Error::from_raw_os_error(err.0 as i32));
        }
        self.armed.set(true);
        Ok(())
    }

    /// Waits for a change or the timeout. On a change it re-arms before returning, so a change made while the
    /// caller reads the store is kept signalled for the next call (the event auto-resets).
    pub fn wait(&self, timeout_ms: u32) -> io::Result<Wake> {
        if !self.armed.get() {
            self.arm()?;
        }
        match unsafe { WaitForSingleObject(self.event, timeout_ms) } {
            WAIT_OBJECT_0 => {
                self.arm()?;
                Ok(Wake::Changed)
            }
            WAIT_TIMEOUT => Ok(Wake::Timeout),
            other => Err(io::Error::other(format!("WaitForSingleObject returned {:?}", other))),
        }
    }
}

impl Drop for ChangeWatcher {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.event) };
    }
}

/// Lowercase exe file name of a process, or None if it can't be opened (e.g. elevated/system process).
pub fn process_exe_name(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Some(path.rsplit('\\').next().unwrap_or(&path).to_lowercase())
    }
}

/// Titles of visible top-level windows whose process exe name equals `exe` (lowercase).
pub fn window_titles_for(exe: &str) -> Vec<String> {
    struct Ctx<'a> { exe: &'a str, titles: Vec<String> }
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam.0 as *mut Ctx);
        if IsWindowVisible(hwnd).as_bool() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if process_exe_name(pid).as_deref() == Some(ctx.exe) {
                let mut buf = [0u16; 512];
                let n = GetWindowTextW(hwnd, &mut buf);
                if n > 0 {
                    ctx.titles.push(String::from_utf16_lossy(&buf[..n as usize]));
                }
            }
        }
        true.into()
    }
    let mut ctx = Ctx { exe, titles: Vec::new() };
    let _ = unsafe { EnumWindows(Some(cb), LPARAM(&mut ctx as *mut Ctx as isize)) };
    ctx.titles
}

/// Must be called once on each thread that uses `capture_sessions`.
pub fn init_com() {
    // RPC_E_CHANGED_MODE (already initialised differently) is fine for our use.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSession {
    pub device_id: String,
    pub device_name: String,
    pub process_id: u32,
    pub process_name: Option<String>,
    pub active: bool,
}

unsafe fn take_pwstr(p: PWSTR) -> String {
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as _));
    s
}

/// Audio sessions on every active capture device. A multi-process session is reported under its creator's pid;
/// sessions whose pid cannot be queried are skipped.
pub fn capture_sessions() -> windows::core::Result<Vec<CaptureSession>> {
    let mut out = Vec::new();
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)?;
        for i in 0..devices.GetCount()? {
            let Ok(device) = devices.Item(i) else { continue };
            let device_id = device.GetId().map(|p| take_pwstr(p)).unwrap_or_default();
            let device_name = device
                .OpenPropertyStore(STGM_READ)
                .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName))
                .and_then(|pv| PropVariantToStringAlloc(&pv))
                .map(|p| take_pwstr(p))
                .unwrap_or_else(|_| "(unknown device)".to_string());
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
            let Ok(sessions) = manager.GetSessionEnumerator() else { continue };
            for j in 0..sessions.GetCount().unwrap_or(0) {
                let Ok(control) = sessions.GetSession(j) else { continue };
                let Ok(control2) = control.cast::<IAudioSessionControl2>() else { continue };
                // A multi-process session returns AUDCLNT_S_NO_SINGLE_PROCESS, a success code, with its creator pid.
                // Errors are skipped so one bad session never fails the scan.
                let Ok(pid) = control2.GetProcessId() else { continue };
                if pid == 0 {
                    continue;
                }
                let active = control2.GetState().map(|s| s == AudioSessionStateActive).unwrap_or(false);
                out.push(CaptureSession {
                    device_id: device_id.clone(),
                    device_name: device_name.clone(),
                    process_id: pid,
                    process_name: process_exe_name(pid),
                    active,
                });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const TEST_ROOT: &str = r"Software\ConclaveTest";

    /// Deletes a test's registry tree on drop, so a failed assertion leaves nothing behind.
    struct TestKey(String);

    impl Drop for TestKey {
        fn drop(&mut self) {
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.0);
        }
    }

    /// Returns the guard first: bindings drop in reverse order, so the key handle closes before the guard deletes.
    fn test_store(name: &str) -> (TestKey, RegKey, String) {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let root = format!(r"{TEST_ROOT}\{name}");
        let _ = hkcu.delete_subkey_all(&root);
        let path = format!(r"{root}\microphone");
        let (key, _) = hkcu.create_subkey(&path).unwrap();
        (TestKey(root), key, path)
    }

    fn set(key: &RegKey, sub: &str, start: u64, stop: u64) {
        let (k, _) = key.create_subkey(sub).unwrap();
        k.set_value("LastUsedTimeStart", &start).unwrap();
        k.set_value("LastUsedTimeStop", &stop).unwrap();
    }

    /// Writes only the stop stamp of the Zoom entry, through a handle opened separately from the watcher's.
    fn write_zoom_stop(store: &str, stop: u64) {
        let k = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(format!(r"{store}\NonPackaged\C:#Zoom.exe"), winreg::enums::KEY_WRITE)
            .unwrap();
        k.set_value("LastUsedTimeStop", &stop).unwrap();
    }

    #[test]
    fn reads_packaged_and_nonpackaged_entries() {
        let (_guard, key, path) = test_store("read");
        set(&key, "MSTeams_8wekyb3d8bbwe", 200, 0);
        set(&key, r"NonPackaged\C:#Program Files#Zoom#bin#Zoom.exe", 100, 200);
        set(&key, r"NonPackaged\C:#Program Files#Google#Chrome#Application#chrome.exe", 300, 100);

        let mut users = read_mic_users(&path).unwrap();
        users.sort_by(|a, b| a.app.cmp(&b.app));
        assert_eq!(users, vec![
            MicUser {
                app: "chrome.exe".into(),
                path: r"C:\Program Files\Google\Chrome\Application\chrome.exe".into(),
                packaged: false,
                in_use: true,
            },
            MicUser {
                app: "msteams_8wekyb3d8bbwe".into(),
                path: "MSTeams_8wekyb3d8bbwe".into(),
                packaged: true,
                in_use: true,
            },
            MicUser {
                app: "zoom.exe".into(),
                path: r"C:\Program Files\Zoom\bin\Zoom.exe".into(),
                packaged: false,
                in_use: false,
            },
        ]);
    }

    #[test]
    fn watcher_wakes_on_change_and_times_out_otherwise() {
        let (_guard, key, path) = test_store("watch");
        set(&key, r"NonPackaged\C:#Zoom.exe", 1, 0);
        let watcher = ChangeWatcher::open(&path).unwrap();

        assert_eq!(watcher.wait(300).unwrap(), Wake::Timeout);

        let store = path.clone();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            write_zoom_stop(&store, 5);
        });
        assert_eq!(watcher.wait(5000).unwrap(), Wake::Changed);
        writer.join().unwrap();
    }

    #[test]
    fn consecutive_changes_each_wake_the_watcher() {
        let (_guard, key, path) = test_store("twice");
        set(&key, r"NonPackaged\C:#Zoom.exe", 1, 0);
        let watcher = ChangeWatcher::open(&path).unwrap();

        // First change lands before the first wait is called.
        write_zoom_stop(&path, 5);
        assert_eq!(watcher.wait(5000).unwrap(), Wake::Changed);

        // Second change lands after the first wake, before the second wait: it must not be lost.
        write_zoom_stop(&path, 6);
        let started = Instant::now();
        assert_eq!(watcher.wait(5000).unwrap(), Wake::Changed);
        assert!(started.elapsed() < Duration::from_secs(2), "second change was not reported promptly");
    }

    #[test]
    fn capture_sessions_never_errors_on_runner() {
        init_com();
        // CI runners usually have no microphone: an empty list is the expected success.
        let sessions = capture_sessions().expect("enumeration must succeed even with zero devices");
        for s in sessions {
            assert_ne!(s.process_id, 0);
        }
    }

    #[test]
    fn window_titles_for_unknown_exe_is_empty() {
        assert!(window_titles_for("definitely-not-running-conclave-test.exe").is_empty());
    }
}
