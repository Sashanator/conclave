"use client"
import { useSidebar } from "@/components/Sidebar/SidebarProvider";
import { useState, useEffect, useCallback, useRef, Suspense } from "react";
import { MeetingSummary, SummaryProcessResponse, Transcript, TranscriptMark } from "@/types";
import PageContent from "./page-content";
import { useRouter, useSearchParams } from "next/navigation";
import { invoke } from "@tauri-apps/api/core";
import { LoaderIcon } from "lucide-react";
import { useConfig } from "@/contexts/ConfigContext";
import { usePaginatedTranscripts } from "@/hooks/usePaginatedTranscripts";
import { parseSummaryContent } from "@/lib/summary-content";
import { toast } from "sonner";
import { lineAtTime, parseMeetingTarget } from "@/lib/meeting-target";
import { heldMeetingId, pageKeyFor, showRecordingView } from "@/lib/recording-target";
import { useRecordingStart } from "@/hooks/useRecordingStart";
import { useModalState } from "@/hooks/useModalState";
import { SettingsModals } from "@/app/_components/SettingsModal";
import { useRecordingState, RecordingStatus } from "@/contexts/RecordingStateContext";
import { useAiSidebarScreen } from "@/components/AiSidebar/AiSidebarProvider";
import { listMeetingRows, type MeetingRow } from "@/lib/meetings";
import { MeetingHeader } from "@/components/MeetingPage/MeetingHeader";
import { RecordingView } from "@/components/MeetingPage/RecordingView";
import { NewMeetingView } from "@/components/MeetingPage/NewMeetingView";

interface MeetingDetailsResponse {
  id: string;
  title: string;
  created_at: string;
  updated_at: string;
  transcripts: Transcript[];
  folder_path?: string;
}

