interface StatusOverlaysProps {
  // Status flags
  isProcessing: boolean;      // Processing transcription after recording stops
  isSaving: boolean;          // Saving transcript to database
}

// Internal reusable component for individual status overlays
interface StatusOverlayProps {
  show: boolean;
  message: string;
}

function StatusOverlay({ show, message }: StatusOverlayProps) {
  if (!show) return null;

  return (
    <div className="absolute bottom-4 left-0 right-0 z-10">
      <div className="flex justify-center">
        <div className="w-2/3 max-w-[750px] flex justify-center">
          <div className="bg-card rounded-lg shadow-lg px-4 py-2 flex items-center space-x-2">
            <div className="animate-spin rounded-full h-4 w-4 border-b-2 border-foreground"></div>
            <span className="text-sm text-foreground">{message}</span>
          </div>
        </div>
      </div>
    </div>
  );
}

// Main exported component - renders multiple status overlays
export function StatusOverlays({
  isProcessing,
  isSaving
}: StatusOverlaysProps) {
  return (
    <>
      {/* Processing status overlay - shown after recording stops while finalizing transcription */}
      <StatusOverlay
        show={isProcessing}
        message="Finalizing transcription..."
      />

      {/* Saving status overlay - shown while saving transcript to database */}
      <StatusOverlay
        show={isSaving}
        message="Saving transcript..."
      />
    </>
  );
}
