import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

// The model-config-updated payload carries apiKey, so no listener may log it whole.
// ConfigContext is also covered by a behaviour test; this guards the other listeners.
const LISTENERS = [
  'src/contexts/ConfigContext.tsx',
  'src/components/Sidebar/index.tsx',
  'src/components/SummaryModelSettings.tsx',
  'src/hooks/meeting-details/useModelConfiguration.ts',
];
const WHOLE_PAYLOAD_LOG = /model-config-updated event:['"]?,?\s*event\.payload\s*\)/;

describe('model-config-updated listeners', () => {
  for (const file of LISTENERS) {
    test(`${file} logs only provider and model`, () => {
      const source = readFileSync(join(import.meta.dir, '..', '..', file), 'utf8');
      expect(source).toContain('model-config-updated');
      expect(WHOLE_PAYLOAD_LOG.test(source)).toBe(false);
    });
  }
});
