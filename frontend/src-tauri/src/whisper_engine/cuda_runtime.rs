//! cudart64_12.dll is bundled with the app. cublas and cublasLt come from the user's CUDA 12 toolkit,
//! and nvcuda.dll is the NVIDIA driver; build.rs delay-loads cublas64_12.dll, cublasLt64_12.dll and
//! nvcuda.dll. Probing these here lets a PC without them run Whisper on the CPU instead of failing to start.

use std::sync::OnceLock;

pub const CUDA_RUNTIME_DLLS: [&str; 3] = ["cublas64_12.dll", "cublasLt64_12.dll", "nvcuda.dll"];

/// The runtime DLLs for which `loadable` returns false, in `CUDA_RUNTIME_DLLS` order.
pub fn missing_from(loadable: impl Fn(&str) -> bool) -> Vec<&'static str> {
    CUDA_RUNTIME_DLLS.iter().copied().filter(|dll| !loadable(dll)).collect()
}

/// Missing CUDA runtime DLLs for this build; always empty unless built with `cuda` for Windows.
/// shortcut: probed once per process, so installing the toolkit needs an app restart; fine for a desktop app.
pub fn missing_cuda_runtime_dlls() -> &'static [&'static str] {
    static MISSING: OnceLock<Vec<&'static str>> = OnceLock::new();
    MISSING.get_or_init(|| {
        if cfg!(all(windows, feature = "cuda")) {
            missing_from(dll_loads)
        } else {
            Vec::new()
        }
    })
}

#[cfg(windows)]
fn dll_loads(name: &str) -> bool {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn FreeLibrary(module: *mut c_void) -> i32;
    }
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // Same search order as the delay-load helper (exe dir, system dirs, PATH), so the probe matches what the loader will find.
    let module = unsafe { LoadLibraryW(wide.as_ptr()) };
    if module.is_null() {
        return false;
    }
    unsafe { FreeLibrary(module) };
    true
}

#[cfg(not(windows))]
fn dll_loads(_name: &str) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_present_means_nothing_missing() {
        assert!(missing_from(|_| true).is_empty());
    }

    #[test]
    fn reports_exactly_the_dlls_that_fail_to_load() {
        let missing = missing_from(|dll| dll != "cublasLt64_12.dll");
        assert_eq!(missing, vec!["cublasLt64_12.dll"]);
    }

    #[test]
    fn toolkit_or_driver_absent_reports_all() {
        assert_eq!(missing_from(|_| false), CUDA_RUNTIME_DLLS.to_vec());
    }

    #[test]
    fn non_cuda_builds_never_report_missing_dlls() {
        if !cfg!(all(windows, feature = "cuda")) {
            assert!(missing_cuda_runtime_dlls().is_empty());
        }
    }
}
