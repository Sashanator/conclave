"use client";
import { useState, useEffect, useRef, useMemo } from 'react';
import { motion } from 'framer-motion';
import { MeetingSummary, SummaryProcessResponse, Transcript, TranscriptMark } from '@/types';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { useRouter } from 'next/navigation';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { TranscriptPanel } from '@/components/MeetingDetails/TranscriptPanel';
import { SummaryPanel } from '@/components/MeetingDetails/SummaryPanel';
import { MeetingDetailsSplitView, type MeetingDetailsTab } from '@/components/MeetingDetails/MeetingDetailsSplitView';
import { AudioPlayerBar } from '@/components/MeetingDetails/AudioPlayerBar';
import { NotesPane } from '@/components/MeetingNotes/NotesPane';
import { ModelConfig } from '@/components/ModelSettingsModal';

// Custom hooks
import { useMeetingData } from '@/hooks/meeting-details/useMeetingData';
import { useSummaryGeneration } from '@/hooks/meeting-details/useSummaryGeneration';
import { useTemplates } from '@/hooks/meeting-details/useTemplates';
import { useCopyOperations } from '@/hooks/meeting-details/useCopyOperations';
import { useMeetingOperations } from '@/hooks/meeting-details/useMeetingOperations';
import { useMeetingNotes } from '@/hooks/useMeetingNotes';
import { useMeetingAudio } from '@/hooks/useMeetingAudio';
import { useMeetingSpeakers } from '@/hooks/useMeetingSpeakers';
import { useConfig } from '@/contexts/ConfigContext';
import { lineSpeakerName } from '@/lib/speakers';
import { readSummaryMetadata } from '@/lib/summary-content';
import { pickTemplateIfAllowed } from '@/lib/regenerate-guard';
import { audioMissingMessage } from '@/lib/meeting-audio';
import { requestResume } from '@/lib/resume';
import { notesCollapsedOnScreen, readFlag, readMeetingPageLayout, writeFlag } from '@/lib/meeting-page-layout';
import { useIsDesktop } from '@/hooks/useIsDesktop';

