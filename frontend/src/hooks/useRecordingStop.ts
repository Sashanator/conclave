import { useState, useEffect, useCallback, useRef } from 'react';
import { useRouter } from 'next/navigation';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useRecordingState, RecordingStatus } from '@/contexts/RecordingStateContext';
import { storageService } from '@/services/storageService';
import { transcriptService } from '@/services/transcriptService';
import {
  applyPinnedSummaryLanguageToMeeting,
  detectAndCacheSummaryLanguage,
} from '@/lib/summary-language-preferences';
import { beginStop, endStop, noteRecordingStarted, noteRecordingStopped, startsSeen, stopsSeen } from '@/lib/recording-stop-gate';
import { saveOutcome } from '@/lib/resume';

type SummaryStatus = 'idle' | 'processing' | 'summarizing' | 'regenerating' | 'completed' | 'error';

export interface UseRecordingStopReturn {
  handleRecordingStop: (callApi: boolean) => Promise<void>;
  isStopping: boolean;
  isProcessingTranscript: boolean;
  isSavingTranscript: boolean;
  summaryStatus: SummaryStatus;
  setIsStopping: (value: boolean) => void;
}

/**
 * Custom hook for managing recording stop lifecycle.
 * Handles the complex stop sequence: transcription wait → buffer flush → SQLite save → navigation.
 *
 * Features:
 * - Transcription completion polling (60s max, 500ms interval; saves what it has after that)
 * - Transcript buffer flush coordination
 * - SQLite meeting save with folder_path from sessionStorage
 * - Auto-navigation to meeting details
 * - Toast notifications for success/error
 * - Window exposure for Rust callbacks
 */
