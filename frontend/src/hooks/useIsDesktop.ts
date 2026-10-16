'use client';
import { useEffect, useState } from 'react';

/** True at 768px and wider (the md breakpoint); starts true so the desktop layout renders first. */
export function useIsDesktop(): boolean {
  const [isDesktop, setIsDesktop] = useState(true);

  useEffect(() => {
    const mediaQuery = window.matchMedia('(min-width: 768px)');
    const updateLayout = () => setIsDesktop(mediaQuery.matches);
    updateLayout();
    mediaQuery.addEventListener('change', updateLayout);
    return () => mediaQuery.removeEventListener('change', updateLayout);
  }, []);

  return isDesktop;
}
