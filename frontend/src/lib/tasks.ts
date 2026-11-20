// Camel-case shape of the Rust `TaskRow` returned by `tasks_queue_list`.
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
  review: 'open' | 'keep' | 'drop' | 'sending' | 'sent';
  removedBy: 'ai' | 'user' | null;
  destId: string | null;
  dueDate: string | null;
  notionUrl: string | null;
  sentAt: string | null;
  sendError: string | null;
  sendAmbiguous: boolean;
  templateId: string | null;
};
