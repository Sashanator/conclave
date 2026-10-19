//! Clipboard access for the dictation paste. Every write carries the three exclusion formats,
//! so clipboard history, cloud sync and monitors skip the dictated text (spec §10).
//! Text and dictation contents are never logged.
use std::thread;
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Ole::{CF_DIB, CF_DIBV5, CF_UNICODETEXT};

/// What `save` read from the clipboard. Other formats (HTML, files) are not kept.
pub struct SavedClipboard {
    text: Option<Vec<u16>>,
    dib: Option<Vec<u8>>,
    dibv5: Option<Vec<u8>>,
    png: Option<Vec<u8>>,
}

/// Closes the clipboard on drop, so every early return still releases it.
struct OpenGuard;

impl Drop for OpenGuard {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

/// Other apps hold the clipboard for a moment, so retry 10 × 20 ms.
fn open() -> Result<OpenGuard, String> {
    for _ in 0..10 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            return Ok(OpenGuard);
        }
        thread::sleep(Duration::from_millis(20));
    }
    Err("clipboard busy".into())
}

// shortcut: text and images (CF_DIB, CF_DIBV5, PNG) survive a paste; files and rich text don't, save those too if users copy them before dictating
pub fn save() -> Result<SavedClipboard, String> {
    let _open = open()?;
    Ok(SavedClipboard {
        text: read_text()?,
        dib: best_effort("CF_DIB", read_bytes(u32::from(CF_DIB.0))).flatten(),
        dibv5: best_effort("CF_DIBV5", read_bytes(u32::from(CF_DIBV5.0))).flatten(),
        png: best_effort("PNG", register(w!("PNG")).and_then(read_bytes)).flatten(),
    })
}

/// Image formats are best effort: a failure logs the format name and drops that image only.
fn best_effort<T>(format: &str, result: Result<T, String>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(e) => {
            log::warn!("dictate: clipboard format {format} skipped: {e}");
            None
        }
    }
}

/// If the write fails after the clipboard was emptied, `fallback` is put back in the same open session.
pub fn set_private_text(text: &str, fallback: Option<&SavedClipboard>) -> Result<(), String> {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let _open = open()?;
    unsafe { EmptyClipboard() }.map_err(|e| format!("EmptyClipboard: {e}"))?;
    // Exclusions first: if a later step fails, the dictated text is not left on the clipboard.
    let written = write_exclusions()
        .and_then(|()| set_bytes(u32::from(CF_UNICODETEXT.0), &wide_bytes(&wide)));
    if let (Err(e), Some(saved)) = (&written, fallback) {
        log::warn!("dictate: clipboard set failed, putting the previous clipboard back: {e}");
        if let Err(e) = write_saved(saved) {
            log::warn!("dictate: previous clipboard not put back: {e}");
        }
    }
    written
}

/// Puts `saved` back unless the clipboard changed since `expected_seq`. Returns false when it was left alone.
pub fn restore(saved: &SavedClipboard, expected_seq: u32) -> Result<bool, String> {
    let _open = open()?;
    // Checked under the lock: an app that copies after an unlocked check would lose its copy to our write.
    if sequence_number() != expected_seq {
        return Ok(false);
    }
    write_saved(saved)?;
    Ok(true)
}

/// Empties the open clipboard and writes `saved` back. An empty `saved` leaves it empty.
fn write_saved(saved: &SavedClipboard) -> Result<(), String> {
    unsafe { EmptyClipboard() }.map_err(|e| format!("EmptyClipboard: {e}"))?;
    write_exclusions()?;
    // Text first: a failed image write must not lose it.
    if let Some(text) = &saved.text {
        set_bytes(u32::from(CF_UNICODETEXT.0), &wide_bytes(text))?;
    }
    if let Some(png) = &saved.png {
        best_effort("PNG", register(w!("PNG")).and_then(|id| set_bytes(id, png)));
    }
    if let Some(dibv5) = &saved.dibv5 {
        best_effort("CF_DIBV5", set_bytes(u32::from(CF_DIBV5.0), dibv5));
    }
    if let Some(dib) = &saved.dib {
        best_effort("CF_DIB", set_bytes(u32::from(CF_DIB.0), dib));
    }
    Ok(())
}

pub fn sequence_number() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

/// The three exclusion formats of spec §10; the clipboard must be open.
fn write_exclusions() -> Result<(), String> {
    set_bytes(
        register(w!("ExcludeClipboardContentFromMonitorProcessing"))?,
        &[0],
    )?;
    set_bytes(
        register(w!("CanIncludeInClipboardHistory"))?,
        &0u32.to_le_bytes(),
    )?;
    set_bytes(
        register(w!("CanUploadToCloudClipboard"))?,
        &0u32.to_le_bytes(),
    )
}

fn register(name: PCWSTR) -> Result<u32, String> {
    match unsafe { RegisterClipboardFormatW(name) } {
        0 => Err("RegisterClipboardFormatW failed".into()),
        id => Ok(id),
    }
}

/// UTF-16 with a terminating NUL, as CF_UNICODETEXT expects.
fn wide_bytes(text: &[u16]) -> Vec<u8> {
    text.iter()
        .copied()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// Puts `bytes` on the open clipboard. The system owns the memory after a successful set.
fn set_bytes(format: u32, bytes: &[u8]) -> Result<(), String> {
    unsafe {
        let mem =
            GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|e| format!("GlobalAlloc: {e}"))?;
        let ptr = GlobalLock(mem);
        if ptr.is_null() {
            let _ = GlobalFree(Some(mem));
            return Err("GlobalLock failed".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
        let _ = GlobalUnlock(mem);
        match SetClipboardData(format, Some(HANDLE(mem.0))) {
            Ok(_) => Ok(()),
            Err(e) => {
                let _ = GlobalFree(Some(mem));
                Err(format!("SetClipboardData({format}): {e}"))
            }
        }
    }
}

/// Copies a clipboard item out. None when the format is absent. The clipboard must be open.
fn read_bytes(format: u32) -> Result<Option<Vec<u8>>, String> {
    let handle = match unsafe { GetClipboardData(format) } {
        Ok(handle) if !handle.0.is_null() => handle,
        _ => return Ok(None),
    };
    let mem = HGLOBAL(handle.0);
    unsafe {
        let size = GlobalSize(mem);
        let ptr = GlobalLock(mem) as *const u8;
        if ptr.is_null() {
            return Err("GlobalLock failed".into());
        }
        let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
        let _ = GlobalUnlock(mem);
        Ok(Some(bytes))
    }
}

fn read_text() -> Result<Option<Vec<u16>>, String> {
    let Some(bytes) = read_bytes(u32::from(CF_UNICODETEXT.0))? else {
        return Ok(None);
    };
    let mut units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    units.truncate(end);
    Ok(Some(units))
}
