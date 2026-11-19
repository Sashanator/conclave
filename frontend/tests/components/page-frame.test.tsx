import { describe, expect, test } from 'bun:test';
import { create, type ReactTestInstance } from 'react-test-renderer';
import { PageFrame } from '../../src/components/PageFrame/PageFrame';

type Variant = 'list' | 'reading' | 'dashboard' | 'form';
const VARIANTS: Variant[] = ['list', 'reading', 'dashboard', 'form'];

const render = (variant: Variant, extra: Record<string, React.ReactNode> = {}) =>
  create(<PageFrame variant={variant} title="Stats" {...extra}><i id="kid">x</i></PageFrame>).root;
const cls = (n: ReactTestInstance): string => (typeof n.props.className === 'string' ? n.props.className : '');
const withClass = (r: ReactTestInstance, c: string) => r.findAll((n) => typeof n.type === 'string' && cls(n).includes(c));
const first = (r: ReactTestInstance, c: string) => withClass(r, c)[0];
const dom = (n: ReactTestInstance): ReactTestInstance => n.parent!.type === 'div' ? n.parent! : dom(n.parent!);

describe('PageFrame', () => {
  test('no frame is centred', () => {
    for (const v of VARIANTS) {
      for (const extra of [{}, { subnav: <b>s</b> }, { rail: <b>r</b> }, { subnav: <b>s</b>, rail: <b>r</b> }]) {
        expect(withClass(render(v, extra), 'mx-auto')).toHaveLength(0);
      }
    }
  });

  test('one h1 style, breadcrumb before it, actions in the same row', () => {
    const r = render('list', { breadcrumb: <u id="crumb">c</u>, actions: <s id="act">a</s> });
    const h1 = r.findByType('h1');
    expect(h1.children.join('')).toBe('Stats');
    expect(cls(h1)).toContain('text-xl');
    expect(cls(h1)).toContain('font-semibold');
    const order = JSON.stringify(create(<PageFrame variant="list" title="Stats" breadcrumb={<u>c</u>}>x</PageFrame>).toJSON());
    expect(order.indexOf('"u"')).toBeLessThan(order.indexOf('"h1"'));
    expect(dom(r.findByProps({ id: 'act' })).parent).toBe(h1.parent);
  });

  test('list gives children a bounded flex area', () => {
    const r = render('list');
    const root = r.findAll((n) => typeof n.type === 'string')[0];
    expect(cls(root)).toContain('h-full');
    expect(cls(root)).toContain('flex-col');
    expect(cls(root)).not.toContain('overflow-y-auto');
    const parent = cls(r.findByProps({ id: 'kid' }).parent!);
    expect(parent).toContain('flex-1');
    expect(parent).toContain('min-h-0');
  });

  test('reading is bounded and its columns scroll separately', () => {
    const r = render('reading', { rail: <b id="rail">r</b> });
    expect(cls(r.findAll((n) => typeof n.type === 'string')[0])).not.toContain('overflow-y-auto');
    const grid = cls(first(r, '@[1280px]/frame:grid-cols-[minmax(0,80ch)_minmax(360px,560px)]'));
    expect(grid).toContain('gap-10');
    expect(grid).toContain('min-h-0');
    const text = cls(r.findByProps({ id: 'kid' }).parent!);
    for (const c of ['max-w-[80ch]', 'min-h-0', 'overflow-y-auto']) expect(text).toContain(c);
    const rail = r.findByProps({ id: 'rail' }).parent!;
    expect(cls(rail)).toContain('overflow-y-auto');
    expect(rail.parent!.children.indexOf(rail)).toBeGreaterThan(rail.parent!.children.indexOf(r.findByProps({ id: 'kid' }).parent!));
  });

  test('reading with subnav: list | text | rail', () => {
    const r = render('reading', { subnav: <b id="sub">s</b>, rail: <b>r</b> });
    const wrap = r.findByProps({ id: 'sub' }).parent!;
    expect(cls(wrap)).toContain('overflow-y-auto');
    const row = wrap.parent!;
    expect(cls(row)).toContain('@[1280px]/frame:flex-row');
    expect(row.children[0]).toBe(wrap);
    expect(cls(row.children[1] as ReactTestInstance)).toContain('grid-cols-[minmax(0,80ch)');
  });

  test('list with subnav keeps the full width', () => {
    const r = render('list', { subnav: <b id="sub">s</b> });
    const wrap = r.findByProps({ id: 'sub' }).parent!;
    expect(cls(wrap)).toContain('w-[220px]');
    expect(cls(wrap.parent!)).toContain('min-h-0');
    expect(cls(wrap.parent!)).toContain('flex-1');
    expect(wrap.parent!.children[0]).toBe(wrap);
    expect(withClass(r, 'max-w-[880px]')).toHaveLength(0);
  });

  test('form with subnav caps only the form column', () => {
    const r = render('form', { subnav: <b id="sub">s</b> });
    const wrap = r.findByProps({ id: 'sub' }).parent!;
    expect(cls(wrap)).toContain('w-[220px]');
    expect(cls(wrap)).not.toContain('max-w-[880px]');
    expect(cls(wrap.parent!)).toContain('flex gap-8');
    expect(wrap.parent!.children[0]).toBe(wrap);
    expect(cls(r.findByProps({ id: 'kid' }).parent!)).toContain('max-w-[880px]');
  });

  test('dashboard caps at 1600 and imposes no grid', () => {
    const r = render('dashboard', { subnav: <b>s</b> });
    expect(withClass(r, 'max-w-[1600px]').length).toBeGreaterThan(0);
    const parent = cls(r.findByProps({ id: 'kid' }).parent!);
    expect(parent).not.toMatch(/\bgrid\b|grid-cols-/);
    expect(withClass(r, 'w-[220px]')).toHaveLength(0);
  });

  test('smoke: a breadcrumb and a title render', () => {
    const r = create(<PageFrame variant="reading" breadcrumb={<a href="/">Meetings</a>} title="T">x</PageFrame>);
    expect(JSON.stringify(r.toJSON())).toContain('Meetings');
  });

  test('container-query classes are written literally so Tailwind generates them', () => {
    const src = require('fs').readFileSync(require('path').join(import.meta.dir, '../../src/components/PageFrame/PageFrame.tsx'), 'utf8');
    expect(src).not.toContain('${wide}');
    for (const c of ['@[1280px]/frame:grid-cols-[minmax(0,80ch)_minmax(360px,560px)]', '@[1280px]/frame:grid-rows-1', '@[1280px]/frame:flex-row', '@[1280px]/frame:max-h-none', '@[1280px]/frame:w-[300px]']) expect(src).toContain(c);
  });
});
