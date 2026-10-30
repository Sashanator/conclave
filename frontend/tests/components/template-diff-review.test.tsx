import { afterEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import { TemplateDiffReview } from '../../src/components/TemplateDiffReview';
import type { TemplateJson } from '../../src/lib/template-draft';

let renderer: ReactTestRenderer;
afterEach(() => act(() => renderer?.unmount()));
const text = (n: ReactTestInstance): string => n.children.map(c => typeof c === 'string' ? c : text(c)).join('');
const button = (label: string) => renderer.root.findAll(n => n.type === 'button' && text(n) === label)[0];
const old: TemplateJson = { name: 'Before', description: 'Keep', sections: [{ title: 'Notes', instruction: 'List notes' }] };

test('empty review and History have no checkboxes; Restore passes next directly', () => {
  const accept = mock((_t: TemplateJson) => {});
  act(() => { renderer = create(<TemplateDiffReview old={old} next={old} selectable={false} acceptLabel="Restore" onAccept={accept} onDiscard={() => {}} />); });
  expect(text(renderer.root)).toContain('No differences');
  expect(text(renderer.root)).toContain('1 unchanged sections');
  expect(renderer.root.findAllByType('input')).toHaveLength(0);
  expect(button('Restore').props.disabled).toBe(false);
  act(() => button('Restore').props.onClick());
  expect(accept).toHaveBeenCalledWith(old);
});

test('selection disables empty acceptance and resets for a new proposal; hidden field changes remain visible', () => {
  const accept = mock((_t: TemplateJson) => {});
  const next = { ...old, sections: [{ ...old.sections[0], format: 'Paragraphs' }] };
  const props = { old, selectable: true, acceptLabel: 'Accept selected', onAccept: accept, onDiscard: () => {} };
  act(() => { renderer = create(<TemplateDiffReview {...props} next={next} />); });
  expect(text(renderer.root)).toContain('Format: Paragraphs');
  expect(renderer.root.findAllByType('ins').map(text).join('')).toContain('Format: Paragraphs');
  act(() => renderer.root.findByType('input').props.onChange({ target: { checked: false } }));
  expect(button('Accept selected').props.disabled).toBe(true);
  const revised = { ...next, description: 'New description' };
  act(() => renderer.update(<TemplateDiffReview {...props} next={revised} />));
  expect(renderer.root.findAllByType('input').every(n => n.props.checked)).toBe(true);
  act(() => button('Accept selected').props.onClick());
  expect(accept).toHaveBeenCalledWith(revised);
});

test('renders notes before amber warnings and diffs, collapsed context, and added removed moved statuses', () => {
  const a = { title: 'A', instruction: 'A text' };
  const b = { title: 'B', instruction: 'B text' };
  const c = { title: 'C', instruction: 'C text' };
  const before = { name: 'Before', context: '1\n2\n3\n4\n5\n6\n7\nold', sections: [a, b, c] };
  const next = { ...before, context: '1\n2\n3\n4\n5\n6\n7\nnew', sections: [b, a] };
  act(() => { renderer = create(<TemplateDiffReview old={before} next={next} selectable={true}
    changes={[{ field: 'context', note: 'Explain' }]} warnings={['Check removals']} acceptLabel="Accept selected"
    onAccept={() => {}} onDiscard={() => {}} />); });
  const content = text(renderer.root);
  expect(content.indexOf('What I changed and why')).toBeLessThan(content.indexOf('Check removals'));
  expect(content.indexOf('Check removals')).toBeLessThan(content.indexOf('Context'));
  expect(content).toContain('unchanged lines');
  expect(content).toContain('moved');
  expect(content).toContain('removed');
  expect(renderer.root.findAllByType('del').map(text).join('')).toContain('old');
});
