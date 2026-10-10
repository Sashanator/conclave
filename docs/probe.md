# Running conclave-probe (Phase 0 checkpoint, spec §5.4)

1. Download `conclave-probe` from the latest green **CI** run on GitHub → Actions → CI → run → Artifacts. Unzip it.
2. Open **Windows Terminal / PowerShell**, `cd` to the folder, run `.\conclave-probe.exe`. Leave it running. It is read-only and offline.
3. Do these, waiting ~10 s between steps, and keep the window's output:
   - **A (mute/leave):** join a Zoom test meeting (zoom.us/test). Mute for 30 s. Unmute. Leave. Repeat in Google Meet (meet.new) and Microsoft Teams (browser or app).
   - **B (notifications):** note whether START/STOP lines say `via=notify` (good) or `via=timeout`.
   - **C (which mic):** during a Zoom call, switch Zoom's microphone (Settings → Audio) to another mic, then back. Watch "ACTIVE CAPTURE SESSIONS" change devices.
   - Also join a Meet call with the Meet tab **not** active, to see what window titles are printed.
4. **D (notification click):** in the installed Conclave app, open Settings → Notifications → send a test notification and click it. Note whether Conclave's window comes to the front.
5. Ctrl+C to stop. Paste the whole output and the D result back to Claude.
