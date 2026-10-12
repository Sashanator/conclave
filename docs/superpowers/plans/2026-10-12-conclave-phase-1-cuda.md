# Conclave Phase 1 (Whisper on CUDA) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship v0.2.0, a release that runs Whisper on the user's NVIDIA GPU via CUDA 12. The CUDA DLLs come from the user's installed toolkit and are not bundled. If the toolkit is missing, the app still starts, transcribes on the CPU, and says why. A one-line backend note in Settings shows which device Whisper uses.

**Architecture:**
- The release build adds `--features cuda`, compiled for sm_89 only.
- `build.rs` makes the linker **delay-load** the three CUDA runtime DLLs, so Windows no longer refuses to start the exe when they're missing.
- At runtime, `whisper_engine/cuda_runtime.rs` probes the DLLs with `LoadLibraryW`, and the acceleration decision sets `use_gpu = false` when any is missing.
- A Tauri command reports the decision to the settings UI and to a startup toast.
- The release workflow checks the delay-load table with `dumpbin` and starts the exe without CUDA on `PATH` before it publishes anything. Releases are created as drafts and published only after that smoke test passes.

**Tech Stack:** Rust (Tauri 2 app crate `meetily`, whisper-rs 0.13.2 → whisper-rs-sys 0.11.1 with `GGML_CUDA`), MSVC linker `/DELAYLOAD` + `delayimp.lib`, Next.js/React (sonner toasts), GitHub Actions (`Jimver/cuda-toolkit`, `tauri-apps/tauri-action@v0`).

