export type TableKeyAction =
  | 'up'
  | 'down'
  | 'open'
  | 'toggle-reviewed'
  | 'focus-search'
  | 'new-meeting'
  | 'clear-search';

export type TableKeyEvent = {
  key: string;
  code?: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
};

export type TableKeyContext = {
  inEditable: boolean;
  searching: boolean;
};

export function tableKeyAction(e: TableKeyEvent, ctx: TableKeyContext): TableKeyAction | null {
  const isCtrl = Boolean(e.ctrlKey || e.metaKey);

  if (isCtrl) {
    if (e.shiftKey || e.altKey) {
      return null;
    }
    if (e.key === 'n' || e.key === 'N' || e.code === 'KeyN') {
      return 'new-meeting';
    }
    return null;
  }

  if (e.altKey) {
    return null;
  }

  if (e.key === 'Escape' || e.key === 'Esc') {
    return ctx.searching ? 'clear-search' : null;
  }

  if (ctx.inEditable) {
    return null;
  }

  if (e.shiftKey && e.key !== '/') {
    return null;
  }

  switch (e.key) {
    case 'ArrowDown':
      return 'down';
    case 'ArrowUp':
      return 'up';
    case 'Enter':
      return 'open';
    case ' ':
      return 'toggle-reviewed';
    case '/':
      return 'focus-search';
    default:
      return null;
  }
}

export function moveSelection(index: number, delta: number, count: number): number {
  if (count <= 0) return -1;
  return Math.max(0, Math.min(count - 1, index + delta));
}
