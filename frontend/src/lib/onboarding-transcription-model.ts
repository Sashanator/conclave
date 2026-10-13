export const ONBOARDING_WHISPER_MODEL = 'large-v3-turbo';
export const ONBOARDING_WHISPER_SIZE_MB = 1549; // config.rs WHISPER_MODEL_CATALOG

export function isOnboardingModelEvent(payload: unknown): boolean {
  return (
    typeof payload === 'object' && payload !== null
    && (payload as { modelName?: unknown }).modelName === ONBOARDING_WHISPER_MODEL
  );
}

export function progressToDownloadedMb(progress: number): number {
  const pct = Math.min(100, Math.max(0, progress));
  return (pct / 100) * ONBOARDING_WHISPER_SIZE_MB;
}
