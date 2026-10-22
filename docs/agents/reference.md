# Conclave agent reference

Moved verbatim from `AGENTS.md` (inherited Meetily codebase guide). `AGENTS.md` says which section to read before changing what.

## Codebase guide (inherited from Meetily)

### Project Overview

**Conclave** is a privacy-first AI meeting assistant that captures, transcribes, and summarizes meetings entirely on local infrastructure. The supported application is the Tauri desktop app with a Rust core.

1. **Frontend**: Tauri-based desktop application (Rust + Next.js + TypeScript)
2. **Rust Backend**: Tauri commands, audio capture, transcription, storage, and summarization orchestration
3. **Legacy Backend Archive**: the old Python/FastAPI, Docker, and standalone whisper-server backend under `backend/` is archived and unsupported

#### Key Technology Stack
- **Desktop App**: Tauri 2.x (Rust) + Next.js 14 + React 18
- **Audio Processing**: Rust (cpal, whisper-rs, professional audio mixing)
- **Transcription**: Whisper.cpp / whisper-rs and Parakeet paths in the Tauri app
- **App API Surface**: Tauri commands and events, not a separate FastAPI service
- **LLM Integration**: Ollama (local), Claude, Groq, OpenRouter

### Essential Development Commands

#### Frontend Development (Tauri Desktop App)

**Location**: `/frontend`

```bash
# macOS Development
./clean_run.sh              # Clean build and run with info logging
./clean_run.sh debug        # Run with debug logging
./clean_build.sh            # Production build

# Windows Development
clean_run_windows.bat       # Clean build and run
clean_build_windows.bat     # Production build

# Manual Commands
pnpm install                # Install dependencies
pnpm run dev                # Next.js dev server (port 3118)
pnpm run tauri:dev          # Full Tauri development mode
pnpm run tauri:build        # Production build

# GPU-Specific Builds (for testing acceleration)
pnpm run tauri:dev:metal    # macOS Metal GPU
pnpm run tauri:dev:cuda     # NVIDIA CUDA
pnpm run tauri:dev:vulkan   # AMD/Intel Vulkan
pnpm run tauri:dev:cpu      # CPU-only (no GPU)
```

### High-Level Architecture

#### Tauri Desktop Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Frontend (Tauri Desktop App)                  │
│  ┌──────────────────┐  ┌─────────────────┐  ┌────────────────┐ │
│  │   Next.js UI     │  │  Rust Backend   │  │ Whisper Engine │ │
│  │  (React/TS)      │←→│  (Audio + IPC)  │←→│  (Local STT)   │ │
│  └──────────────────┘  └─────────────────┘  └────────────────┘ │
│         ↑ Tauri Events           ↑ Audio Pipeline               │
└─────────────────────────────────────────────────────────────────┘
```

The current app does not require a separate FastAPI tier. Meeting persistence, local transcription, and summary orchestration are handled through the Rust/Tauri core.

#### Audio Processing Pipeline (Critical Understanding)

The audio system has **two parallel paths** with different purposes:

```
Raw Audio (Mic + System)
         ↓
┌────────────────────────────────────────────────────────────┐
│              Audio Pipeline Manager                         │
│  (frontend/src-tauri/src/audio/pipeline.rs)                │
└─────────────┬──────────────────────────┬───────────────────┘
              ↓                          ↓
    ┌─────────────────┐        ┌─────────────────────┐
    │ Recording Path  │        │ Transcription Path  │
    │ (Pre-mixed)     │        │ (VAD-filtered)      │
    └─────────────────┘        └─────────────────────┘
              ↓                          ↓
    RecordingSaver.save()      WhisperEngine.transcribe()
