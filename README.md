# Conclave

Conclave is a personal Windows 10 app that replaces Notion AI Meeting Notes. It records online calls (Zoom, Google Meet, Microsoft Teams), transcribes them locally with Whisper, and summarizes them with a cloud LLM using your own API key.

Design spec: [docs/superpowers/specs/2026-10-10-conclave-v1-design.md](docs/superpowers/specs/2026-10-10-conclave-v1-design.md)

## Install

Download `Conclave_<version>_x64-setup.exe` from the Releases page of this repository and run it. The installer is unsigned, so Windows SmartScreen shows "Unknown publisher" once per install. Click **More info → Run anyway**.

## Update

Download the newer `…-setup.exe` from the Releases page and run it. It upgrades the existing install in place, so do not uninstall first. Meetings, settings and downloaded Whisper models live in `%APPDATA%\com.conclave.app`, which updates do not touch.

## Uninstall

Use Windows Settings → Apps & features (or Control Panel → Programs and Features) → Conclave → Uninstall. The uninstaller has a "Delete the application data" checkbox. It is unchecked by default, which keeps meetings and models. Checking it removes them. Do not delete the install folder by hand, because that leaves the Start-menu entry and the uninstall registration behind.

## Credits

Based on Meetily (MIT) by Zackriya Solutions: [github.com/Zackriya-Solutions/meeting-minutes](https://github.com/Zackriya-Solutions/meeting-minutes).
