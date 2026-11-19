import React, { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';

export function AiKeyStep() {
  const { goNext, setPendingAiKey } = useOnboarding();
  const [isMac, setIsMac] = useState(false);
  const [key, setKey] = useState('');

  useEffect(() => {
    const checkPlatform = async () => {
      try {
        const { platform } = await import('@tauri-apps/plugin-os');
        setIsMac(platform() === 'macos');
      } catch (e) {
        setIsMac(navigator.userAgent.includes('Mac'));
      }
    };
    checkPlatform();
  }, []);

  const save = () => {
    setPendingAiKey(key.trim());
    goNext();
  };
  const skip = () => {
    setPendingAiKey(null);
    goNext();
  };

  return (
    <OnboardingContainer title="Summaries: add your AI key" step={2} totalSteps={isMac ? 4 : 3}>
      <div className="flex flex-col items-center space-y-6">
        <div className="w-full max-w-md space-y-4">
          <p className="text-sm text-muted-foreground">
            Conclave writes summaries with Claude, using your own Anthropic API key. Only the transcript is sent; audio stays on this PC.
          </p>
          <div className="space-y-2">
            <Label htmlFor="onboarding-ai-key">Anthropic API key</Label>
            <Input
              id="onboarding-ai-key"
              type="password"
              autoComplete="off"
              value={key}
              onChange={(e) => setKey(e.target.value)}
              placeholder="sk-ant-..."
            />
          </div>
          <p className="text-xs text-muted-foreground">
            Using OpenAI, Groq, OpenRouter or your own server? Skip this, then choose it in Settings → Models & keys.
          </p>
        </div>
        <div className="w-full max-w-xs space-y-3">
          <Button onClick={save} disabled={!key.trim()} className="w-full h-11 bg-foreground hover:bg-foreground/90 text-background">
            Save and continue
          </Button>
          <Button onClick={skip} variant="ghost" className="w-full">
            Skip for now
          </Button>
        </div>
      </div>
    </OnboardingContainer>
  );
}
