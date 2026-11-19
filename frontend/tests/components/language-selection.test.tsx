import { afterAll, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';

const originals = { config: { ...await import('../../src/contexts/ConfigContext') } };
afterAll(() => { mock.module('../../src/contexts/ConfigContext', () => originals.config); });
mock.module('../../src/contexts/ConfigContext', () => ({ useConfig: () => ({ setSelectedLanguage() {}, allowedLanguages: ['ru'], setAllowedLanguages() {} }) }));
const { LanguageSelection } = await import('../../src/components/LanguageSelection');

const render = async (hideSpoken?: boolean) => {
  let r!: ReturnType<typeof create>;
  await act(async () => { r = create(<LanguageSelection selectedLanguage="auto" onLanguageChange={() => {}} hideSpoken={hideSpoken} />); });
  return JSON.stringify(r.toJSON());
};

test('Auto shows no accuracy warning', async () => {
  expect(await render()).not.toContain('Auto Detect may produce');
});

test('Languages spoken is shown in Settings and hidden with hideSpoken', async () => {
  expect(await render()).toContain('Languages spoken');
  expect(await render(true)).not.toContain('Languages spoken');
});
