# Conclave

Conclave is a personal Windows 10 app that replaces Notion AI Meeting Notes. It records online calls (Zoom, Google Meet, Microsoft Teams), transcribes them locally with Whisper, and summarizes them with a cloud LLM using your own API key.

Design spec: [docs/superpowers/specs/2026-10-10-conclave-v1-design.md](docs/superpowers/specs/2026-10-10-conclave-v1-design.md)

## Install

Download `Conclave_<version>_x64-setup.exe` from the Releases page of this repository and run it. The installer is unsigned, so Windows SmartScreen shows "Unknown publisher" once per install. Click **More info → Run anyway**.

GPU (NVIDIA) transcription needs cuBLAS from a CUDA 12 toolkit on your PC: see [CUDA_SETUP.md](CUDA_SETUP.md). The installer ships only the small `cudart64_12.dll`. Without the toolkit Conclave transcribes on the CPU.

## Update

Download the newer `…-setup.exe` from the Releases page and run it. It upgrades the existing install in place, so do not uninstall first. Updates do not touch your data, which lives in three places:

- `%APPDATA%\com.conclave.app`: the meetings database, settings and downloaded models.
- `%APPDATA%\Conclave`: custom summary templates, notification settings and a fallback model folder.
- `%USERPROFILE%\Music\conclave-recordings` (falls back to Documents if Music is unavailable): recordings.

## Uninstall

Use Windows Settings → Apps & features (or Control Panel → Programs and Features) → Conclave → Uninstall. The uninstaller has a "Delete the application data" checkbox. It is unchecked by default. Checking it removes the `%APPDATA%\com.conclave.app` folder (meetings, settings and models). It does not remove `%APPDATA%\Conclave` or `%USERPROFILE%\Music\conclave-recordings`; delete those by hand if you want them gone. Do not delete the install folder by hand, because that leaves the Start-menu entry and the uninstall registration behind.

## Credits

Based on Meetily (MIT) by Zackriya Solutions: [github.com/Zackriya-Solutions/meeting-minutes](https://github.com/Zackriya-Solutions/meeting-minutes).