```

**Key Insight**: The pipeline performs **professional audio mixing** (RMS-based ducking, clipping prevention) for recording, while simultaneously applying **Voice Activity Detection (VAD)** to send only speech segments to Whisper for transcription.

#### Audio Device Modularization (Recently Completed)

**Context**: The audio system was refactored from a monolithic 1028-line `core.rs` file into focused modules. See [AUDIO_MODULARIZATION_PLAN.md](AUDIO_MODULARIZATION_PLAN.md) for details.

```
audio/
├── devices/                    # Device discovery and configuration
│   ├── discovery.rs           # list_audio_devices, trigger_audio_permission
│   ├── microphone.rs          # default_input_device
│   ├── speakers.rs            # default_output_device
│   ├── configuration.rs       # AudioDevice types, parsing
│   └── platform/              # Platform-specific implementations
│       ├── windows.rs         # WASAPI logic (~200 lines)
│       ├── macos.rs           # ScreenCaptureKit logic
│       └── linux.rs           # ALSA/PulseAudio logic
├── capture/                   # Audio stream capture
│   ├── microphone.rs          # Microphone capture stream
│   ├── system.rs              # System audio capture stream
│   └── core_audio.rs          # macOS ScreenCaptureKit integration
├── pipeline.rs                # Audio mixing and VAD processing
├── recording_manager.rs       # High-level recording coordination
├── recording_commands.rs      # Tauri command interface
└── recording_saver.rs         # Audio file writing
```

**When working on audio features**:
- Device detection issues → `devices/discovery.rs` or `devices/platform/{windows,macos,linux}.rs`
- Microphone/speaker problems → `devices/microphone.rs` or `devices/speakers.rs`
- Audio capture issues → `capture/microphone.rs` or `capture/system.rs`
- Mixing/processing problems → `pipeline.rs`
- Recording workflow → `recording_manager.rs`

#### Rust ↔ Frontend Communication (Tauri Architecture)

**Command Pattern** (Frontend → Rust):
```typescript
// Frontend: src/app/page.tsx
await invoke('start_recording', {
  mic_device_name: "Built-in Microphone",
  system_device_name: "BlackHole 2ch",
  meeting_name: "Team Standup"
});
```

```rust
// Rust: src/lib.rs
#[tauri::command]
async fn start_recording<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>
) -> Result<(), String> {
    // Implementation delegates to audio::recording_commands
}
```

**Event Pattern** (Rust → Frontend):
```rust
// Rust: Emit transcript updates
app.emit("transcript-update", TranscriptUpdate {
    text: "Hello world".to_string(),
    timestamp: chrono::Utc::now(),
    // ...
})?;
```

```typescript
// Frontend: Listen for events
await listen<TranscriptUpdate>('transcript-update', (event) => {
  setTranscripts(prev => [...prev, event.payload]);
});
```

#### Whisper Model Management

**Model Storage Locations**:
- **Development**: `frontend/models/`
- **Production (macOS)**: `~/Library/Application Support/com.conclave.app/models/`
- **Production (Windows)**: `%APPDATA%\com.conclave.app\models\` (fallback: `%APPDATA%\Conclave\models\`; app_data_dir; `whisper_engine/commands.rs:16-19`)

**Model Loading** (frontend/src-tauri/src/whisper_engine/whisper_engine.rs):
```rust
pub async fn load_model(&self, model_name: &str) -> Result<()> {
    // Automatically detects GPU capabilities (Metal/CUDA/Vulkan)
    // Falls back to CPU if GPU unavailable
}
```

**GPU Acceleration**:
- **macOS**: Metal + CoreML (automatically enabled)
- **Windows/Linux**: CUDA (NVIDIA), Vulkan (AMD/Intel), or CPU
- Configure via Cargo features: `--features cuda`, `--features vulkan`


### Common Development Tasks

#### Adding a New Audio Device Platform

1. Create platform file: `audio/devices/platform/{platform_name}.rs`
2. Implement device enumeration for the platform
3. Add platform-specific configuration in `audio/devices/configuration.rs`
4. Update `audio/devices/platform/mod.rs` to export new platform functions
5. Test with `cargo check` and platform-specific device tests

#### Adding a New Tauri Command

1. Define command in `src/lib.rs`:
   ```rust
   #[tauri::command]
   async fn my_command(arg: String) -> Result<String, String> { /* ... */ }
   ```
2. Register in `tauri::Builder`:
   ```rust
   .invoke_handler(tauri::generate_handler![
       start_recording,
       my_command,  // Add here
   ])
   ```
3. Call from frontend:
   ```typescript
   const result = await invoke<string>('my_command', { arg: 'value' });
   ```

#### Modifying Audio Pipeline Behavior

**Location**: `frontend/src-tauri/src/audio/pipeline.rs`

Key components:
- `AudioMixerRingBuffer`: Manages mic + system audio synchronization
- `ProfessionalAudioMixer`: RMS-based ducking and mixing
- `AudioPipelineManager`: Orchestrates VAD, mixing, and distribution

**Testing Audio Changes**:
```bash
# Enable verbose audio logging
RUST_LOG=app_lib::audio=debug ./clean_run.sh

