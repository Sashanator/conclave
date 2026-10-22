'use client';

import { useCallback, useEffect, useState } from 'react';
import { Bell } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useRouter } from 'next/navigation';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { confirmLeave } from '@/lib/unsaved-guard';

export interface FeedItem {
  id: string;
  kind: string;
  title: string;
  targetId: string;
  createdAt: string;
  read: boolean;
}

export function FeedBell({ side = 'top' }: { side?: 'top' | 'right' }) {
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [unread, setUnread] = useState(0);
  const [items, setItems] = useState<FeedItem[]>([]);

  const refresh = useCallback(async () => {
    try {
      const [count, list] = await Promise.all([
        invoke<number>('feed_unread_count'),
        invoke<FeedItem[]>('feed_list'),
      ]);
      setUnread(count);
      setItems(list);
    } catch (error) {
      console.error('Failed to load notifications:', error);
    }
  }, []);

  useEffect(() => {
    refresh();
    const unlisten = listen('feed-updated', () => {
      refresh();
    });
    return () => {
      unlisten.then((stop) => stop());
    };
  }, [refresh]);

  const openItem = async (item: FeedItem) => {
    if (!confirmLeave()) return;
    setOpen(false);
    try {
      await invoke('feed_mark_read', { id: item.id });
    } catch (error) {
      console.error('Failed to mark notification read:', error);
    }
    router.push(`/briefs?id=${encodeURIComponent(item.targetId)}&kind=${encodeURIComponent(item.kind)}`);
    refresh();
  };

  const markAllRead = async () => {
    try {
      await invoke('feed_mark_all_read');
    } catch (error) {
      console.error('Failed to mark notifications read:', error);
    }
    refresh();
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label="Notifications"
          className="relative p-2 rounded-lg transition-colors duration-150 hover:bg-accent"
        >
          <Bell className="w-5 h-5 text-muted-foreground" />
          {unread > 0 && (
            <span className="absolute -top-0.5 -right-0.5 min-w-[1rem] h-4 px-1 rounded-full bg-red-500 text-[10px] leading-4 text-white text-center">
              {unread > 9 ? '9+' : unread}
            </span>
          )}
        </button>
      </PopoverTrigger>
      <PopoverContent side={side} align="start" className="w-80 p-0">
        {items.length === 0 ? (
          <p className="px-3 py-4 text-sm text-muted-foreground">No notifications</p>
        ) : (
          <>
            <div className="flex justify-end px-3 py-2 border-b">
              <button type="button" onClick={markAllRead} className="text-xs text-muted-foreground hover:text-foreground">
                Mark all as read
              </button>
            </div>
            <ul className="max-h-96 overflow-y-auto py-1">
              {items.map((item) => (
                <li key={item.id}>
                  <button
                    type="button"
                    onClick={() => openItem(item)}
                    className={`w-full text-left px-3 py-2 text-sm hover:bg-accent ${item.read ? '' : 'font-semibold'}`}
                  >
                    {item.title}
                  </button>
                </li>
              ))}
            </ul>
          </>
        )}
      </PopoverContent>
    </Popover>
  );
}
