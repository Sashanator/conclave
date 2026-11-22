import { expect, test } from 'bun:test';
import { create } from 'react-test-renderer';
import { DestPicker } from '../../src/components/Tasks/DestPicker';
import type { Destination } from '../../src/lib/notion';
import type { TaskRow } from '../../src/lib/tasks';

const dest = (id: string, position: number, over: Partial<Destination> = {}): Destination => ({
  id, name: id, colour: position + 1, position, databaseId: id, dataSourceId: null,
  mapping: { titleProp: 'Name', dueProp: null, meetingProp: null, fixed: [] }, lastStatus: null, lastMessage: null, ...over,
});
const task = (destId: string | null) => ({ id: 't1', text: 'Send the deck', destId }) as TaskRow;
const render = (t: TaskRow, dests: Destination[], onPick = (_: string | null) => {}) => create(<DestPicker task={t} dests={dests} onPick={onPick} />).root;

test('no destination: Choose database… selected, no dot', () => {
  const r = render(task(null), [dest('a', 0)]);
  const sel = r.findByType('select');
  expect(sel.props.value).toBe('');
  expect(sel.findAllByType('option')[0].children).toEqual(['Choose database…']);
  expect(r.findAllByType('i')).toHaveLength(0);
});

test('a deleted destination counts as none', () => {
  expect(render(task('gone'), [dest('a', 0)]).findByType('select').props.value).toBe('');
});

test('a chosen destination shows its dot; a blocked one is amber', () => {
  const ok = render(task('a'), [dest('a', 0)]);
  expect(ok.findByType('i').props['aria-hidden']).toBe('true');
  expect(ok.findByType('i').props.style.background).toBe('hsl(var(--db-1))');
  expect(ok.findByType('select').props.className).not.toContain('amber');
  const bad = render(task('a'), [dest('a', 0, { lastStatus: 'not_shared' })]);
  expect(bad.findByType('select').props.className).toContain('amber');
});

test('choosing the empty option reports null; a destination reports its id', () => {
  const got: (string | null)[] = [];
  const r = render(task('a'), [dest('a', 0), dest('b', 1)], (d) => got.push(d));
  r.findByType('select').props.onChange({ target: { value: '' } });
  r.findByType('select').props.onChange({ target: { value: 'b' } });
  expect(got).toEqual([null, 'b']);
});