function MeetingDetailsContent() {
  const searchParams = useSearchParams();
  const meetingId = searchParams.get('id');
  const source = searchParams.get('source'); // Check if navigated from recording
  const continued = searchParams.get('continued') === '1'; // A recording was added to this meeting: replace its summary
  const target = parseMeetingTarget(searchParams); // Search hits: a line to scroll to, or a tab to open
  const { setCurrentMeeting, refetchMeetings } = useSidebar();
  const { isAutoSummary, transcriptModelConfig } = useConfig(); // Get auto-summary toggle state
  const { liveMeetingId, isStopping, isProcessing, isSaving, isRecording, status } = useRecordingState();
  const { meetings } = useSidebar();
  // undefined: loading; null: no row (an old link), shown as the finished page
  const [row, setRow] = useState<MeetingRow | null | undefined>(undefined);
  // The page mounts the start hook for every non-page start (tray, detector, Resume): they land here.
  const [startingLocally, setStartingLocally] = useState(false);
  const { modals, messages, showModal, hideModal } = useModalState(transcriptModelConfig);
  // A start that reached the backend leaves startingLocally set; the recording ending clears it for the next start.
  useEffect(() => { if (!isRecording) setStartingLocally(false); }, [isRecording]);
  const router = useRouter();
  // A failed start gives its empty row back: reload it, or leave a meeting that no longer exists.
  const checkRowAfterRelease = useCallback(() => {
    listMeetingRows()
      .then((rows) => {
        const found = rows.find((r) => r.id === meetingId);
        if (found) { setRow(found); return; }
        toast.error('This meeting was removed because the recording could not start');
        router.push('/');
      })
      .catch((e) => toast.error(`Could not reload the meeting: ${String(e)}`));
  }, [meetingId, router]);
  const { handleRecordingStart, isAutoStarting } = useRecordingStart(startingLocally, setStartingLocally, showModal, pageKeyFor(meetingId, searchParams.get('start')), checkRowAfterRelease);
  // The backend clears liveMeetingId when the stop lands; the save that follows stays on this view, and so does a failed one.
  const stopFlow = isStopping || isProcessing || isSaving || status === RecordingStatus.ERROR;
  const recordingHere = showRecordingView({ meetingId, liveMeetingId, heldId: heldMeetingId(stopFlow), stopFlow });
  useAiSidebarScreen(meetingId ? (meetingId === liveMeetingId ? 'live' : 'meeting') : null, { label: 'This meeting', meetingIds: meetingId ? [meetingId] : [] });
  // When the hold ends the page's data is from before the recording: load it again (and auto-summarize).
  const [savedEpoch, setSavedEpoch] = useState(0);
  const wasHeld = useRef(false);
  useEffect(() => {
    if (wasHeld.current && !recordingHere) setSavedEpoch((e) => e + 1);
    wasHeld.current = recordingHere;
  }, [recordingHere]);
  const [meetingDetails, setMeetingDetails] = useState<MeetingDetailsResponse | null>(null);
  const [summaryResponse, setSummaryResponse] = useState<SummaryProcessResponse | null>(null);
  const [meetingSummary, setMeetingSummary] = useState<MeetingSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [shouldAutoGenerate, setShouldAutoGenerate] = useState<boolean>(false);
  const [hasCheckedAutoGen, setHasCheckedAutoGen] = useState<boolean>(false);

  // Use pagination hook for efficient transcript loading
  const {
    metadata,
    segments,
    transcripts,
    isLoading: isLoadingTranscripts,
    isLoadingMore,
    hasMore,
    totalCount,
    loadedCount,
    loadMore,
    refetch,
    error: transcriptError,
    setTranscriptMark,
    setTranscriptSpeaker,
  } = usePaginatedTranscripts({ meetingId: meetingId || '' });

  // A task's time (t) opens the transcript row at or before it. Pages load until that row is known.
  const targetTime = target?.t;
  const targetLine = target?.line;
  const timeLine = targetTime !== undefined && !targetLine ? lineAtTime(segments, targetTime, hasMore) : null;
  useEffect(() => {
    if (targetTime === undefined || targetLine || !hasMore || isLoadingMore) return;
    const last = segments[segments.length - 1];
    if (!last || last.timestamp <= targetTime) loadMore();
  }, [targetTime, targetLine, hasMore, isLoadingMore, segments, loadMore]);

  // The row decides New or finished. It reloads when a recording ends so the saved meeting shows as finished.
  useEffect(() => { setRow(undefined); }, [meetingId]);
  useEffect(() => {
    if (!meetingId || meetingId === 'intro-call') { setRow(null); return; }
    let cancelled = false;
    listMeetingRows()
      .then((rows) => { if (!cancelled) setRow(rows.find((r) => r.id === meetingId) ?? null); })
      .catch((e) => { console.error('Could not load the meeting row:', e); if (!cancelled) { toast.error(`Could not load the meeting: ${String(e)}`); setRow(null); } });
    return () => { cancelled = true; };
  }, [meetingId, liveMeetingId]);

  // Check if gemma3:1b model is available in Ollama
  const checkForGemmaModel = useCallback(async (): Promise<boolean> => {
    try {
      const models = await invoke('get_ollama_models', { endpoint: null }) as any[];
      const hasGemma = models.some((m: any) => m.name === 'gemma3:1b');
      console.log('🔍 Checked for gemma3:1b:', hasGemma);
      return hasGemma;
    } catch (error) {
      console.error('❌ Failed to check Ollama models:', error);
      return false;
    }
  }, []);

  // Set up auto-generation - respects DB as source of truth
  const setupAutoGeneration = useCallback(async () => {
    if (hasCheckedAutoGen) return; // Only check once

    // Only auto-generate if navigated from recording
    if (source !== 'recording') {
      console.log('Not from recording navigation, skipping auto-generation');
      setHasCheckedAutoGen(true);
      return;
    }

    // Respect user's auto-summary toggle preference
    if (!isAutoSummary) {
      console.log('Auto-summary is disabled in settings');
      setHasCheckedAutoGen(true);
      return;
    }

    try {
      // Check what's currently in database
      const currentConfig = await invoke('api_get_model_config') as any;

      // If DB already has a model, use it (never override!)
      if (currentConfig && currentConfig.model) {
        console.log('Using existing model from DB:', currentConfig.model);
        setShouldAutoGenerate(true);
        setHasCheckedAutoGen(true);
        return;
      }

      // DB is empty - check if gemma3:1b exists as fallback
      const hasGemma = await checkForGemmaModel();

      if (hasGemma) {
        console.log('💾 DB empty, using gemma3:1b as initial default');

        await invoke('api_save_model_config', {
          provider: 'ollama',
          model: 'gemma3:1b',
          whisperModel: 'large-v3',
          apiKey: null,
          ollamaEndpoint: null,
        });

        setShouldAutoGenerate(true);
      } else {
        console.log('⚠️ No model configured and gemma3:1b not found');
      }
    } catch (error) {
      console.error('❌ Failed to setup auto-generation:', error);
    }

    setHasCheckedAutoGen(true);
  }, [hasCheckedAutoGen, checkForGemmaModel, source, isAutoSummary]);

  // Sync meeting metadata from pagination hook to meeting details state
  useEffect(() => {
    if (isLoadingTranscripts || !meetingId || meetingId === 'intro-call' || metadata?.id !== meetingId) {
      // Keep the current view until the selected meeting's first transcript page is ready.
      return;
    }

    if (metadata) {
      console.log('Meeting metadata loaded:', metadata);

      // Build meeting details from metadata and paginated transcripts
      setMeetingDetails({
        id: metadata.id,
        title: metadata.title,
        created_at: metadata.created_at,
        updated_at: metadata.updated_at,
        transcripts: transcripts, // Paginated transcripts from hook
        folder_path: metadata.folder_path, // For retranscription feature
      });

      // Sync with sidebar context
      setCurrentMeeting({ id: metadata.id, title: metadata.title });
    }
  }, [metadata, transcripts, meetingId, isLoadingTranscripts, setCurrentMeeting]);

  useEffect(() => { if (savedEpoch) void refetch(); }, [savedEpoch, refetch]);

  // Handle transcript loading errors
  useEffect(() => {
    if (transcriptError) {
      console.error('Error loading transcripts:', transcriptError);
      setError(transcriptError);
    }
  }, [transcriptError]);

  // Extract fetchMeetingDetails for use in child components (now refetches via hook)
  const fetchMeetingDetails = useCallback(async () => {
    if (!meetingId || meetingId === 'intro-call') {
      return;
    }

    // The usePaginatedTranscripts hook automatically refetches when meetingId changes
    // This function is kept for compatibility with onMeetingUpdated callback
    console.log('fetchMeetingDetails called - pagination hook will handle refetch');
  }, [meetingId]);

  // Reset states when meetingId changes (prevent race conditions)
  useEffect(() => {
    setMeetingDetails(null);
    setMeetingSummary(null);
    setSummaryResponse(null);
    setError(null);
    setIsLoading(true);
    // Reset auto-generation state to allow new meeting to be checked
    setHasCheckedAutoGen(false);
    setShouldAutoGenerate(false);
  }, [meetingId, savedEpoch, source]);

  useEffect(() => {
    console.log('MeetingDetails useEffect triggered - meetingId:', meetingId);

    if (!meetingId || meetingId === 'intro-call') {
      console.warn('No valid meeting ID in URL - meetingId:', meetingId);
      setError("No meeting selected");
      setIsLoading(false);
      return;
    }

    console.log('Valid meeting ID found, fetching details for:', meetingId);

    setMeetingDetails(null);
    setMeetingSummary(null);
    setSummaryResponse(null);
    setError(null);
    setIsLoading(true);

    let cancelled = false;
    const fetchMeetingSummary = async () => {
      try {
        const response = await invoke<SummaryProcessResponse>('api_get_summary', {
          meetingId,
        });
        if (cancelled) return;
        setSummaryResponse(response);
        const summary = parseSummaryContent(response.data);
        setMeetingSummary(response.status === 'idle' ? null : summary);
      } catch (error) {
        if (cancelled) return;
        console.error('FETCH SUMMARY: Error fetching meeting summary:', error);
        setMeetingSummary(null);
      }
    };

    const loadData = async () => {
      try {
        await fetchMeetingSummary();
      } finally {
        if (!cancelled) setIsLoading(false);
      }
    };

    loadData();
    return () => { cancelled = true; };
  }, [meetingId, savedEpoch, source]);

  // Auto-generation check: runs when meeting is loaded with no summary
  useEffect(() => {
    const checkAutoGen = async () => {
      // Only auto-generate if:
      // 1. We have meeting details
      // 2. No summary exists
      // 3. Meeting has transcripts
      // 4. Haven't checked yet
      if (
        meetingDetails &&
        !isLoading &&
        summaryResponse?.status === 'idle' &&
        meetingSummary === null &&
        meetingDetails.transcripts &&
        meetingDetails.transcripts.length > 0 &&
        !hasCheckedAutoGen
      ) {
        console.log('No summary found, checking for auto-generation...');
        await setupAutoGeneration();
      }
    };

    checkAutoGen();
  }, [meetingDetails, meetingSummary, summaryResponse, isLoading, hasCheckedAutoGen, setupAutoGeneration]);

  const busy = startingLocally || isAutoStarting || isStopping || isProcessing || isSaving;
  const liveElsewhere = liveMeetingId && liveMeetingId !== meetingId
    ? { id: liveMeetingId, title: meetings.find((m) => m.id === liveMeetingId)?.title ?? 'another meeting' }
    : null;
  let shell: React.ReactNode = null;
  if (meetingId && row === undefined) {
    shell = <div className="flex items-center justify-center h-screen"><LoaderIcon className="animate-spin size-6" /></div>;
  } else if (row && recordingHere) {
    shell = <RecordingView row={row} onChange={setRow} onTitleSaved={refetchMeetings} showModal={showModal} />;
  } else if (row && !row.startedAt && !row.hasTranscript) {
    shell = (
      <div className="h-screen overflow-y-auto bg-muted/50">
        <div className="mx-auto w-full max-w-3xl px-6 pt-6"><MeetingHeader row={row} live={busy} onChange={setRow} onTitleSaved={refetchMeetings} onMeetingDeleted={() => { void refetchMeetings().finally(() => router.push('/')); }} /></div>
        <NewMeetingView row={row} live={liveElsewhere} disabled={busy} onStart={() => { handleRecordingStart(row.id).catch((e) => toast.error('Could not start recording', { description: e instanceof Error ? e.message : String(e) })); }} />
      </div>
    );
  }
  const modalsEl = <SettingsModals modals={modals} messages={messages} onClose={hideModal} />;
  if (shell) return <>{modalsEl}{shell}</>;

  if (error) {
    return (
      <div className="flex items-center justify-center h-screen">{modalsEl}
        <div className="text-center">
          <p className="text-red-500 mb-4">{error}</p>
          <button
            onClick={() => router.push('/')}
            className="px-4 py-2 bg-blue-500 text-white rounded hover:bg-blue-600"
          >
            Go Back
          </button>
        </div>
      </div>
    );
  }

  // Show loading spinner while initial data loads
  if (isLoading || !meetingDetails || meetingDetails.id !== meetingId) {
    return <div className="flex items-center justify-center h-screen">
      {modalsEl}
      <LoaderIcon className="animate-spin size-6 " />
    </div>;
  }

  return <>{modalsEl}<PageContent
    key={meetingId}
    initialSummary={summaryResponse}
    meeting={meetingDetails}
    summaryData={meetingSummary}
    shouldAutoGenerate={shouldAutoGenerate}
    regenerateOnAuto={continued}
    onAutoGenerateComplete={() => setShouldAutoGenerate(false)}
    onMeetingUpdated={async () => {
      // Refetch meeting details to get updated title from backend
      await fetchMeetingDetails();
      // Refetch meetings list to update sidebar
      await refetchMeetings();
      // The header shows the row's title, which a summary rename changed
      await listMeetingRows().then((rows) => { const r = rows.find((x) => x.id === meetingId); if (r) setRow(r); }).catch(() => {});
    }}
    onRefetchTranscripts={refetch}
    // Pagination props for efficient transcript loading
    segments={segments}
    hasMore={hasMore}
    isLoadingMore={isLoadingMore}
    totalCount={totalCount}
    loadedCount={loadedCount}
    onLoadMore={loadMore}
    onSetTranscriptMark={setTranscriptMark}
    onSetTranscriptSpeaker={setTranscriptSpeaker}
    focusSegmentId={targetLine ?? timeLine}
    requestedTab={target?.tab ?? null}
    reviewed={row?.reviewed ?? false}
    row={row}
    onRowChange={setRow}
  /></>;
}

export default function MeetingDetails() {
  return (
    <Suspense fallback={
      <div className="flex items-center justify-center h-screen">
        <LoaderIcon className="animate-spin size-6" />
      </div>
    }>
      <MeetingDetailsContent />
    </Suspense>
  );
}
