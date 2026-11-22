'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { SettingsRow, useSavedTick } from '@/components/Settings/SettingsRow';
import { confirmAction } from '@/lib/confirm';
import {
  DEST_STATE_LABEL, MAX_DESTINATIONS, mappingChoices, nextColour, notionDbUrl,
  type DbSchema, type DestStatus, type Destination, type FixedKind, type FixedValue, type Mapping, type NotionConfig,
} from '@/lib/notion';

const selectCls = 'h-9 rounded-md border border-input bg-background px-2 text-sm';
const Alert = ({ msg }: { msg: string | null }) => (msg ? <p role="alert" className="mt-1 text-sm text-destructive">{msg}</p> : null);
const Dot = ({ colour }: { colour: number }) => (
  <i aria-hidden="true" className="inline-block h-2 w-2 shrink-0 rounded-full ring-1 ring-border" style={{ background: `hsl(var(--db-${colour}))` }} />
);

type Form = { id: string; colour: number; position: number; old: Destination | null; name: string; link: string; schema: DbSchema | null; due: string; meeting: string; fixed: Record<string, string> };

export function NotionSettings() {
  const [cfg, setCfg] = useState<NotionConfig | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [templates, setTemplates] = useState<{ id: string; name: string }[]>([]);
  const [token, setToken] = useState('');
  const [tokenErr, setTokenErr] = useState<string | null>(null);
  const [tokenLine, setTokenLine] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [tokenSaved, markTokenSaved] = useSavedTick();
  const [form, setForm] = useState<Form | null>(null);
  const [formErr, setFormErr] = useState<string | null>(null);
  const [tableErr, setTableErr] = useState<string | null>(null);
  const [defaultsErr, setDefaultsErr] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false); // state lags a same-tick second click
  const addRef = useRef<HTMLButtonElement>(null);
  const focusAdd = useRef(false); // set by Delete/Save/Cancel: their button is gone after the next render
  useEffect(() => { if (focusAdd.current) { focusAdd.current = false; addRef.current?.focus(); } });

  const reload = useCallback(async () => {
    try { setCfg(await invoke<NotionConfig>('notion_get_config')); setLoadErr(null); } catch (e) { setLoadErr(String(e)); }
  }, []);
  useEffect(() => {
    void reload();
    invoke<{ templates: { id: string; name: string }[] }>('api_list_templates').then((r) => setTemplates(r?.templates ?? [])).catch(() => {});
  }, [reload]);

  const dests = [...(cfg?.destinations ?? [])].sort((a, b) => a.position - b.position);

  const saveToken = async () => {
    setBusy(true);
    try { await invoke('notion_set_token', { token: token.trim() }); setToken(''); setTokenErr(null); setTokenLine(null); await reload(); markTokenSaved(); }
    catch (e) { setTokenErr(String(e)); } finally { setBusy(false); }
  };
  const removeToken = async () => {
    setBusy(true);
    try { await invoke('notion_remove_token'); setTokenErr(null); setTokenLine(null); await reload(); }
    catch (e) { setTokenErr(String(e)); } finally { setBusy(false); }
  };
  const testToken = async () => {
    setBusy(true);
    try {
      const out = (await invoke<DestStatus[]>('notion_test')) ?? [];
      const first = out.find((s) => s.destId === '');
      setTokenLine(!first ? null : first.state === 'connected' ? 'Works' : `${DEST_STATE_LABEL[first.state]}${first.message ? `: ${first.message}` : ''}`);
      setTokenErr(null);
      await reload();
    } catch (e) { setTokenErr(String(e)); } finally { setBusy(false); }
  };

  const openForm = (d: Destination | null) => {
    setFormErr(null);
    setForm({ id: d?.id ?? '', colour: d?.colour ?? nextColour(dests), position: d?.position ?? dests.length, old: d, name: d?.name ?? '', link: d ? notionDbUrl(d.databaseId) : '', schema: null, due: '', meeting: '', fixed: {} });
  };
  const readColumns = async () => {
    if (!form) return;
    try {
      const schema = await invoke<DbSchema>('notion_read_columns', { link: form.link.trim() });
      // Keep the saved column choices whose columns still exist.
      const c = mappingChoices(schema);
      const m = form.old?.mapping;
      const fixed: Record<string, string> = {};
      for (const f of m?.fixed ?? []) {
        const p = c.fixed.find((x) => x.name === f.prop);
        const v = Array.isArray(f.value) ? f.value[0] : String(f.value);
        if (p && (p.kind === 'checkbox' || p.options.includes(v))) fixed[f.prop] = v;
      }
      setForm({
        ...form, schema, fixed, name: form.name || schema.title,
        due: c.dates.some((p) => p.name === m?.dueProp) ? m!.dueProp! : '',
        meeting: c.texts.some((p) => p.name === m?.meetingProp) ? m!.meetingProp! : '',
      });
      setFormErr(null);
    } catch (e) { setFormErr(String(e)); }
  };
  const mappingOf = (f: Form): Mapping | null => {
    if (!f.schema) return null;
    const c = mappingChoices(f.schema);
    if (!c.title) return null;
    const fixed: FixedValue[] = [];
    for (const p of c.fixed) {
      const v = f.fixed[p.name];
      if (!v) continue;
      const kind = p.kind as FixedKind;
      fixed.push({ prop: p.name, kind, value: kind === 'checkbox' ? v === 'true' : kind === 'multi_select' ? [v] : v });
    }
    return { titleProp: c.title.name, dueProp: f.due || null, meetingProp: f.meeting || null, fixed };
  };
  const saveForm = async () => {
    if (!form || savingRef.current) return;
    const m = mappingOf(form);
    const base = form.old;
    if (!base && !m) return;
    const dest: Destination = base && !m
      ? { ...base, name: form.name.trim() }
      : { id: form.id, name: form.name.trim(), colour: form.colour, position: form.position, databaseId: form.schema!.databaseId, dataSourceId: form.schema!.dataSourceId, mapping: m!, lastStatus: null, lastMessage: null };
    savingRef.current = true;
    setSaving(true);
    try { await invoke('notion_save_destination', { dest }); focusAdd.current = true; setForm(null); await reload(); }
    catch (e) { setFormErr(String(e)); } finally { savingRef.current = false; setSaving(false); }
  };
  const del = async (d: Destination) => {
    if (!(await confirmAction({ title: `Delete ${d.name}?`, body: 'Tasks waiting for it will need a database again.', confirmLabel: 'Delete', destructive: true }))) return;
    try { await invoke('notion_delete_destination', { id: d.id }); setTableErr(null); focusAdd.current = true; await reload(); }
    catch (e) { setTableErr(String(e)); }
  };
  const openDb = (d: Destination) => invoke('open_external_url', { url: notionDbUrl(d.databaseId) }).catch((e) => setTableErr(String(e)));
  const setDefault = async (templateKey: string, destId: string | null) => {
    try { await invoke('notion_set_category_default', { templateKey, destId }); setDefaultsErr(null); await reload(); }
    catch (e) { setDefaultsErr(String(e)); }
  };

  const choices = form?.schema ? mappingChoices(form.schema) : null;
  const linkChanged = !!form?.old && !form.schema && form.link.trim() !== notionDbUrl(form.old.databaseId);
  const canSave = !!form && !saving && !linkChanged && form.name.trim() !== '' && (form.old ? true : !!mappingOf(form));
  const field = 'flex flex-col gap-1 text-sm';
  const hasToken = cfg?.tokenLast4 != null;

  return (
    <div>
      <p className="py-2 text-sm text-muted-foreground">Conclave creates tasks in your Notion databases only when you click Send. The token is kept in Windows Credential Manager.</p>
      {loadErr && <Alert msg={loadErr} />}

      <SettingsRow label="Integration token" saved={tokenSaved} htmlFor={hasToken ? undefined : 'notion-token'} block
        helper="Create an internal integration in Notion's settings, then share each database below with it (database ••• → Connections).">
        {hasToken ? (
          <div className="flex flex-wrap items-center gap-3">
            <span className="text-sm">Token saved ••••{cfg!.tokenLast4}</span>
            <Button size="sm" variant="outline" onClick={testToken} disabled={busy}>Test</Button>
            <Button size="sm" variant="outline" onClick={removeToken} disabled={busy}>Remove</Button>
            {tokenLine && <span role="status" className={`text-sm ${tokenLine === 'Works' ? 'text-emerald-700 dark:text-emerald-400' : 'text-amber-700 dark:text-amber-400'}`}>{tokenLine}</span>}
          </div>
        ) : (
          <div className="flex flex-wrap items-center gap-2">
            <Input id="notion-token" type="password" autoComplete="off" className="w-72" value={token} onChange={(e) => setToken(e.target.value)} />
            <Button size="sm" onClick={saveToken} disabled={!token.trim() || busy}>Save</Button>
          </div>
        )}
        <Alert msg={tokenErr} />
      </SettingsRow>

      <SettingsRow label="Notion databases" block>
        {dests.length > 0 && (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left text-xs text-muted-foreground">
                  <th scope="col" className="py-1 pr-3 font-medium">Name</th>
                  <th scope="col" className="py-1 pr-3 font-medium">Notion database</th>
                  <th scope="col" className="py-1 pr-3 font-medium">Status</th>
                  <th scope="col" className="py-1 font-medium"><span className="sr-only">Actions</span></th>
                </tr>
              </thead>
              <tbody>
                {dests.map((d) => (
                  <tr key={d.id} className="border-t border-border align-top">
                    <td className="py-2 pr-3"><span className="inline-flex items-center gap-2"><Dot colour={d.colour} />{d.name}</span></td>
                    <td className="py-2 pr-3"><button type="button" className="underline" aria-label={`Open ${d.name} in Notion`} onClick={() => openDb(d)}>Open ↗</button></td>
                    <td className={`py-2 pr-3 ${d.lastStatus === 'connected' ? 'text-emerald-700 dark:text-emerald-400' : d.lastStatus ? 'text-amber-700 dark:text-amber-400' : 'text-muted-foreground'}`}>
                      {d.lastStatus ? DEST_STATE_LABEL[d.lastStatus] : 'Not tested'}
                      {d.lastStatus && d.lastStatus !== 'connected' && d.lastMessage ? `: ${d.lastMessage}` : ''}
                    </td>
                    <td className="py-2 whitespace-nowrap">
                      <Button size="sm" variant="ghost" aria-label={`Edit ${d.name}`} onClick={() => openForm(d)}>Edit</Button>
                      <Button size="sm" variant="ghost" aria-label={`Delete ${d.name}`} onClick={() => del(d)}>Delete</Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        <Alert msg={tableErr} />
        {!form && <Button ref={addRef} size="sm" variant="outline" onClick={() => openForm(null)} disabled={dests.length >= MAX_DESTINATIONS}>Add database</Button>}

        {form && (
          <div role="group" aria-label={form.old ? `Edit ${form.old.name}` : 'Add database'} className="mt-3 space-y-3 rounded-md border border-border p-3">
            <label className={field}>Name<Input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} className="w-72" /></label>
            <div className="flex flex-wrap items-end gap-2">
              <label className={field}>Notion database link<Input value={form.link} onChange={(e) => setForm({ ...form, link: e.target.value })} className="w-96 max-w-full" /></label>
              <Button size="sm" variant="outline" onClick={readColumns} disabled={!form.link.trim()}>Read columns</Button>
            </div>
            {form.old && !form.schema && <p className="text-sm text-muted-foreground">{linkChanged ? 'Press Read columns to use the new link.' : 'Mapping kept. Press Read columns to change it.'}</p>}
            {choices && (
              <div className="space-y-3">
                <p className="text-sm">Task text → {choices.title ? choices.title.name : 'no title column found'}</p>
                <label className={field}>Due date
                  <select className={selectCls} value={form.due} onChange={(e) => setForm({ ...form, due: e.target.value })}>
                    <option value="">None</option>
                    {choices.dates.map((p) => <option key={p.id} value={p.name}>{p.name}</option>)}
                  </select>
                </label>
                <label className={field}>Meeting
                  <select className={selectCls} value={form.meeting} onChange={(e) => setForm({ ...form, meeting: e.target.value })}>
                    <option value="">None</option>
                    {choices.texts.map((p) => <option key={p.id} value={p.name}>{p.name}</option>)}
                  </select>
                </label>
                {choices.fixed.map((p) => (
                  <label key={p.id} className={field}>{p.name}
                    <select className={selectCls} value={form.fixed[p.name] ?? ''} onChange={(e) => setForm({ ...form, fixed: { ...form.fixed, [p.name]: e.target.value } })}>
                      <option value="">Not set</option>
                      {p.kind === 'checkbox'
                        ? <><option value="true">Checked</option><option value="false">Unchecked</option></>
                        : p.options.map((o) => <option key={o} value={o}>{o}</option>)}
                    </select>
                  </label>
                ))}
                {choices.unused.length > 0 && <p className="text-xs text-muted-foreground">Not used: {choices.unused.map((p) => p.name).join(', ')}</p>}
              </div>
            )}
            <Alert msg={formErr} />
            <div className="flex gap-2">
              <Button size="sm" onClick={saveForm} disabled={!canSave}>Save</Button>
              <Button size="sm" variant="outline" onClick={() => { focusAdd.current = true; setForm(null); }}>Cancel</Button>
            </div>
          </div>
        )}
      </SettingsRow>

      <SettingsRow label="Default database per meeting category" block helper="A new task starts with this destination. You can change it on each task before sending.">
        <ul className="space-y-2">
          {[...templates.map((t) => ({ key: t.id, name: t.name })), { key: '', name: 'No template' }].map((t) => (
            <li key={t.key} className="flex flex-wrap items-center justify-between gap-2 text-sm">
              <span>{t.name}</span>
              <select className={selectCls} aria-label={`${t.name} default`} value={cfg?.categoryDefaults.find((c) => c.templateKey === t.key)?.destId ?? ''}
                onChange={(e) => setDefault(t.key, e.target.value || null)}>
                <option value="">Ask me</option>
                {dests.map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
              </select>
            </li>
          ))}
        </ul>
        <Alert msg={defaultsErr} />
      </SettingsRow>
    </div>
  );
}
