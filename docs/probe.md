# Running conclave-probe (Phase 0 checkpoint, spec §5.4)

1. Download `conclave-probe` from the latest green **CI** run on GitHub → Actions → CI → run → Artifacts. Unzip it.
2. Open **Windows Terminal / PowerShell**, `cd` to the folder, run `.\conclave-probe.exe`. Leave it running. It is read-only and offline.
3. Do these, waiting ~10 s between steps, and keep the window's output:
   - **A (mute/leave):** join a Zoom test meeting (zoom.us/test). Mute for 30 s. Unmute. Leave. Repeat in Google Meet (meet.new) and Microsoft Teams (browser or app).
   - **B (notifications):** note whether START/STOP lines say `via=notify` (good) or `via=timeout`.
   - **C (which mic):** during a Zoom call, switch Zoom's microphone (Settings → Audio) to another mic, then back. Watch "ACTIVE CAPTURE SESSIONS" change devices.
   - Also join a Meet call with the Meet tab **not** active, to see what window titles are printed.
4. **D (notification click):** in Conclave, turn on Settings → General → Notifications. Start a recording, then bring another window to the front. When the "recording started" Windows notification appears, click it. Note whether Conclave's window comes to the front.
5. Ctrl+C to stop. Paste the whole output and the D result back to Claude.

## Results

- 2026-10-13, Zoom desktop:
    - A: Zoom keeps the mic open while muted; only leaving releases it.
    - B: `via=notify` arrives instantly.
    - C: capture sessions follow mic switches.
    - D: clicking a Windows notification does nothing, so Ask mode uses Conclave's own banner (spec §5, "Ask mode").
    - OBS and Notion hold the mic permanently.
- Google Meet and Teams: not yet run.
