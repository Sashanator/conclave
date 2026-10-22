// Camel-case shape of the Rust `TaskRow` returned by `tasks_list` and `tasks_add`.
export type TaskRow = {
  id: string;
  meetingId: string | null;
  meetingTitle: string | null;
  meetingDate: string | null;
  text: string;
  owner: 'me' | 'unassigned';
  due: string | null;
  at: number | null;
  status: 'open' | 'done' | 'dismissed';
  source: 'auto' | 'manual';
  createdAt: string;
};

export type TaskGroup = {
  key: string;
  title: string;
  date: string | null;
  meetingId: string | null;
  tasks: TaskRow[];
};

export const MANUAL_GROUP_TITLE = 'Your own tasks';

/** Groups keep the order of their first row. Manual tasks (no meeting) form one group, last. */
export function groupByMeeting(rows: TaskRow[]): TaskGroup[] {
  const groups = new Map<string, TaskGroup>();
  for (const row of rows) {
    const key = row.meetingId === null ? 'manual' : `meeting:${row.meetingId}`;
    let group = groups.get(key);
    if (!group) {
      group = {
        key,
        title: row.meetingId === null ? MANUAL_GROUP_TITLE : row.meetingTitle ?? '',
        date: row.meetingId === null ? null : row.meetingDate,
        meetingId: row.meetingId,
        tasks: [],
      };
      groups.set(key, group);
    }
    group.tasks.push(row);
  }
  const all = [...groups.values()];
  const manual = all.filter((g) => g.meetingId === null);
  return [...all.filter((g) => g.meetingId !== null), ...manual];
}