**Spec:** `docs/superpowers/specs/2026-10-10-conclave-v1-design.md` §4 (as amended on 2026-10-12 in `docs/cuda-setup` / PR #2: no bundled DLLs) and §6. User guide: `CUDA_SETUP.md`.

## Execution setup (unchanged from Phase 0; see CLAUDE.md)

| Role | Agent | Model / effort |
|---|---|---|
| Orchestration | main session | Opus 5.5, high |
| Implementation | `conclave-implementer` | Haiku 5.5, high |
| Review | `reviewer` | Opus 5.5, medium |
| Codex pass (Tasks 1, 3, and the phase review) | inside the reviewer brief | `codex review -c model="gpt-6.1-sol" -c model_reasoning_effort="medium" -c sandbox_mode='"danger-full-access"' -` |
| 3rd reviewer, whole branch only (Task 4 Step 3) | Antigravity CLI, run by the controller | `agy -p "<prompt>" --model gemini-3.8-flash-high --mode plan --dangerously-skip-permissions --print-timeout 1800s` (Gemini 3.8 Flash High; read-only plan mode) |

Parallel streams:
- **Stream A:** Task 1, then Task 2, in worktree `.worktrees/p1-app` on branch `p1-app`.
- **Stream B:** Task 3, in worktree `.worktrees/p1-release` on branch `p1-release`.

Both branch from `phase-1-cuda`, which is created from `docs/cuda-setup`. The files don't overlap. The controller merges both into `phase-1-cuda` before Task 4.

## Global Constraints

- **CUDA 12.x only, pinned to 12.8.0 in CI**, built on `windows-2022`. nvcc 12.8 rejects the VS 2026 compiler on `windows-latest`.
- **Bundle only `cudart64_12.dll`** (about 0.5 MB), as a normal import next to the exe. `cublas64_12.dll` and `cublasLt64_12.dll` are never bundled. `cublas64_12.dll` and `nvcuda.dll` (the driver) are **delay-loaded**. The probe list is `cublas64_12.dll`, `cublasLt64_12.dll` and `nvcuda.dll`. This is a controller ruling of 2026-10-12, made after the Task 3 review.
- `CMAKE_CUDA_ARCHITECTURES=89` (RTX 40 series) for the release build. whisper-rs-sys forwards every `CMAKE_*` environment variable to CMake (`whisper-rs-sys-0.11.1/build.rs:236-243`).
- Only `release.yml` builds CUDA. `ci.yml` stays CPU-only and unchanged. No Vulkan build.
- **A missing CUDA runtime** means: the app starts, Whisper runs on the CPU, the Settings note reads `Whisper running on: CPU — CUDA 12 runtime not found (see CUDA_SETUP.md)`, and a toast is shown once at startup.
- The backend note format is exactly `Whisper running on: CUDA (<GPU name>)`, `Whisper running on: CPU`, or `Whisper running on: <Backend>`.
- Parakeet is unchanged (CPU).
- **CI minutes:** verify locally first. There are only two push points (integration, and the release tag), and no trial release run. The v0.2.0 tag build is the validation, and its release stays a **draft** until the smoke test passes.
- Phase 0 invariants still hold: no telemetry or phone-home (the guard script), the Conclave identity, NSIS-only unsigned installer, version from the tag.
- Every commit ends with the trailer of the model that made it.

## Review Focus

1. **CUDA toolkit not installed, or only 13.x installed.** The app must still start: no Windows "DLL not found" dialog and no delay-load exception. Pinned by the release smoke step (Task 3) and the `missing_from` tests (Task 1).
2. **Driver too old, or no GPU, with the toolkit present.** whisper.cpp must fall back to the CPU rather than crash. Not testable in CI (the runner has no GPU). It's on the user's PC checklist (Task 4) and the plan treats it as whisper.cpp's existing behaviour.
3. **A delay-loaded DLL that exports data, not just functions.** The link would fail or misbehave. Pinned by the `dumpbin` delay-import check plus the smoke run (Task 3).
4. **A broken build reaching Releases.** Pinned by draft-then-publish (Task 3): the release becomes public only after the smoke test passes.
5. **`nvidia-smi` missing, slow, or printing nothing.** The Settings note must still render. Pinned by the `backend_label` tests with `gpu_name = None` (Task 2), and by the command being `async` so it never blocks the UI thread.

---

## File Structure

| Path | Change | Responsibility |
|---|---|---|
| `frontend/src-tauri/build.rs` | modify | `/DELAYLOAD` the three CUDA DLLs and link `delayimp` (Windows + `cuda` feature only) |
| `frontend/src-tauri/src/whisper_engine/cuda_runtime.rs` | create | Probe the CUDA runtime DLLs (pure `missing_from` and a cached Windows probe) |
| `frontend/src-tauri/src/whisper_engine/mod.rs` | modify | `pub mod cuda_runtime;` |
| `frontend/src-tauri/src/whisper_engine/acceleration.rs` | modify | `use_gpu` respects CUDA runtime availability; `backend_label` |
| `frontend/src-tauri/src/whisper_engine/whisper_engine.rs:320-324` | modify | Pass runtime availability; warn in the log |
| `frontend/src-tauri/src/whisper_engine/commands.rs` | modify | `get_whisper_backend_status` command and GPU name |
| `frontend/src-tauri/src/lib.rs` | modify | Register the command |
| `frontend/src/components/TranscriptSettings.tsx` | modify | Backend note line |
| `frontend/src/app/page.tsx` | modify | One startup toast when the CUDA runtime is missing |
| `.github/workflows/release.yml` | modify | CUDA toolkit, `--features cuda`, sm_89, dumpbin check, smoke test, draft then publish |
| `README.md`, `AGENTS.md` | modify | Point to `CUDA_SETUP.md`; releases are CUDA builds |

---

### Task 1: CUDA runtime probe, CPU fallback, delay-load linking

**Files:**
- Create: `frontend/src-tauri/src/whisper_engine/cuda_runtime.rs`
- Modify: `frontend/src-tauri/src/whisper_engine/mod.rs`, `frontend/src-tauri/src/whisper_engine/acceleration.rs`, `frontend/src-tauri/src/whisper_engine/whisper_engine.rs:318-324`, `frontend/src-tauri/build.rs`

**Interfaces:**
- Produces:
  - `crate::whisper_engine::cuda_runtime::{CUDA_RUNTIME_DLLS: [&str; 3], missing_from(loadable: impl Fn(&str) -> bool) -> Vec<&'static str>, missing_cuda_runtime_dlls() -> &'static [&'static str]}`
  - `acceleration::whisper_context_acceleration_for(compiled_backend, runtime_detected_gpu, performance_tier, cuda_runtime_available: bool) -> WhisperContextAcceleration`

- [ ] **Step 1: Write the failing tests**

Create `frontend/src-tauri/src/whisper_engine/cuda_runtime.rs` with only its tests first:

```rust
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
    fn toolkit_absent_reports_all_three() {
        assert_eq!(missing_from(|_| false), CUDA_RUNTIME_DLLS.to_vec());
    }

    #[test]
    fn non_cuda_builds_never_report_missing_dlls() {
        if !cfg!(all(windows, feature = "cuda")) {
            assert!(missing_cuda_runtime_dlls().is_empty());
        }
    }
}
```

Add `pub mod cuda_runtime;` to `frontend/src-tauri/src/whisper_engine/mod.rs`, next to the other `pub mod` lines.

In `acceleration.rs` tests, add:

```rust
    #[test]
    fn acceleration_cuda_backend_without_runtime_falls_back_to_cpu() {
        let params = whisper_context_acceleration_for(
            WhisperCompiledBackend::Cuda,
            GpuType::Cuda,
            PerformanceTier::Ultra,
            false,
        );
        assert!(!params.use_gpu);
        assert!(!params.flash_attn);
    }
```

Update the four existing test calls to pass `true` as the new last argument.

- [ ] **Step 2: Run them to confirm they fail**

Run: `cd /workspace && cargo test -p meetily --lib whisper_engine:: 2>&1 | tail -20` (with `CARGO_TARGET_DIR=/workspace/target`).
Expected: compile errors (`cannot find function missing_from`, wrong number of arguments).

- [ ] **Step 3: Implement**

Put this above the tests in `cuda_runtime.rs`:

```rust
//! CUDA builds delay-load these DLLs (see build.rs) instead of bundling them; probing them here
//! lets a PC without the CUDA 12 toolkit run Whisper on the CPU instead of failing to start.

use std::sync::OnceLock;

pub const CUDA_RUNTIME_DLLS: [&str; 3] = ["cudart64_12.dll", "cublas64_12.dll", "cublasLt64_12.dll"];

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
```

In `acceleration.rs`, change the function signature and the `use_gpu` line:

```rust
pub fn whisper_context_acceleration_for(
    compiled_backend: WhisperCompiledBackend,
    runtime_detected_gpu: GpuType,
    performance_tier: PerformanceTier,
    cuda_runtime_available: bool,
) -> WhisperContextAcceleration {
    let use_gpu = match compiled_backend {
        WhisperCompiledBackend::Cpu => false,
        WhisperCompiledBackend::Cuda => cuda_runtime_available,
        _ => true,
    };
```

Leave the rest of the function unchanged. `flash_attn` is already `use_gpu && flash_attn`.

In `whisper_engine.rs`, around lines 318-324, replace the call with:

```rust
                let missing_cuda = crate::whisper_engine::cuda_runtime::missing_cuda_runtime_dlls();
                if !missing_cuda.is_empty() {
                    log::warn!(
                        "CUDA 12 runtime not found ({}); Whisper will run on CPU. See CUDA_SETUP.md",
                        missing_cuda.join(", ")
                    );
                }
                let acceleration = whisper_context_acceleration_for(
                    WhisperCompiledBackend::current(),
                    hardware_profile.gpu_type,
                    hardware_profile.performance_tier,
                    missing_cuda.is_empty(),
                );
```

In `frontend/src-tauri/build.rs` `main()`, insert this just before `tauri_build::build()`:

```rust
    // CUDA runtime DLLs come from the user's CUDA 12 toolkit (not bundled): delay-load them so a PC
    // without the toolkit can still start the app; whisper_engine/cuda_runtime.rs then picks CPU.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_FEATURE_CUDA").is_ok()
    {
        for dll in ["cudart64_12.dll", "cublas64_12.dll", "cublasLt64_12.dll"] {
            println!("cargo:rustc-link-arg=/DELAYLOAD:{dll}");
        }
        println!("cargo:rustc-link-lib=delayimp");
    }
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cd /workspace && cargo test -p meetily --lib whisper_engine:: 2>&1 | tail -15`
Expected: the 4 `cuda_runtime` tests and the 5 acceleration tests pass.
Then run the full `cargo test --workspace --exclude llama-helper`. Expect 240 + 5 = 245 tests passing; the 3 ignored ones stay ignored.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/build.rs frontend/src-tauri/src/whisper_engine/
git commit -m "feat(cuda): delay-load CUDA runtime, fall back to CPU when it is missing"
```

- [ ] **Step 6: Review.** Use the `reviewer` with a Codex pass. Focus: the soundness of the FFI and `unsafe` code, the delay-load flags, and that `use_gpu` is false on every path where a DLL is missing.

---

### Task 2: Backend note and startup toast

**Files:**
- Modify: `frontend/src-tauri/src/whisper_engine/acceleration.rs` (add `backend_label` and its tests), `frontend/src-tauri/src/whisper_engine/commands.rs`, `frontend/src-tauri/src/lib.rs` (`generate_handler!`), `frontend/src/components/TranscriptSettings.tsx`, `frontend/src/app/page.tsx`

**Interfaces:**
- Consumes: `cuda_runtime::missing_cuda_runtime_dlls()` and `WhisperCompiledBackend::current()` (from Task 1).
- Produces: the Tauri command `get_whisper_backend_status` → `{ label: string, cuda_runtime_missing: boolean }` (serde field names as written).

- [ ] **Step 1: Write the failing tests** in the `acceleration.rs` tests module:

```rust
    #[test]
    fn label_cuda_with_gpu_name() {
        assert_eq!(
            backend_label(WhisperCompiledBackend::Cuda, &[], Some("NVIDIA GeForce RTX 4070 Ti SUPER")),
            "Whisper running on: CUDA (NVIDIA GeForce RTX 4070 Ti SUPER)"
        );
    }

    #[test]
    fn label_cuda_without_gpu_name() {
        assert_eq!(backend_label(WhisperCompiledBackend::Cuda, &[], None), "Whisper running on: CUDA");
    }

    #[test]
    fn label_cuda_runtime_missing_points_to_setup_guide() {
        assert_eq!(
            backend_label(WhisperCompiledBackend::Cuda, &["cublas64_12.dll"], Some("RTX")),
            "Whisper running on: CPU — CUDA 12 runtime not found (see CUDA_SETUP.md)"
        );
    }

    #[test]
    fn label_cpu_and_other_backends() {
        assert_eq!(backend_label(WhisperCompiledBackend::Cpu, &[], None), "Whisper running on: CPU");
        assert_eq!(backend_label(WhisperCompiledBackend::Vulkan, &[], None), "Whisper running on: Vulkan");
    }
```

Run: `cargo test -p meetily --lib whisper_engine::acceleration` (with `CARGO_TARGET_DIR=/workspace/target`).
Expected: FAIL (`cannot find function backend_label`).

- [ ] **Step 2: Implement `backend_label`** in `acceleration.rs`, below `whisper_context_acceleration_for`:

```rust
/// One-line note for the settings UI (spec §4.2).
pub fn backend_label(
    compiled_backend: WhisperCompiledBackend,
    missing_cuda_dlls: &[&str],
    gpu_name: Option<&str>,
) -> String {
    match compiled_backend {
        WhisperCompiledBackend::Cuda if !missing_cuda_dlls.is_empty() => {
            "Whisper running on: CPU — CUDA 12 runtime not found (see CUDA_SETUP.md)".to_string()
        }
        WhisperCompiledBackend::Cuda => match gpu_name {
            Some(name) => format!("Whisper running on: CUDA ({name})"),
            None => "Whisper running on: CUDA".to_string(),
        },
        WhisperCompiledBackend::Cpu => "Whisper running on: CPU".to_string(),
        other => format!("Whisper running on: {}", other.as_str()),
    }
}
```

Run the tests again. Expected: 4 new tests pass.

- [ ] **Step 3: Add the command** to `frontend/src-tauri/src/whisper_engine/commands.rs`:

```rust
#[derive(serde::Serialize)]
pub struct WhisperBackendStatus {
    pub label: String,
    pub cuda_runtime_missing: bool,
}

/// Async so the one-time nvidia-smi call never blocks the UI thread.
#[tauri::command]
pub async fn get_whisper_backend_status() -> WhisperBackendStatus {
    use super::acceleration::{backend_label, WhisperCompiledBackend};
    let compiled = WhisperCompiledBackend::current();
    let missing = super::cuda_runtime::missing_cuda_runtime_dlls();
    let gpu_name = if compiled == WhisperCompiledBackend::Cuda { nvidia_gpu_name() } else { None };
    WhisperBackendStatus {
        label: backend_label(compiled, missing, gpu_name.as_deref()),
        cuda_runtime_missing: compiled == WhisperCompiledBackend::Cuda && !missing.is_empty(),
    }
}

fn nvidia_gpu_name() -> Option<String> {
    static NAME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        let mut cmd = std::process::Command::new("nvidia-smi");
        cmd.args(["--query-gpu=name", "--format=csv,noheader"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flash
        }
        let out = cmd.output().ok().filter(|o| o.status.success())?;
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .map(|line| line.trim().to_string())
            .filter(|name| !name.is_empty())
    })
    .clone()
}
```

If `serde` isn't already imported in that file, use the fully qualified `serde::Serialize` as written; serde is already a dependency. Register `whisper_engine::commands::get_whisper_backend_status` in `generate_handler!` in `lib.rs`, next to the other `whisper_engine::commands::*` entries. Find them with `grep -n "whisper_engine::commands::" frontend/src-tauri/src/lib.rs`.

- [ ] **Step 4: Settings note.** In `frontend/src/components/TranscriptSettings.tsx`:
  1. Add `const [backendLabel, setBackendLabel] = useState<string>('');`.
  2. Add a mount effect:

     ```tsx
     useEffect(() => {
         invoke<{ label: string; cuda_runtime_missing: boolean }>('get_whisper_backend_status')
             .then((s) => setBackendLabel(s.label))
             .catch(() => setBackendLabel(''));
     }, []);
     ```

  3. Inside the `{uiProvider === 'localWhisper' && (...)}` block, right above `<ModelManager`, render:

     ```tsx
     {backendLabel && (
         <p className="text-xs text-gray-500 mb-2 mx-1">{backendLabel}</p>
     )}
     ```

  Reuse the file's existing `useState`, `useEffect` and `invoke` imports, adding any that are missing.

- [ ] **Step 5: Startup toast.** In `frontend/src/app/page.tsx`, the client component that already imports `toast` from `sonner` and uses `useEffect`, add a mount effect:

```tsx
useEffect(() => {
    invoke<{ label: string; cuda_runtime_missing: boolean }>('get_whisper_backend_status')
        .then((s) => {
            if (s.cuda_runtime_missing) {
                toast.warning('CUDA 12 runtime not found — Whisper runs on CPU. See CUDA_SETUP.md', { duration: 10000 });
            }
        })
        .catch(() => {});
}, []);
```

Import `invoke` from `@tauri-apps/api/core` if the file doesn't already.

- [ ] **Step 6: Verify**

```bash
cd /workspace/frontend && pnpm exec tsc --noEmit && bun test && pnpm run build
cd /workspace && cargo test --workspace --exclude llama-helper 2>&1 | tail -5
bash scripts/check-no-phone-home.sh
```

Expected: everything passes, with 249 app tests (245 + 4).

- [ ] **Step 7: Commit:** `feat(cuda): Whisper backend note in settings and missing-runtime toast`.

- [ ] **Step 8: Review.** Use the `reviewer` (no Codex). Focus: the note renders when `nvidia-smi` is missing, the command is non-blocking, and the toast shows only for CUDA builds.

---

### Task 3: Release workflow builds CUDA, verifies delay-load, smoke-tests, publishes after the test

**Files:**
- Modify: `.github/workflows/release.yml`, `README.md`, `AGENTS.md`

**Interfaces:**
- Consumes: the delay-load flags from Task 1's `build.rs`, which merge in at integration. This task changes no Rust code.

- [ ] **Step 1: Check the CUDA action version** with `gh api repos/Jimver/cuda-toolkit/releases/latest -q .tag_name` (it was `v0.2.36` on 2026-10-12). Use the exact tag, not a moving one.

- [ ] **Step 2: Edit `release.yml`.** Make these changes and nothing else:

  - After `dtolnay/rust-toolchain@stable`, add:

    ```yaml
          - name: Install CUDA 12.8 toolkit (build only; the installer does not bundle CUDA DLLs)
            uses: Jimver/cuda-toolkit@v0.2.36
            with:
              cuda: '12.8.0'
              method: network
              sub-packages: '["nvcc", "cudart", "cublas", "cublas_dev", "visual_studio_integration"]'
    ```

  - In the rust-cache step, change `key: release-cpu-v1` to `key: release-cuda-v1`.
  - On the `Build installer` step:
    - add `env:` `CMAKE_CUDA_ARCHITECTURES: '89'` next to `GITHUB_TOKEN`;
    - change `args:` to `--target x86_64-pc-windows-msvc --bundles nsis --features cuda ${{ steps.portability.outputs.args }}`;
    - set `releaseDraft: true`;
    - rename the step to "Build installer (draft release on tag)".
  - After the build step, add a delay-load check:

    ```yaml
          - name: Verify CUDA DLLs are delay-loaded, not hard imports
            shell: pwsh
            run: |
              $exe = "target/x86_64-pc-windows-msvc/release/meetily.exe"
              $dumpbin = & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -latest -find "VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe" | Select-Object -First 1
              $imports = & $dumpbin /imports $exe | Out-String
              $split = $imports.IndexOf("Section contains the following delay load imports")
              if ($split -lt 0) { Write-Error "no delay-load import section in $exe"; exit 1 }
              $normal = $imports.Substring(0, $split); $delayed = $imports.Substring($split)
              foreach ($dll in "cudart64_12.dll", "cublas64_12.dll", "cublasLt64_12.dll") {
                if ($normal -match [regex]::Escape($dll)) { Write-Error "$dll is a normal import (app would fail to start without CUDA)"; exit 1 }
                if ($delayed -notmatch [regex]::Escape($dll)) { Write-Error "$dll missing from delay-load imports"; exit 1 }
              }
              Write-Host "OK: CUDA runtime DLLs are delay-loaded"
    ```

  - Then add a startup smoke test without CUDA on `PATH`:

    ```yaml
          - name: Smoke test - app starts without CUDA on PATH
            shell: pwsh
            run: |
              $env:PATH = ($env:PATH -split ';' | Where-Object { $_ -notmatch 'CUDA' }) -join ';'
              $env:CUDA_PATH = $null
              $p = Start-Process -FilePath "target/x86_64-pc-windows-msvc/release/meetily.exe" -PassThru
              Start-Sleep -Seconds 20
              if ($p.HasExited) {
                $code = $p.ExitCode
                # 0xC0000135 STATUS_DLL_NOT_FOUND, 0xC06D007E delay-load module not found
                if ($code -eq -1073741515 -or $code -eq -1066598274) { Write-Error "exe failed to start without CUDA (exit $code)"; exit 1 }
                Write-Warning "exe exited early with code $code (not a CUDA DLL failure; runner has no audio/GPU)"
              } else {
                Stop-Process -Id $p.Id -Force
                Write-Host "OK: app stayed up 20s without CUDA on PATH"
              }
    ```

  - Then add a publish step:

    ```yaml
          - name: Publish release (only after the checks above pass)
            if: github.ref_type == 'tag'
            shell: bash
            env:
              GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
            run: gh release edit "$GITHUB_REF_NAME" --draft=false
    ```

  - Change the `releaseBody` to: `'Unsigned build: on first install choose "More info → Run anyway". GPU transcription needs the CUDA 12 runtime — see CUDA_SETUP.md in the repo (without it Whisper runs on CPU). Update by running the new setup over the old one.'`

- [ ] **Step 3: Lint.** Run `cd /workspace && actionlint .github/workflows/release.yml`. Expected: no output.

- [ ] **Step 4: Docs.**
  - `README.md`: in the install section, add one line: "GPU (NVIDIA) transcription needs the CUDA 12 runtime: see [CUDA_SETUP.md](CUDA_SETUP.md). Without it Conclave transcribes on the CPU."
  - `AGENTS.md` "Releases" bullet: say the installer is a CUDA build (`--features cuda`, sm_89) whose CUDA DLLs are delay-loaded from the user's toolkit (`CUDA_SETUP.md`), not bundled. Also say that releases are drafts until the smoke test passes.

- [ ] **Step 5: Commit** as `ci(release): CUDA 12.8 build, delay-load check, no-CUDA smoke test, publish after checks`. Don't push.

- [ ] **Step 6: Review.** Use the `reviewer` with a Codex pass. Focus:
  - It's the first CUDA run, so look for first-run risks: the sub-package names, the VS integration with the cmake crate's Visual Studio generator, and that `CMAKE_CUDA_ARCHITECTURES` reaches whisper-rs-sys.
  - The `dumpbin` parsing.
  - That the smoke test really strips CUDA from `PATH`.
  - The draft and publish flow.

---

### Task 4: Integrate, phase review, release v0.2.0, hand-off

- [ ] **Step 1: Integration.** In `/workspace` on `phase-1-cuda`, merge `p1-app` and `p1-release` with `--no-ff`, then run the full local suite:
  - `pnpm exec tsc --noEmit`, `bun test` and `pnpm run build`, in `frontend/`
  - `cargo test --workspace --exclude llama-helper`
  - `cargo check -p meeting-detector --target x86_64-pc-windows-msvc --tests --bins`
  - the guard script, `actionlint` and the `set-version` node tests

  All must pass.
- [ ] **Step 2: Push and open a PR.** Push `phase-1-cuda` (push point 1) and open a PR to `main`. PR #2 (`docs/cuda-setup`) is included in this branch's history. Wait for PR checks (Linux `checks`, Windows `detector`).
- [ ] **Step 3: Phase review (three reviewers).** Run these in parallel:
  - a `reviewer` review with a Codex pass of `main..phase-1-cuda`;
  - an **Antigravity CLI** review, which the controller runs in the background from `/workspace`:
    ```bash
    agy -p "Review the changes in git diff main...phase-1-cuda of this repo against docs/superpowers/specs/2026-10-10-conclave-v1-design.md section 4 and docs/superpowers/plans/2026-10-12-conclave-phase-1-cuda.md. Read-only: do not modify files. Report only confirmed bugs or risks, each with file:line, failure scenario and fix." \
      --model gemini-3.8-flash-high --mode plan --dangerously-skip-permissions --print-timeout 1800s \
      > /tmp/claude-1000/-workspace/856fdc4d-db31-4468-b4a5-55dd868575b4/scratchpad/agy-phase1-review.txt 2>&1
    ```
    Check that `git status` is unchanged afterwards. Treat Antigravity's findings like Codex's: the `reviewer` (or a re-review) verifies each one before anything is fixed, labelled with its source.

  Fix the confirmed findings, then get one recheck. After two cycles, bring unresolved findings to the user.
- [ ] **Step 4: Ask the user** "Merge and tag v0.2.0?" and wait for an explicit yes.
- [ ] **Step 5: Merge, then tag `v0.2.0`** (push point 2). Watch the Release run.
  - **If the build, `dumpbin` or smoke step fails:** the draft stays unpublished. Fix on a branch, merge, delete the draft, move the tag (`git tag -fa v0.2.0`, then `git push -f origin v0.2.0`) and tell the user.
  - **If it succeeds:** `gh release view v0.2.0` shows `Conclave_0.2.0_x64-setup.exe`, published.
- [ ] **Step 6: Hand off this checklist to the user.**
  1. Install the CUDA 12.8 runtime following `CUDA_SETUP.md`.
  2. Install v0.2.0 over v0.1.0.
  3. Settings shows `Whisper running on: CUDA (NVIDIA GeForce RTX 4070 Ti SUPER)`.
  4. With `large-v3-turbo`, the live transcript keeps up during a call, and Task Manager's "Cuda" graph is active.
  5. **Optional:** temporarily remove `%CUDA_PATH%\bin` from `PATH`, restart Conclave, and check you see the toast plus `Whisper running on: CPU — CUDA 12 runtime not found…` with transcription still working. Restore `PATH` afterwards.
