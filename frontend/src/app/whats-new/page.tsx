import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { WhatsNew } from '@/components/WhatsNew/WhatsNew';

export default function Page() {
  return <WhatsNew changelog={readFileSync(join(process.cwd(), '..', 'CHANGELOG.md'), 'utf8')} />;
}
