import { useEffect, useReducer } from 'react';
import { listen } from '@tauri-apps/api/event';
import { initialMicFollowing, reduceMicFollowing } from '@/lib/mic-following';

/** Mic following state for the recording screen: which mic is live, and any warning. */
export function useMicFollowing() {
  const [state, dispatch] = useReducer(reduceMicFollowing, initialMicFollowing);

  useEffect(() => {
    const subs = [
      listen<{ device: string; app: string }>('mic-following', (e) =>
        dispatch({ type: 'following', device: e.payload.device, app: e.payload.app })),
      listen<{ device_name: string }>('mic-device-switched', (e) =>
        dispatch({ type: 'switched', device: e.payload.device_name })),
      listen<{ message: string }>('mic-following-warning', (e) =>
        dispatch({ type: 'warning', message: e.payload.message })),
      listen('recording-started', () => dispatch({ type: 'reset' })),
    ];
    return () => {
      subs.forEach((sub) => sub.then((unlisten) => unlisten()));
    };
  }, []);

  return state;
}