export function useRecordingStop(
  setIsRecording: (value: boolean) => void,
  setIsRecordingDisabled: (value: boolean) => void
): UseRecordingStopReturn {
  // USE global state instead
  const recordingState = useRecordingState();
  const {
    status,
    setStatus,
    isStopping,
    isProcessing: isProcessingTranscript,
    isSaving: isSavingTranscript
  } = recordingState;

  const {
    transcriptsRef,
    notesRef,
    flushBuffer,
    clearTranscripts,
    meetingTitle,
    titleSource,
    calendarLink,
    markMeetingAsSaved,
    currentMeetingId,
  } = useTranscripts();

  const router = useRouter();

  // Promise to track recording-stopped event data (fixes race condition with recording-stop-complete)
  const recordingStoppedDataRef = useRef<Promise<void> | null>(null);

  // Set up recording-stopped listener for meeting navigation
  useEffect(() => {
    let unlistenFn: (() => void) | undefined;
    let unlistenStarted: (() => void) | undefined;

    const setupRecordingStoppedListener = async () => {
      try {
        console.log('Setting up recording-stopped listener for navigation...');
        unlistenFn = await listen<{
          message: string;
          folder_path?: string;
          meeting_name?: string;
          meeting_id?: string | null;
          held_meeting_id?: string | null;
          recording_generation?: number | null;
        }>('recording-stopped', async (event) => {
          noteRecordingStopped();
          // Create promise that resolves when sessionStorage is set (prevents race condition)
          recordingStoppedDataRef.current = (async () => {
            const { folder_path, meeting_name, meeting_id, held_meeting_id, recording_generation } = event.payload;

            // Store folder_path and meeting_name for later use in handleRecordingStop
            if (folder_path) {
              sessionStorage.setItem('last_recording_folder_path', folder_path);
            }
            if (meeting_name) {
              sessionStorage.setItem('last_recording_meeting_name', meeting_name);
            }
            // Always written (or cleared): a stale id from an earlier recording must never reach this save.
            if (meeting_id) {
              sessionStorage.setItem('last_recording_meeting_id', meeting_id);
            } else {
              sessionStorage.removeItem('last_recording_meeting_id');
            }
            if (recording_generation != null) {
              sessionStorage.setItem('last_recording_generation', String(recording_generation));
            } else {
              sessionStorage.removeItem('last_recording_generation');
            }
            // The meeting page holds this one until the save ends: a continue target has no routed id.
            if (held_meeting_id) {
              sessionStorage.setItem('last_recording_held_id', held_meeting_id);
            } else {
              sessionStorage.removeItem('last_recording_held_id');
            }
          })();

        });
        unlistenStarted = await listen('recording-started', () => noteRecordingStarted());
        console.log('Recording stopped listener setup complete');
      } catch (error) {
        console.error('Failed to setup recording stopped listener:', error);
      }
    };

    setupRecordingStoppedListener();

    return () => {
      console.log('Cleaning up recording stopped listener...');
      unlistenStarted?.();
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [router]);

  // Main recording stop handler
  const handleRecordingStop = useCallback(async (isCallApi: boolean) => {
    // Everything this save needs is taken here, at stop time and before waiting for the gate: a queued stop
    // must not read the lines or session keys of a recording that starts while it waits (they are rewritten then).
    const dataPromise = recordingStoppedDataRef.current;
    if (dataPromise) {
      // A failed data write must not lose the save: it reads sessionStorage either way.
      await dataPromise.catch((error) => console.error('Recording-stopped data unavailable:', error));
    }
    // Lines still in the buffer belong to this recording: flush them, and wait a tick for them to land.
    flushBuffer();
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    const folderPath = sessionStorage.getItem('last_recording_folder_path');
    const savedMeetingName = sessionStorage.getItem('last_recording_meeting_name');
    const routedMeetingId = sessionStorage.getItem('last_recording_meeting_id');
    const generationText = sessionStorage.getItem('last_recording_generation');
    const recordingGeneration = generationText === null ? null : Number(generationText);
    const indexedDbId = currentMeetingId || sessionStorage.getItem('indexeddb_current_meeting_id');
    const saveTitle = savedMeetingName || meetingTitle || 'Untitled'; // PREFER savedMeetingName (backend source)
    const saveTitleOrigin = { source: titleSource, uid: calendarLink?.uid, start: calendarLink?.start };
    // App-wide counters (not hook refs): they keep counting after this page unmounts.
    const startsAtStop = startsSeen();
    const stopsAtStop = stopsSeen();
    // False once a recording started after this stop: its state is no longer this save's to touch.
    const isCurrent = () => startsSeen() === startsAtStop;
    const setStatusIfCurrent = (...args: Parameters<typeof setStatus>) => { if (isCurrent()) setStatus(...args); };
    const stoppedTranscripts = [...transcriptsRef.current];
    const stoppedNotes = [...notesRef.current];

    // Guard: one save at a time across every instance (UI and tray stops both reach here). A refused call
    // returns before it touches the post-processing flag, which only the owner clears.
    if (!(await beginStop())) {
      return;
    }
    // Keeps meeting detection from starting a recording over this save (cleared in finally).
    invoke('set_recording_post_processing', { active: true }).catch(() => {});

    // Set status to STOPPING immediately
    setStatus(RecordingStatus.STOPPING);
    setIsRecording(false);
    setIsRecordingDisabled(true);
    const stopStartTime = Date.now();

    try {
      console.log('Post-stop processing (new implementation)...', {
        stop_initiated_at: new Date(stopStartTime).toISOString(),
        current_transcript_count: transcriptsRef.current.length
      });

      // Note: stop_recording is already called by RecordingControls.stopRecordingAction
      // This function only handles post-stop processing (transcription wait, API call, navigation)
      console.log('Recording already stopped by RecordingControls, processing transcription...');

      // Wait for transcription to complete
      setStatusIfCurrent(RecordingStatus.PROCESSING_TRANSCRIPTS, 'Waiting for transcription...');
      console.log('Waiting for transcription to complete...');

      const MAX_WAIT_TIME = 60000; // 60 seconds maximum wait (increased for longer processing)
      const POLL_INTERVAL = 500; // Check every 500ms
      let elapsedTime = 0;
      let transcriptionComplete = false;

      // Listen for transcription-complete event
      const unlistenComplete = await listen('transcription-complete', () => {
        console.log('Received transcription-complete event');
        transcriptionComplete = true;
      });

      // Poll for transcription status
      while (elapsedTime < MAX_WAIT_TIME && !transcriptionComplete) {
        try {
          const status = await transcriptService.getTranscriptionStatus();
          console.log('Transcription status:', status);

          // Check if transcription is complete
          if (!status.is_processing && status.chunks_in_queue === 0) {
            console.log('Transcription complete - no active processing and no chunks in queue');
            transcriptionComplete = true;
            break;
          }

          // If no activity for more than 8 seconds and no chunks in queue, consider it done (increased from 5s to 8s)
          if (status.last_activity_ms > 8000 && status.chunks_in_queue === 0) {
            console.log('Transcription likely complete - no recent activity and empty queue');
            transcriptionComplete = true;
            break;
          }

          // Update user with current status
          if (status.chunks_in_queue > 0) {
            console.log(`Processing ${status.chunks_in_queue} remaining audio chunks...`);
            setStatusIfCurrent(RecordingStatus.PROCESSING_TRANSCRIPTS, `Processing ${status.chunks_in_queue} remaining chunks...`);
          }

          // Wait before next check
          await new Promise(resolve => setTimeout(resolve, POLL_INTERVAL));
          elapsedTime += POLL_INTERVAL;
        } catch (error) {
          console.error('Error checking transcription status:', error);
          break;
        }
      }

      // Clean up listener
      console.log('🧹 CLEANUP: Cleaning up transcription-complete listener');
      unlistenComplete();

      if (!transcriptionComplete) {
        // Save what we have anyway: skipping the save here used to lose the whole meeting.
        console.warn('⏰ Transcription not confirmed complete after', elapsedTime, 'ms; saving what is transcribed so far');
      } else {
        console.log('✅ Transcription completed after', elapsedTime, 'ms');
        // Wait longer for any late transcript segments (increased from 1s to 4s)
        console.log('⏳ Waiting for late transcript segments...');
        await new Promise(resolve => setTimeout(resolve, 4000));
      }

      // Final buffer flush: process ALL remaining transcripts regardless of timing
      const flushStartTime = Date.now();
      console.log('🔄 Final buffer flush: forcing processing of any remaining transcripts...', {
        flush_started_at: new Date(flushStartTime).toISOString(),
        time_since_stop: flushStartTime - stopStartTime,
        current_transcript_count: transcriptsRef.current.length
      });
      setStatusIfCurrent(RecordingStatus.PROCESSING_TRANSCRIPTS, 'Flushing transcript buffer...');
      flushBuffer();
      const flushEndTime = Date.now();
      console.log('✅ Final buffer flush completed', {
        flush_duration: flushEndTime - flushStartTime,
        total_time_since_stop: flushEndTime - stopStartTime,
        final_transcript_count: transcriptsRef.current.length
      });

      // NOTE: Status remains PROCESSING_TRANSCRIPTS until we start saving

      // Wait a bit more to ensure all transcript state updates have been processed
      console.log('Waiting for transcript state updates to complete...');
      await new Promise(resolve => setTimeout(resolve, 500));

      // Save to SQLite
      // NOTE: enabled to save COMPLETE transcripts after frontend receives all updates
      // This ensures user sees all transcripts streaming in before database save
      if (isCallApi) {

        setStatusIfCurrent(RecordingStatus.SAVING, 'Saving meeting to database...');

        // Get fresh transcript state (ALL transcripts including late ones)
        // (Late lines arrive in the live list; once a new recording replaced it, the lines taken at stop are all there is.)
        const sameRecording = isCurrent();
        const freshTranscripts = sameRecording ? [...transcriptsRef.current] : stoppedTranscripts;

        console.log('💾 Saving COMPLETE transcripts to database...', {
          transcript_count: freshTranscripts.length,
          title_chars: saveTitle.length,
          folder_path_set: !!folderPath,
        });

        try {
          const responseData = await storageService.saveMeeting(
            saveTitle,
            freshTranscripts,
            folderPath,
            (sameRecording ? [...notesRef.current] : stoppedNotes),
            indexedDbId,
            saveTitleOrigin,
            true,
            routedMeetingId,
            recordingGeneration
          );

          const meetingId = responseData.meeting_id;
          if (!meetingId) {
            console.error('No meeting_id in response:', responseData);
            throw new Error('No meeting ID received from save operation');
          }
          const outcome = saveOutcome(responseData);
          if (outcome.continueError) {
            toast.error('Could not add this recording to the earlier meeting', {
              description: `${outcome.continueError} It was saved ${routedMeetingId && !outcome.fallback ? 'into the meeting it started in' : 'as a new meeting'} instead.`,
            });
          }
          if (outcome.fallback && !outcome.continueError) {
            toast.warning('Saved as a new meeting', {
              description: 'Its meeting was deleted, or it already holds a recording.',
            });
          }
          if (responseData.release_error) {
            toast.error('An empty meeting was left behind', {
              description: `${responseData.release_error} Delete the empty started meeting by hand.`,
            });
          }
          if (!transcriptionComplete) {
            toast.warning('Saved before transcription finished', {
              description: 'The last part of the transcript may be missing.',
            });
          }

          // A continued recording keeps the earlier meeting's language settings
          let shouldDetectSummaryLanguage = false;
          if (!outcome.continued) {
            try {
              shouldDetectSummaryLanguage = !(await applyPinnedSummaryLanguageToMeeting(meetingId));
            } catch (error) {
              console.warn('Failed to apply pinned summary language preference for new meeting:', error);
              toast.warning('Could not apply default summary language', {
                description: 'The meeting was saved, but the default summary language was not applied.',
              });
            }

            if (shouldDetectSummaryLanguage) {
              try {
                await detectAndCacheSummaryLanguage(
                  meetingId,
                  freshTranscripts.map(t => t.text)
                );
              } catch (error) {
                console.warn('Failed to detect summary language for new meeting:', error);
                toast.warning('Could not detect summary language', {
                  description: 'The meeting was saved, but Auto could not detect the summary language.',
                });
              }
            }
          }

          console.log('✅ Successfully saved COMPLETE meeting with ID:', meetingId);
          console.log('   Transcripts:', freshTranscripts.length);
          console.log('   folder saved:', Boolean(folderPath));

          // Mark meeting as saved in IndexedDB (for recovery system)
          await markMeetingAsSaved(indexedDbId ?? undefined);

          // Clean up session storage: a newer stop owns the stop keys, a newer start the recording keys
          if (stopsSeen() === stopsAtStop) {
            sessionStorage.removeItem('last_recording_generation');
            sessionStorage.removeItem('last_recording_folder_path');
            sessionStorage.removeItem('last_recording_meeting_name');
            sessionStorage.removeItem('last_recording_meeting_id');
            sessionStorage.removeItem('last_recording_held_id');
          }
          if (isCurrent()) {
            // Redundant with markMeetingAsSaved cleanup, but ensures cleanup
            sessionStorage.removeItem('indexeddb_current_meeting_id');
            sessionStorage.removeItem('recording_title_origin');
          }

          // Mark as completed
          setStatusIfCurrent(RecordingStatus.COMPLETED);

          // Show success toast with navigation option
          toast.success('Recording saved successfully!', {
            description: outcome.continued
              ? 'Added to the earlier meeting.'
              : `${freshTranscripts.length} transcript segments saved.`,
            action: {
              label: 'View Meeting',
              onClick: () => {
                router.push(outcome.url);
              }
            },
            duration: 10000,
          });

          // Auto-navigate after a short delay with source parameter
          setTimeout(() => {
            // A recording started meanwhile owns the page, the lines and the status.
            if (!isCurrent()) return;
            router.push(outcome.url);
            clearTranscripts()

            // Reset to IDLE after navigation
            setStatusIfCurrent(RecordingStatus.IDLE);
          }, 2000);
        } catch (saveError) {
          console.error('Failed to save meeting to database:', saveError);
          setStatusIfCurrent(RecordingStatus.ERROR, saveError instanceof Error ? saveError.message : 'Unknown error');
          toast.error('Failed to save meeting', {
            description: saveError instanceof Error ? saveError.message : 'Unknown error'
          });
          throw saveError;
        }
      } else {
        // No save needed, go back to IDLE
        setStatusIfCurrent(RecordingStatus.IDLE);
      }

      // isRecording already set to false at function start
      setIsRecordingDisabled(false);
    } catch (error) {
      console.error('Error in handleRecordingStop:', error);
      setStatusIfCurrent(RecordingStatus.ERROR, error instanceof Error ? error.message : 'Unknown error');
      // isRecording already set to false at function start
      setIsRecordingDisabled(false);
    } finally {
      // Always reset the guard flag when done
      endStop();
      invoke('set_recording_post_processing', { active: false }).catch(() => {});
      // The meeting may be deleted again now that its save is over (Rust kept it protected since the stop).
      if (routedMeetingId) invoke('api_finish_saving_target', { meetingId: routedMeetingId }).catch(() => {});
    }
  }, [
    setIsRecording,
    setIsRecordingDisabled,
    setStatus,
    transcriptsRef,
    notesRef,
    flushBuffer,
    clearTranscripts,
    meetingTitle,
    titleSource,
    calendarLink,
    markMeetingAsSaved,
    currentMeetingId,
    router,
  ]);

  // Expose handleRecordingStop function to window for Rust callbacks
  const handleRecordingStopRef = useRef(handleRecordingStop);
  useEffect(() => {
    handleRecordingStopRef.current = handleRecordingStop;
  });

  useEffect(() => {
    (window as any).handleRecordingStop = (callApi: boolean = true) => {
      handleRecordingStopRef.current(callApi);
    };

    // Cleanup on unmount
    return () => {
      delete (window as any).handleRecordingStop;
    };
  }, []);

  // Derive summaryStatus from RecordingStatus for backward compatibility
  const summaryStatus: SummaryStatus = status === RecordingStatus.PROCESSING_TRANSCRIPTS ? 'processing' : 'idle';

  return {
    handleRecordingStop,
    isStopping,
    isProcessingTranscript,
    isSavingTranscript,
    summaryStatus,
    setIsStopping: (value: boolean) => {
      setStatus(value ? RecordingStatus.STOPPING : RecordingStatus.IDLE);
    },
  };
}
