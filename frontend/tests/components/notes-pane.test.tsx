import { describe, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';
import { NotesPane } from '../../src/components/MeetingNotes/NotesPane';

const notes = [
  { id: 'n2', at: null, text: 'after' },
  { id: 'n1', at: 122, text: 'ask about Q2 budget' },
];

function render(extra: Partial<Parameters<typeof NotesPane>[0]> = {}) {
  const props = {
    notes, onAdd: mock(() => {}), onUpdate: mock(() => {}), onDelete: mock(() => {}),
    collapsed: false, onToggleCollapsed: mock(() => {}), ...extra,
  };
  let r!: ReturnType<typeof create>;
  act(() => { r = create(<NotesPane {...props} />); });
  return { r, props };
}

describe('NotesPane', () => {
  test('lists notes sorted, with times and "added later"', () => {
    const { r } = render();
    const text = JSON.stringify(r.toJSON());
    expect(text.indexOf('ask about Q2 budget')).toBeLessThan(text.indexOf('after'));
    expect(text).toContain('02:02');
    expect(text).toContain('added later');
  });

  test('Enter adds a trimmed note, Shift+Enter does not, empty is ignored', () => {
    const { r, props } = render();
    const input = r.root.findByProps({ 'aria-label': 'New note' });
    act(() => input.props.onChange({ target: { value: '  new idea ' } }));
    act(() => r.root.findByProps({ 'aria-label': 'New note' }).props.onKeyDown({ key: 'Enter', shiftKey: true, preventDefault() {} }));
    expect(props.onAdd).not.toHaveBeenCalled();
    act(() => r.root.findByProps({ 'aria-label': 'New note' }).props.onKeyDown({ key: 'Enter', shiftKey: false, preventDefault() {} }));
    expect(props.onAdd).toHaveBeenCalledWith('new idea');
    act(() => r.root.findByProps({ 'aria-label': 'New note' }).props.onKeyDown({ key: 'Enter', shiftKey: false, preventDefault() {} }));
    expect(props.onAdd).toHaveBeenCalledTimes(1);
  });

  test('delete and time click', () => {
    const onTimeClick = mock(() => {});
    const { r, props } = render({ onTimeClick });
    act(() => r.root.findByProps({ 'aria-label': 'Delete note: ask about Q2 budget' }).props.onClick());
    expect(props.onDelete).toHaveBeenCalledWith('n1');
    act(() => r.root.findByProps({ 'aria-label': 'Play from 02:02' }).props.onClick());
    expect(onTimeClick).toHaveBeenCalledWith(122);
  });

  test('collapsed shows only the expand button; disabled shows the hint', () => {
    const { r } = render({ collapsed: true });
    expect(r.root.findAllByProps({ 'aria-label': 'New note' }).length).toBe(0);
    r.root.findByProps({ 'aria-label': 'Show notes' });
    const d = render({ disabled: true, disabledHint: 'Notes start with a recording' });
    expect(d.r.root.findByProps({ 'aria-label': 'New note' }).props.disabled).toBe(true);
    expect(JSON.stringify(d.r.toJSON())).toContain('Notes start with a recording');
  });
});
