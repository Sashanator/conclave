/** Sidebar Home: while recording, the live recording is on /record, so Home goes there. */
export const homeRoute = (isRecording: boolean): string => (isRecording ? '/record' : '/');
