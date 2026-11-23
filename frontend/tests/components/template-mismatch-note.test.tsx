import { expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';
import { TemplateMismatchNote, templateMismatch } from '../../src/components/MeetingDetails/TemplateMismatchNote';

const templates = [{ id: 'a', name: 'Standup' }, { id: 'b', name: 'Interview' }];
const base = { shown: { id: 'a', name: 'Standup' }, currentId: 'b', templates, status: 'completed', autoPending: false };

test('shown when the summary template differs from the meeting category', () => {
  expect(templateMismatch(base)).toEqual({ shown: 'Standup', current: templates[1] });
});

test.each([
  ['templates match', { currentId: 'a' }],
  ['no summary', { shown: null }],
  ['no category', { currentId: null }],
  ['a summary is running', { status: 'summarizing' }],
  ['regenerating', { status: 'regenerating' }],
  ['an automatic summary has not started', { autoPending: true }],
  ['the category is unknown', { currentId: 'gone' }],
])('hidden when %s', (_name, patch) => {
  expect(templateMismatch({ ...base, ...patch })).toBeNull();
});

test('the button regenerates with the current template, and nothing runs before the click', async () => {
  const onRegenerate = mock((_id: string) => {});
  const r = create(<TemplateMismatchNote mismatch={templateMismatch(base)!} onRegenerate={onRegenerate} />);
  expect(r.root.findByType('span').children.join('')).toBe('Made with Standup. This meeting is now Interview.');
  expect(onRegenerate).not.toHaveBeenCalled();
  await act(async () => { r.root.findByType('button').props.onClick(); });
  expect(onRegenerate).toHaveBeenCalledWith('b');
});
