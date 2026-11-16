# Conclave

Conclave is a personal Windows 10 app that replaces Notion AI Meeting Notes. It records online calls (Zoom, Google Meet, Microsoft Teams), transcribes them locally with Whisper, and summarizes them with a cloud LLM using your own API key.

Design spec: [docs/superpowers/specs/2026-10-10-conclave-v1-design.md](docs/superpowers/specs/2026-10-10-conclave-v1-design.md)

## Install

Download `Conclave_<version>_x64-setup.exe` from the Releases page of this repository and run it. The installer is unsigned, so Windows SmartScreen shows "Unknown publisher" once per install. Click **More info → Run anyway**.

GPU (NVIDIA) transcription needs cuBLAS from a CUDA 12 toolkit on your PC: see [CUDA_SETUP.md](CUDA_SETUP.md). The installer ships only the small `cudart64_12.dll`. Without the toolkit Conclave transcribes on the CPU.

## Update

### In the app

1. Create a fine-grained GitHub token with read-only **Contents** access to this repository only.
2. Paste it in **Settings → General → Updates → GitHub token**, click **Save**, then click **Test**. Conclave stores the token in Windows Credential Manager.
3. Conclave checks for a new version about 30 seconds after it starts. The switch for this is on by default. You can also click **Check for updates** at any time.
4. When a new version is available, a dialog shows "Conclave vX.Y.Z is available" with the release notes.
   - **Update** downloads the new version, verifies its signature, installs it with a progress bar and restarts Conclave.
   - **Later** closes the dialog. Conclave asks again at the next start or the next check.
5. Conclave never asks or installs during a recording or its save, a Practice session or report, a summary being generated, or a model download.
6. If the check at start-up fails, Conclave shows nothing. The reason is in the status line in Settings.

### By hand

Download the newer `…-setup.exe` from the Releases page and run it. It upgrades the existing install in place, so do not uninstall first. Updates do not touch your data, which lives in three places:

- `%APPDATA%\com.conclave.app`: the meetings database, settings and downloaded models.
- `%APPDATA%\Conclave`: custom summary templates, notification settings and a fallback model folder.
- `%USERPROFILE%\Music\conclave-recordings` (falls back to Documents if Music is unavailable): recordings.
- `%LOCALAPPDATA%\com.conclave.app\logs\`: the log file (warnings, errors and crash messages; up to 5 MB, plus one older file). Attach it when reporting a problem.

## Back up before you update

**Automatic backup.** On the first start of a new version, Conclave backs up your data before it changes the database. The backup covers `meeting_minutes.sqlite` with its `-wal` and `-shm` files (if present), every `.json` file in `%APPDATA%\com.conclave.app`, and `%APPDATA%\Conclave\templates`. It does not cover models or recordings.

**Where backups are kept:**

- `%APPDATA%\com.conclave.app\backups\pre-upgrade\<date>_v<old>-to-v<new>\` for an automatic backup. If you are coming from a version before v1.2.0, the folder is `<date>_unknown-to-v<new>` instead.
- `%APPDATA%\com.conclave.app\backups\manual\<date>_v<version>\` for a manual backup.

Each backup folder has a `backup.json` file that describes it. Conclave keeps the last 3 automatic backups and the last 5 manual ones. Click **Open backups folder** in Settings to open the folder. A backup folder with no `backup.json` is an incomplete backup (for example, the PC lost power during the copy). Conclave never deletes it; delete it by hand.

**If the backup fails:** Conclave does not change anything and shows this dialog: "Couldn't back up your data before upgrading. Continue without a backup, or quit?" Quit leaves everything as it was. The next start tries the backup again.

**Recordings are not in the backup.** Copy `%USERPROFILE%\Music\conclave-recordings` (or the folder you set in Settings) by hand.

**Manual backup.** Click **Back up now** in Settings. To back up by hand instead, quit Conclave from the tray first (closing the window keeps Conclave running), then copy the first three locations listed under **By hand** in Update.

**Warning:** the uninstaller's "Delete the application data" checkbox also deletes `backups\`. Copy the backups folder somewhere else first.

## Restore from a backup

1. Quit Conclave from the tray.
2. Delete the live `meeting_minutes.sqlite-wal` and `meeting_minutes.sqlite-shm` files, if they are present. This step is required: a newer journal file next to an older database corrupts it.
3. Copy the backup's `meeting_minutes.sqlite` (plus its `-wal` and `-shm` files, if it has them) and its `.json` files into `%APPDATA%\com.conclave.app`. Copy the backup's `templates` folder over `%APPDATA%\Conclave\templates`. Leave out `backup.json`.
4. If the upgrade was the problem, run the older installer from the Releases page. The `v<old>` in the backup folder name tells you which version that is.

Your keys stay in Windows Credential Manager, so a restore with the current version needs nothing else.

**Downgrade:** If you reinstall a version older than v1.2.0, paste your keys into Settings again. After you upgrade again, re-enter any API key or calendar link you changed while on the older version, because Conclave keeps the copy it already stored in Windows Credential Manager. If you go back to an older version and then reinstall the same version you had before, Conclave does not make a new automatic backup for that step; click **Back up now** in Settings first.

## Uninstall

Use Windows Settings → Apps & features (or Control Panel → Programs and Features) → Conclave → Uninstall. The uninstaller has a "Delete the application data" checkbox. It is unchecked by default. Checking it removes the `%APPDATA%\com.conclave.app` folder (meetings, settings, models and backups). It does not remove `%APPDATA%\Conclave` or `%USERPROFILE%\Music\conclave-recordings`; delete those by hand if you want them gone. Read the [backup warning](#back-up-before-you-update) before you check that box. Do not delete the install folder by hand, because that leaves the Start-menu entry and the uninstall registration behind.

## Credits

Based on Meetily (MIT) by Zackriya Solutions: [github.com/Zackriya-Solutions/meeting-minutes](https://github.com/Zackriya-Solutions/meeting-minutes).