# Monitor audio metrics in real-time
# Check Developer Console in the app (Cmd+Shift+I on macOS)
```

#### Tauri Backend Development

Current app behavior should be implemented in the Rust/Tauri core, not in the archived Python backend. Add new frontend-facing behavior through Tauri commands/events and existing Rust services under `frontend/src-tauri/src`.

Do not add new endpoints to `backend/app/main.py`; that FastAPI code is legacy archive material only.

### Testing and Debugging

#### Frontend Debugging

**Enable Rust Logging**:
```bash
# macOS
RUST_LOG=debug ./clean_run.sh

# Windows (PowerShell)
$env:RUST_LOG="debug"; ./clean_run_windows.bat
```

**Developer Tools**:
- Open DevTools: `Cmd+Shift+I` (macOS) or `Ctrl+Shift+I` (Windows)
- Console Toggle: Built into app UI (console icon)
- View Rust logs: Check terminal output

#### Audio Pipeline Debugging

**Key Metrics** (emitted by pipeline):
- Buffer sizes (mic/system)
- Mixing window count
- VAD detection rate
- Dropped chunk warnings

**Monitor via Developer Console**: The app includes real-time metrics display when recording.

### Platform-Specific Notes

#### macOS
- **Audio Capture**: Uses ScreenCaptureKit for system audio (macOS 13+)
- **GPU**: Metal + CoreML automatically enabled
- **Permissions**: Requires microphone + screen recording permissions
- **System Audio**: Requires virtual audio device (BlackHole) for system capture

#### Windows
- **Audio Capture**: Uses WASAPI (Windows Audio Session API)
- **GPU**: CUDA (NVIDIA) or Vulkan (AMD/Intel) via Cargo features
- **Build Tools**: Requires Visual Studio Build Tools with C++ workload
- **System Audio**: Uses WASAPI loopback for system capture

#### Linux
- **Audio Capture**: ALSA/PulseAudio
- **GPU**: CUDA (NVIDIA) or Vulkan via Cargo features
- **Dependencies**: Requires cmake, llvm, libomp

### Performance Optimization Guidelines

#### Audio Processing
- Use `perf_debug!()` / `perf_trace!()` for hot-path logging (zero cost in release)
- Batch audio metrics using `AudioMetricsBatcher` (pipeline.rs)
- Pre-allocate buffers with `AudioBufferPool` (buffer_pool.rs)
- VAD filtering reduces Whisper load by ~70% (only processes speech)

#### Whisper Transcription
- **Model Selection**: Balance accuracy vs speed
  - Development: `base` or `small` (fast iteration)
  - Production: `medium` or `large-v3` (best quality)
- **GPU Acceleration**: 5-10x faster than CPU
- **Parallel Processing**: Available in `whisper_engine/parallel_processor.rs` for batch workloads

#### Frontend Performance
- React state updates batched via Sidebar context
- Transcript rendering virtualized for large meetings
- Audio level monitoring throttled to 60fps

### Key Files Reference

**Core Coordination**:
- [frontend/src-tauri/src/lib.rs](frontend/src-tauri/src/lib.rs) - Main Tauri entry point, command registration
- [frontend/src-tauri/src/audio/mod.rs](frontend/src-tauri/src/audio/mod.rs) - Audio module exports
- [frontend/src-tauri/src/database/mod.rs](frontend/src-tauri/src/database/mod.rs) - Local database module

**Audio System**:
- [frontend/src-tauri/src/audio/recording_manager.rs](frontend/src-tauri/src/audio/recording_manager.rs) - Recording orchestration
- [frontend/src-tauri/src/audio/pipeline.rs](frontend/src-tauri/src/audio/pipeline.rs) - Audio mixing and VAD
- [frontend/src-tauri/src/audio/recording_saver.rs](frontend/src-tauri/src/audio/recording_saver.rs) - Audio file writing

**UI Components**:
- [frontend/src/app/page.tsx](frontend/src/app/page.tsx) - Main recording interface
- [frontend/src/components/Sidebar/SidebarProvider.tsx](frontend/src/components/Sidebar/SidebarProvider.tsx) - Global state management

**Whisper Integration**:
- [frontend/src-tauri/src/whisper_engine/whisper_engine.rs](frontend/src-tauri/src/whisper_engine/whisper_engine.rs) - Whisper model management and transcription
