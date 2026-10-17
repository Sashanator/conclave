"use client"

import { useCallback, useEffect, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { toast } from "sonner"
import { Switch } from "./ui/switch"
import { Button } from "./ui/button"
import { Input } from "./ui/input"
import { Progress } from "./ui/progress"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select"
import { lastAnalysisText } from "@/lib/speakers"
import type { Person, SpeakerSettings as Settings } from "@/types"

const DELETE_PROMPT = "Voiceprints are removed; past transcripts keep the name"

async function call(command: string, args: Record<string, unknown> | undefined, failure: string) {
  try {
    await invoke(command, args)
  } catch (err) {
    toast.error(failure, { description: String(err) })
  }
}

export function SpeakerSettings() {
  const [settings, setSettings] = useState<Settings | null>(null)
  const [people, setPeople] = useState<Person[]>([])
  const [percent, setPercent] = useState<number | null>(null)
  const [downloadError, setDownloadError] = useState<string | null>(null)
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null)

  const loadSettings = useCallback(async () => {
    try {
      setSettings(await invoke<Settings>("speakers_get_settings"))
    } catch (err) {
      toast.error("Could not load speaker settings", { description: String(err) })
    }
  }, [])

  const loadPeople = useCallback(async () => {
    try {
      setPeople(await invoke<Person[]>("people_list"))
    } catch (err) {
      toast.error("Could not load known voices", { description: String(err) })
    }
  }, [])

  useEffect(() => {
    loadSettings()
    loadPeople()
    let cancelled = false
    let unlisteners: UnlistenFn[] = []
    Promise.all([
      listen<{ percent: number }>("speaker-models-progress", (e) => setPercent(e.payload.percent)),
      listen<{ ok: boolean; error: string | null }>("speaker-models-done", (e) => {
        setPercent(null)
        setDownloadError(e.payload.ok ? null : e.payload.error ?? "Unknown error")
        loadSettings()
      }),
      listen("speaker-settings-changed", () => loadSettings()),
    ]).then((fns) => {
      if (cancelled) fns.forEach((fn) => fn())
      else unlisteners = fns
    })
    return () => {
      cancelled = true
      unlisteners.forEach((fn) => fn())
    }
  }, [loadSettings, loadPeople])

  const setEnabled = async (enabled: boolean) => {
    await call("speakers_set_enabled", { enabled }, "Could not change speaker identification")
    loadSettings()
  }

  const download = async () => {
    setDownloadError(null)
    await call("speakers_download_models", undefined, "Could not start the model download")
    loadSettings()
  }

  const deleteModels = async () => {
    await call("speakers_delete_models", undefined, "Could not delete the speaker models")
    loadSettings()
  }

  const saveRename = async () => {
    if (!renaming) return
    const name = renaming.name.trim()
    if (name) await call("people_rename", { id: renaming.id, name }, "Could not rename the voice")
    setRenaming(null)
    loadPeople()
  }

  const mergeInto = async (from: string, into: string) => {
    await call("people_merge", { from, into }, "Could not merge the voices")
    loadPeople()
  }

  const remove = async (id: string) => {
    if (!window.confirm(DELETE_PROMPT)) return
    await call("people_delete", { id }, "Could not delete the voice")
    loadPeople()
  }

  const analysis = settings ? lastAnalysisText(settings.last_analysis) : null
  const sorted = [...people].sort((a, b) => Number(b.is_self) - Number(a.is_self))

  return (
    <div className="space-y-6">
      <div className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-4">
        <div className="flex items-center justify-between">
          <h3 className="text-lg font-semibold text-foreground">Identify speakers</h3>
          <Switch
            aria-label="Identify speakers"
            checked={settings?.enabled ?? false}
            disabled={!settings}
            onCheckedChange={setEnabled}
          />
        </div>

        {settings?.downloading && (
          <div className="flex items-center gap-3">
            <Progress value={percent ?? 0} className="flex-1" />
            <span className="text-sm text-muted-foreground w-12 text-right">{percent ?? 0}%</span>
          </div>
        )}

        {downloadError && (
          <div className="flex items-center gap-3">
            <p className="text-sm text-red-600 flex-1">{downloadError}</p>
            <Button size="sm" onClick={download}>Retry</Button>
          </div>
        )}

        <div className="flex items-center justify-between gap-3">
          <Button variant="outline" size="sm" onClick={deleteModels}>Delete models</Button>
          {analysis && <p className="text-sm text-muted-foreground">{analysis}</p>}
        </div>
      </div>

      <div className="bg-card rounded-lg border border-border p-6 shadow-sm">
        <h3 className="text-lg font-semibold text-foreground mb-2">Known voices</h3>
        <ul className="divide-y divide-border">
          {sorted.map((p) => {
            const others = people.filter((o) => !o.is_self && o.id !== p.id)
            return (
              <li key={p.id} data-person={p.id} className="flex items-center gap-3 py-3">
                {renaming?.id === p.id ? (
                  <>
                    <Input
                      id={`person-name-${p.id}`}
                      aria-label="Name"
                      className="flex-1"
                      value={renaming.name}
                      onChange={(e) => setRenaming({ id: p.id, name: e.target.value })}
                    />
                    <Button size="sm" onClick={saveRename}>Save</Button>
                    <Button size="sm" variant="ghost" onClick={() => setRenaming(null)}>Cancel</Button>
                  </>
                ) : (
                  <>
                    <div className="flex-1 min-w-0">
                      <div>
                        <span className="font-medium">{p.name}</span>
                        {p.is_self && <span className="ml-2 text-xs text-muted-foreground">this is you</span>}
                      </div>
                      <p className="text-sm text-muted-foreground">
                        {p.meetings} meetings · last heard {p.last_heard ? new Date(p.last_heard).toLocaleDateString() : "never"}
                      </p>
                    </div>
                    <Button variant="outline" size="sm" onClick={() => setRenaming({ id: p.id, name: p.name })}>Rename</Button>
                    {!p.is_self && others.length > 0 && (
                      <Select onValueChange={(into) => mergeInto(p.id, into)}>
                        <SelectTrigger className="w-40">
                          <SelectValue placeholder="Merge into…" />
                        </SelectTrigger>
                        <SelectContent>
                          {others.map((o) => (
                            <SelectItem key={o.id} value={o.id}>{o.name}</SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    )}
                    {!p.is_self && (
                      <Button variant="destructive" size="sm" onClick={() => remove(p.id)}>Delete</Button>
                    )}
                  </>
                )}
              </li>
            )
          })}
        </ul>
      </div>
    </div>
  )
}
