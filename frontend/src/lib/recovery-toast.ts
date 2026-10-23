// Wording for the toast after a meeting is recovered. Only a 'success' or 'partial' audio status
// means some audio came back; anything else is reported as no audio.
export function recoveredDescription(audioStatus: string | undefined): string {
  if (audioStatus === 'success') return 'Transcripts and audio recovered';
  if (audioStatus === 'partial') {
    return 'Transcripts and part of the audio recovered (some audio could not be restored)';
  }
  return 'Transcripts recovered (no audio available)';
}
