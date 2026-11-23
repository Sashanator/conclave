import { useEffect, useState } from 'react';

// Per-device display preference, in localStorage like the theme. Default ON (the stored value is only ever 'off').
export const SHOW_UNRECORDED_KEY = 'conclave.showUnrecordedCalendar';
const EVENT = 'conclave:showUnrecordedCalendar';

export function readShowUnrecorded(): boolean {
  try { return localStorage.getItem(SHOW_UNRECORDED_KEY) !== 'off'; } catch { return true; }
}

export function writeShowUnrecorded(on: boolean): void {
  try { localStorage.setItem(SHOW_UNRECORDED_KEY, on ? 'on' : 'off'); } catch { /* not saved; the switch still applies until reload */ }
  window.dispatchEvent(new Event(EVENT)); // `storage` fires only in other windows
}

export function useShowUnrecorded(): [boolean, (on: boolean) => void] {
  const [on, setOn] = useState(true); // true first so server and client markup match
  useEffect(() => {
    const sync = () => setOn(readShowUnrecorded());
    sync();
    window.addEventListener(EVENT, sync);
    window.addEventListener('storage', sync);
    return () => { window.removeEventListener(EVENT, sync); window.removeEventListener('storage', sync); };
  }, []);
  return [on, writeShowUnrecorded];
}
