import { useEffect, useRef } from 'react';

const TYPING = 'input,textarea,select,[contenteditable]:not([contenteditable="false"])';
// An open dialog, menu, listbox or the AI sidebar owns Esc; the first press closes it, the next goes back.
const LAYERS = '[role="dialog"],[role="alertdialog"],[role="menu"],[role="listbox"],[data-esc-layer]';

export function isTypingTarget(t: EventTarget | null): boolean {
  return t instanceof Element && t.closest(TYPING) !== null;
}

/** Esc or Alt+Left calls `onBack`, never while typing, in a dialog or menu, or when something already handled the key. */
export function useBackShortcut(onBack: () => void, enabled = true): void {
  const ref = useRef(onBack);
  ref.current = onBack;
  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      const esc = e.key === 'Escape' && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey;
      const alt = e.key === 'ArrowLeft' && e.altKey && !e.ctrlKey && !e.shiftKey && !e.metaKey;
      if ((!esc && !alt) || e.repeat || e.defaultPrevented || isTypingTarget(e.target) || document.querySelector(LAYERS)) return;
      e.preventDefault(); // WebView2 would also navigate on Alt+Left
      ref.current();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [enabled]);
}
