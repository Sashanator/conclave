//! Clipboard access for the dictation paste. Every write carries the three exclusion formats,
//! so clipboard history, cloud sync and monitors skip the dictated text (spec §10).
//! Text and dictation contents are never logged.
use std::thread;
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Ole::{CF_DIB, CF_DIBV5, CF_UNICODETEXT};

use super::{is_skipped_format, within_clipboard_cap};

/// What `save` read from the clipboard: each copyable format's id and bytes, in clipboard order.
pub struct SavedClipboard {
    formats: Vec<(u32, Vec<u8>)>,
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

// shortcut: over 64 MB only text and images are kept (the old behaviour), so a huge copy is never read into memory
pub fn save() -> Result<SavedClipboard, String> {
    let _open = open()?;
    let mut formats = enumerate_formats();
    let total: usize = formats.iter().map(|&f| format_size(f)).sum();
    if !within_clipboard_cap(total) {
        log::warn!("dictate: clipboard over the size cap, keeping only text and images");
        let png = register(w!("PNG")).ok();
        let text_or_image = [CF_UNICODETEXT, CF_DIB, CF_DIBV5].map(|f| u32::from(f.0));
        formats.retain(|f| text_or_image.contains(f) || Some(*f) == png);
    }
    let formats = formats
        .into_iter()
        .filter_map(|f| {
            best_effort(&f.to_string(), read_bytes(f))
                .flatten()
                .map(|bytes| (f, bytes))
        })
        .collect();
    Ok(SavedClipboard { formats })
}

/// Ids of the formats on the open clipboard that can be copied, in clipboard order.
fn enumerate_formats() -> Vec<u32> {
    let mut formats = Vec::new();
    let mut format = 0;
    loop {
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            return formats;
        }
        if !is_skipped_format(format) {
            formats.push(format);
        }
    }
}

/// Bytes a clipboard item holds, 0 when absent. The clipboard must be open.
fn format_size(format: u32) -> usize {
    match unsafe { GetClipboardData(format) } {
        Ok(handle) if !handle.0.is_null() => unsafe { GlobalSize(HGLOBAL(handle.0)) },
        _ => 0,
    }
}

/// Format reads and writes are best effort: a failure logs the format id and drops that format only.
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
    for (format, bytes) in &saved.formats {
        best_effort(&format.to_string(), set_bytes(*format, bytes));
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

