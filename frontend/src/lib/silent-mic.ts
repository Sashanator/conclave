/** The mic counts as silent after this long at (near) zero while recording. */
export const SILENT_MIC_MS = 10_000;
/** About -100 dBFS: only a dead mic (digital silence) is this quiet; a working mic's room noise is far above it. */
export const SILENT_PEAK = 0.00001;

export const hasSound = (peak: number): boolean => peak >= SILENT_PEAK;

/** Silent when nothing but silence has been heard since `lastSoundAt` (ms), for at least `SILENT_MIC_MS`. */
export const micSilent = (lastSoundAt: number, now: number): boolean => now - lastSoundAt >= SILENT_MIC_MS;
