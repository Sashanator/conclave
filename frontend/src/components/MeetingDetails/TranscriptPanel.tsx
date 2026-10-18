"use client";

import { Transcript, TranscriptMark, TranscriptSegmentData } from '@/types';
import { TranscriptView } from '@/components/TranscriptView';
import { VirtualizedTranscriptView } from '@/components/VirtualizedTranscriptView';
import { TranscriptButtonGroup } from './TranscriptButtonGroup';
import { SpeakersBar } from './SpeakersBar';
import { SpeakerLineMenu } from './SpeakerLineMenu';
import type { MeetingSpeakersState } from '@/hooks/useMeetingSpeakers';
import { useConfig } from '@/contexts/ConfigContext';
import { lineSpeakerName } from '@/lib/speakers';
import { useMemo, type ReactNode } from 'react';

interface TranscriptPanelProps {
  transcripts: Transcript[];
  customPrompt: string;
  onPromptChange: (value: string) => void;
  onCopyTranscript: () => void;
  onOpenMeetingFolder: () => Promise<void>;
  isRecording: boolean;
  disableAutoScroll?: boolean;

  // Optional pagination props (when using virtualization)
  usePagination?: boolean;
  segments?: TranscriptSegmentData[];
  hasMore?: boolean;
  isLoadingMore?: boolean;
  totalCount?: number;
  loadedCount?: number;
  onLoadMore?: () => void;

  // Retranscription props
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;
  audioMissingReason?: string | null;

  // Marks and playback (saved meeting page)
  onMarkChange?: (segmentId: string, mark: TranscriptMark | null) => void;
  onPlayFrom?: (seconds: number) => void;
  playheadSeconds?: number | null;
  footer?: ReactNode;
  // The page owns the speaker state so the summary can wait on it and show the names-changed hint
  speakers: MeetingSpeakersState;
}

export function TranscriptPanel({
  transcripts,
  customPrompt,
  onPromptChange,
  onCopyTranscript,
  onOpenMeetingFolder,
  isRecording,
  disableAutoScroll = false,
  usePagination = false,
  segments,
  hasMore,
  isLoadingMore,
  totalCount,
  loadedCount,
  onLoadMore,
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
  audioMissingReason,
  onMarkChange,
  onPlayFrom,
  playheadSeconds,
  footer,
  speakers,
}: TranscriptPanelProps) {
  const { speakerIdActive } = useConfig();
  const speakersOn = speakerIdActive && !!meetingId;

  // Convert transcripts to segments if pagination is not used but we want virtualization
  const convertedSegments = useMemo(() => {
    if (usePagination && segments) {
      return segments;
    }
    // Convert transcripts to segments for virtualization
    return transcripts.map(t => ({
      id: t.id,
      timestamp: t.audio_start_time ?? 0,
      endTime: t.audio_end_time,
      text: t.text,
      confidence: t.confidence,
      mark: t.mark,
      speaker_idx: t.speaker_idx,
    }));
  }, [transcripts, usePagination, segments]);

  // Feature off: no label node, so rows render exactly as before
  const renderSpeakerLabel = useMemo(() => {
    if (!speakersOn) return undefined;
    const list = speakers.data?.speakers ?? [];
    return (s: TranscriptSegmentData) => {
      if (list.length === 0) {
        // Before analysis only the live label exists, and only for lines that have one
        if (s.speaker_idx == null) return null;
        const name = lineSpeakerName(s.speaker_idx, [], false);
        return <span data-speaker-label={name} className="text-xs text-muted-foreground mt-1 flex-shrink-0">{name}</span>;
      }
      return (
        <SpeakerLineMenu
          idx={s.speaker_idx}
          speakers={list}
          disabled={speakers.busy}
          onMove={(idx) => void speakers.moveLine(s.id, idx)}
        />
      );
    };
  }, [speakersOn, speakers.data, speakers.busy, speakers.moveLine]);

  return (
    <div className="flex h-full min-w-0 w-full bg-background flex-col relative @container">
      {/* Title area */}
      <div className="p-4 border-b border-border">
        <TranscriptButtonGroup
          transcriptCount={usePagination ? (totalCount ?? convertedSegments.length) : (transcripts?.length || 0)}
          onCopyTranscript={onCopyTranscript}
          onOpenMeetingFolder={onOpenMeetingFolder}
          meetingId={meetingId}
          meetingFolderPath={meetingFolderPath}
          onRefetchTranscripts={onRefetchTranscripts}
          audioMissingReason={audioMissingReason}
          speakerJob={speakers.data?.job ?? null}
          hasSpeakers={(speakers.data?.speakers.length ?? 0) > 0}
          speakersBusy={speakers.busy}
          onIdentifySpeakers={speakers.identify}
        />
      </div>

      {speakersOn && (
        <SpeakersBar
          data={speakers.data}
          busy={speakers.busy}
          percent={speakers.percent}
          onName={speakers.name}
          onIdentify={speakers.identify}
        />
      )}

      {/* Transcript content - use virtualized view for better performance */}
      <div className="flex-1 overflow-hidden pb-4">
        <VirtualizedTranscriptView
          segments={convertedSegments}
          isRecording={isRecording}
          isPaused={false}
          isProcessing={false}
          isStopping={false}
          enableStreaming={false}
          showConfidence={true}
          disableAutoScroll={disableAutoScroll}
          hasMore={hasMore}
          isLoadingMore={isLoadingMore}
          totalCount={totalCount}
          loadedCount={loadedCount}
          onLoadMore={onLoadMore}
          onMarkChange={onMarkChange}
          onPlayFrom={onPlayFrom}
          playheadSeconds={playheadSeconds}
          renderSpeakerLabel={renderSpeakerLabel}
        />
      </div>

      {footer}

      {/* Custom prompt input at bottom of transcript section */}
      {!isRecording && convertedSegments.length > 0 && (
        <div className="p-1 border-t border-border">
          <textarea
            placeholder="Add context for AI summary. For example people involved, meeting overview, objective etc..."
            className="w-full px-3 py-2 border border-input rounded-md text-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 bg-background shadow-sm min-h-[80px] resize-y"
            value={customPrompt}
            onChange={(e) => onPromptChange(e.target.value)}
          />
        </div>
      )}
    </div>
  );
}
