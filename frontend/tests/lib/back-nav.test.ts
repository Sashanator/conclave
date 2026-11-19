import { beforeEach, describe, expect, mock, spyOn, test } from 'bun:test';
import { goBack, readOrigin, rememberOrigin } from '../../src/lib/back-nav';
import { setUnsavedChanges } from '../../src/lib/unsaved-guard';

let router: { back: ReturnType<typeof mock>; push: ReturnType<typeof mock> };
const store = (o: unknown) => sessionStorage.setItem('conclave.origin', typeof o === 'string' ? o : JSON.stringify(o));

beforeEach(() => {
  sessionStorage.clear();
  setUnsavedChanges(false);
  router = { back: mock(), push: mock() };
});

describe('back-nav', () => {
  test('cold start with no origin goes to the fallback', async () => {
    await goBack(router, '/');
    expect(router.push).toHaveBeenCalledWith('/');
    expect(router.back).not.toHaveBeenCalled();
  });

  test('deep link with a stale origin goes to the fallback', async () => {
    store({ path: '/', label: 'Meetings', len: history.length + 3 });
    await goBack(router, '/');
    expect(router.push).toHaveBeenCalledWith('/');
    expect(router.back).not.toHaveBeenCalled();
  });

  test('the origin as the previous entry goes back', async () => {
    store({ path: '/?from=test', label: 'Meetings', len: history.length - 1 });
    await goBack(router, '/');
    expect(router.back).toHaveBeenCalledTimes(1);
    expect(router.push).not.toHaveBeenCalled();
  });

  test('rememberOrigin stores path, label and history length', () => {
    rememberOrigin({ path: '/tasks', label: 'Tasks' });
    expect(JSON.parse(sessionStorage.getItem('conclave.origin')!)).toEqual({ path: '/tasks', label: 'Tasks', len: history.length });
    expect(readOrigin()).toEqual({ path: '/tasks', label: 'Tasks' });
  });

  test('corrupt or partial storage reads as no origin', async () => {
    for (const bad of ['{', '{"path":"/"}', 'null']) {
      store(bad);
      expect(readOrigin()).toBeNull();
      router = { back: mock(), push: mock() };
      await goBack(router, '/');
      expect(router.push).toHaveBeenCalledWith('/');
      expect(router.back).not.toHaveBeenCalled();
    }
  });

  test('declined unsaved edits stay put', async () => {
    store({ path: '/?from=test', label: 'Meetings', len: history.length - 1 });
    setUnsavedChanges(true);
    const real = window.confirm;
    window.confirm = mock(() => false) as any;
    try {
      await goBack(router, '/');
    } finally {
      window.confirm = real;
    }
    expect(router.back).not.toHaveBeenCalled();
    expect(router.push).not.toHaveBeenCalled();
  });

  test('storage that throws', async () => {
    const spy = spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('blocked'); });
    try {
      await goBack(router, '/x');
    } finally {
      spy.mockRestore();
    }
    expect(router.push).toHaveBeenCalledWith('/x');
  });
});
