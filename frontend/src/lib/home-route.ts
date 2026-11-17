import type { RecordingStatus } from '@/contexts/RecordingStateContext';

// Stop flow after isRecording is already false: the save progress lives on /record (RecordingStatus values).
const STOP_FLOW = new Set<string>(['starting', 'stopping', 'processing', 'saving']);

/** Sidebar Home: the live recording and its stop/save progress are on /record, so Home goes there while either is active. */
export const homeRoute = (isRecording: boolean, status: RecordingStatus): string =>
  isRecording || STOP_FLOW.has(status) ? '/record' : '/';