export default function PageContent({
  meeting,
  summaryData,
  initialSummary,
  shouldAutoGenerate = false,
  regenerateOnAuto = false,
  onAutoGenerateComplete,
  onMeetingUpdated,
  onRefetchTranscripts,
  // Pagination props for efficient transcript loading
  segments,
  hasMore,
  isLoadingMore,
  totalCount,
  loadedCount,
  onLoadMore,
  onSetTranscriptMark,
  onSetTranscriptSpeaker,
  focusSegmentId = null,
  requestedTab = null,
}: {
  meeting: any;
  summaryData: MeetingSummary | null;
  initialSummary: SummaryProcessResponse | null;
  shouldAutoGenerate?: boolean;
  // A continued meeting replaces its summary instead of generating the first one
  regenerateOnAuto?: boolean;
  onAutoGenerateComplete?: () => void;
  onMeetingUpdated?: () => Promise<void>;
  onRefetchTranscripts?: () => Promise<void>;
  // Pagination props
  segments?: any[];
  hasMore?: boolean;
  isLoadingMore?: boolean;
  totalCount?: number;
  loadedCount?: number;
  onLoadMore?: () => void;
  onSetTranscriptMark?: (id: string, mark: TranscriptMark | null) => Promise<void>;
  onSetTranscriptSpeaker?: (id: string, idx: number | null) => void;
  // From a search hit: the transcript line to scroll to, and the summary or notes view to open
  focusSegmentId?: string | null;
  requestedTab?: 'summary' | 'notes' | null;
}) {
  console.log('📄 PAGE CONTENT: Initializing with data:', {
    meetingId: meeting.id,
    summaryDataKeys: summaryData ? Object.keys(summaryData) : null,
    transcriptsCount: meeting.transcripts?.length
  });

  // State
  const [customPrompt, setCustomPrompt] = useState<string>('');
  const isRecording = false;
  const [activeTab, setActiveTab] = useState<MeetingDetailsTab>('transcript');
  const layout = useMemo(readMeetingPageLayout, []);
  const notes = useMeetingNotes(meeting.id);
  const audio = useMeetingAudio(meeting.id);
  const router = useRouter();
  const { isRecording: recordingNow, isStopping, isProcessing, isSaving } = useRecordingState();
  const [notesCollapsed, setNotesCollapsed] = useState(() => readFlag('conclave.notesPaneCollapsed'));
  const toggleNotesCollapsed = () => setNotesCollapsed((c) => { writeFlag('conclave.notesPaneCollapsed', !c); return !c; });
  const isDesktop = useIsDesktop();
  const notesStripShown = notesCollapsedOnScreen(layout, notesCollapsed, isDesktop);

  // Ref to store the modal open function from SummaryGeneratorButtonGroup
  const openModelSettingsRef = useRef<(() => void) | null>(null);
  const autoSwitchedSummaryMeetingIdsRef = useRef(new Set<string>());
  const manuallySelectedTabMeetingIdsRef = useRef(new Set<string>());
  const autoGenerationStartedMeetingIdRef = useRef<string | null>(null);

  // Sidebar context
  const { serverAddress } = useSidebar();

  // Get model config from ConfigContext
  const { modelConfig, setModelConfig, isModelConfigLoading, speakerIdActive, speakerSettingsLoaded } = useConfig();
  // Speaker analysis: the summary waits for it, but a settings lookup that never answers must not block it
  const [settingsTimedOut, setSettingsTimedOut] = useState(false);
  const [summarizeNowId, setSummarizeNowId] = useState<string | null>(null);
  useEffect(() => {
    if (speakerSettingsLoaded) return;
    const timer = setTimeout(() => setSettingsTimedOut(true), 5000);
    return () => clearTimeout(timer);
  }, [speakerSettingsLoaded]);

  // Custom hooks
  const meetingData = useMeetingData({ meeting, summaryData, onMeetingUpdated });
  const templates = useTemplates();
  // One instance for the page: the transcript bar renames through it and the summary panel shows its names-changed hint
  const speakers = useMeetingSpeakers(meeting.id, speakerIdActive, onRefetchTranscripts, onSetTranscriptSpeaker);
  // The timeout only opens the gate while the settings are unanswered; names follow the settings as they arrive
  const speakersOn = speakerIdActive;
  const summarizeNow = summarizeNowId === meeting.id;
  const speakerGateOpen = summarizeNow
    || (speakerSettingsLoaded ? !speakersOn || (speakers.loaded && !speakers.busy) : settingsTimedOut);
  // Undefined while off, so the summary text and Copy are exactly as before
  const speakerName = useMemo(() => {
    if (!speakersOn) return undefined;
    const list = speakers.data?.speakers ?? [];
    // Published speakers stay in place after a failed re-identify, so they name the lines too
    const analysed = list.length > 0;
    // Before analysis a line without a live label stays unnamed, as in the transcript
    return (t: Transcript) => (t.speaker_idx == null && !analysed ? undefined : lineSpeakerName(t.speaker_idx, list, analysed));
  }, [speakersOn, speakers.data]);

  // Any new summary takes the current names, so the names-changed hint no longer applies
  const generateSummary = (customPrompt: string) => {
    speakers.clearNamesChanged();
    return summaryGeneration.handleGenerateSummary(customPrompt);
  };
  const regenerateSummary = (templateId?: string) => {
    speakers.clearNamesChanged();
    return summaryGeneration.handleRegenerateSummary(templateId);
  };

  // Callback to register the modal open function
  const handleRegisterModalOpen = (openFn: () => void) => {
    console.log('📝 Registering modal open function in PageContent');
    openModelSettingsRef.current = openFn;
  };

  // Callback to trigger modal open (called from error handler)
  const handleOpenModelSettings = () => {
    console.log('🔔 Opening model settings from PageContent');
    if (openModelSettingsRef.current) {
      openModelSettingsRef.current();
    } else {
      console.warn('⚠️ Modal open function not yet registered');
    }
  };

  // Save model config to backend database and sync via event
  const handleSaveModelConfig = async (config?: ModelConfig) => {
    if (!config) return;
    try {
      await invoke('api_save_model_config', {
        provider: config.provider,
        model: config.model,
        whisperModel: config.whisperModel,
        apiKey: config.apiKey ?? null,
        ollamaEndpoint: config.ollamaEndpoint ?? null,
      });

      // Emit event so ConfigContext and other listeners stay in sync
      const { emit } = await import('@tauri-apps/api/event');
      await emit('model-config-updated', config);

      toast.success('Model settings saved successfully');
    } catch (error) {
      console.error('Failed to save model config:', error);
      toast.error('Failed to save model settings');
    }
  };

  const summaryGeneration = useSummaryGeneration({
    initialSummary,
    meeting,
    transcripts: meetingData.transcripts,
    modelConfig: modelConfig,
    isModelConfigLoading,
    selectedTemplate: templates.selectedTemplate,
    onMeetingUpdated,
    updateMeetingTitle: meetingData.updateMeetingTitle,
    setAiSummary: meetingData.setAiSummary,
    onOpenModelSettings: handleOpenModelSettings,
    speakerName,
  });

  const summaryMetadata = readSummaryMetadata(meetingData.aiSummary);
  const currentTemplate = summaryMetadata.templateId && summaryMetadata.templateName
    ? { id: summaryMetadata.templateId, name: summaryMetadata.templateName, source: summaryMetadata.templateSource }
    : null;

  const copyOperations = useCopyOperations({
    meeting,
    transcripts: meetingData.transcripts,
    meetingTitle: meetingData.meetingTitle,
    aiSummary: meetingData.aiSummary,
    blockNoteSummaryRef: meetingData.blockNoteSummaryRef,
    speakerName,
  });

  const meetingOperations = useMeetingOperations({
    meeting,
  });

  // A link to a line or a view opens it. Latching the tab as chosen keeps the summary auto-switch from overriding it.
  useEffect(() => {
    if (!focusSegmentId && !requestedTab) return;
    manuallySelectedTabMeetingIdsRef.current.add(meeting.id);
    if (requestedTab === 'notes') setNotesCollapsed(false);
    setActiveTab(requestedTab ?? 'transcript');
  }, [meeting.id, focusSegmentId, requestedTab]);

  useEffect(() => {
    if (
      (meetingData.aiSummary || summaryGeneration.summaryStatus === 'completed')
      && !autoSwitchedSummaryMeetingIdsRef.current.has(meeting.id)
      && !manuallySelectedTabMeetingIdsRef.current.has(meeting.id)
    ) {
      autoSwitchedSummaryMeetingIdsRef.current.add(meeting.id);
      setActiveTab('summary');
    }
  }, [meeting.id, meetingData.aiSummary, summaryGeneration.summaryStatus]);

  // Auto-generate only after the model configuration and the speaker analysis have settled.
  // The latch below is one-way, so a start before the analysis answers could not be undone.
  const speakerWaiting = shouldAutoGenerate
    && summaryGeneration.summaryStatus === 'idle'
    && meetingData.transcripts.length > 0
    && !speakerGateOpen;
  const replaceOnAuto = regenerateOnAuto
    && (summaryGeneration.summaryStatus === 'completed' || summaryGeneration.summaryStatus === 'error');
  useEffect(() => {
    if (
      !shouldAutoGenerate
      || (summaryGeneration.summaryStatus !== 'idle' && !replaceOnAuto)
      || isModelConfigLoading
      || meetingData.transcripts.length === 0
      || !speakerGateOpen
      || autoGenerationStartedMeetingIdRef.current === meeting.id
    ) {
      return;
    }

    autoGenerationStartedMeetingIdRef.current = meeting.id;
    console.log(`🤖 Auto-generating summary with ${modelConfig.provider}/${modelConfig.model}...`);
    onAutoGenerateComplete?.();
    speakers.clearNamesChanged();
    if (replaceOnAuto) void regenerateSummary();
    else void summaryGeneration.handleGenerateSummary('');
  }, [
    shouldAutoGenerate,
    replaceOnAuto,
    meeting.id,
    meetingData.transcripts.length,
    isModelConfigLoading,
    modelConfig.provider,
    modelConfig.model,
    summaryGeneration.handleGenerateSummary,
    summaryGeneration.summaryStatus,
    speakerGateOpen,
    onAutoGenerateComplete,
  ]);

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
      className="flex flex-col h-screen min-w-0 bg-muted/50"
    >
      <div className="flex flex-1 min-w-0 overflow-hidden">
        <MeetingDetailsSplitView
          activeTab={activeTab}
          onTabChange={(tab) => {
            manuallySelectedTabMeetingIdsRef.current.add(meeting.id);
            setActiveTab(tab);
          }}
          layout={layout}
          notesCount={notes.notes.length}
          notesCollapsed={notesStripShown}
          notes={
            <NotesPane
              disabled={!notes.loaded}
              disabledHint="Loading notes…"
              notes={notes.notes}
              onAdd={notes.addNote}
              onUpdate={notes.updateNote}
              onDelete={notes.deleteNote}
              onTimeClick={audio.info?.path ? audio.playFrom : undefined}
              collapsed={notesStripShown}
              onToggleCollapsed={toggleNotesCollapsed}
            />
          }
          transcript={
            <TranscriptPanel
              transcripts={meetingData.transcripts}
              customPrompt={customPrompt}
              onPromptChange={setCustomPrompt}
              onCopyTranscript={copyOperations.handleCopyTranscript}
              onOpenMeetingFolder={meetingOperations.handleOpenMeetingFolder}
              isRecording={isRecording}
              disableAutoScroll={true}
              usePagination={true}
              segments={segments}
              hasMore={hasMore}
              isLoadingMore={isLoadingMore}
              totalCount={totalCount}
              loadedCount={loadedCount}
              onLoadMore={onLoadMore}
              meetingId={meeting.id}
              meetingFolderPath={meeting.folder_path}
              onRefetchTranscripts={onRefetchTranscripts}
              audioMissingReason={audioMissingMessage(audio.info)}
              onMarkChange={onSetTranscriptMark}
              onPlayFrom={audio.info?.path ? audio.playFrom : undefined}
              onResumeRecording={audio.info?.resumable && !recordingNow && !isStopping && !isProcessing && !isSaving ? () => requestResume(meeting.id, router.push) : undefined}
              playheadSeconds={audio.playhead}
              focusSegmentId={focusSegmentId}
              footer={<AudioPlayerBar audio={audio} />}
              speakers={speakers}
            />
          }
          summary={
            <SummaryPanel
              meeting={meeting}
              meetingTitle={meetingData.meetingTitle}
              summaryRef={meetingData.blockNoteSummaryRef}
              isSaving={meetingData.isSaving}
              isSummaryDirty={meetingData.isSummaryDirty}
              onSaveAll={meetingData.saveAllChanges}
              onCopySummary={copyOperations.handleCopySummary}
              aiSummary={meetingData.aiSummary}
              summaryStatus={summaryGeneration.summaryStatus}
              transcripts={meetingData.transcripts}
              modelConfig={modelConfig}
              setModelConfig={setModelConfig}
              onSaveModelConfig={handleSaveModelConfig}
              onGenerateSummary={generateSummary}
              onStopGeneration={summaryGeneration.handleStopGeneration}
              customPrompt={customPrompt}
              onSaveSummary={meetingData.handleSaveSummary}
              onSummaryChange={meetingData.handleSummaryChange}
              onDirtyChange={meetingData.setIsSummaryDirty}
              summaryError={summaryGeneration.summaryError}
              onRegenerateSummary={regenerateSummary}
              getSummaryStatusMessage={summaryGeneration.getSummaryStatusMessage}
              availableTemplates={templates.availableTemplates}
              selectedTemplate={templates.selectedTemplate}
              currentTemplate={currentTemplate}
              languageFallback={summaryMetadata.languageFallback}
              onTemplateSelect={(id, name) =>
                pickTemplateIfAllowed(
                  meetingData.isSummaryDirty,
                  () => templates.handleTemplateSelection(id, name),
                  () => regenerateSummary(id),
                )
              }
              isModelConfigLoading={isModelConfigLoading}
              onOpenModelSettings={handleRegisterModalOpen}
              speakerWaiting={speakerWaiting}
              onSummarizeNow={() => setSummarizeNowId(meeting.id)}
              namesChanged={speakers.namesChanged}
            />
          }
        />
        {/* One element for the whole page, outside every tab, so switching tabs keeps playback. */}
        {audio.src && <audio ref={audio.audioRef} src={audio.src} preload="metadata" className="hidden" {...audio.audioEvents} />}
      </div>
    </motion.div>
  );
}
