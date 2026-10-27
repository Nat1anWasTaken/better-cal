import { useCallback, useEffect, useReducer, useRef, useState } from 'react'

import { ApiError, api } from '@/api/client'
import type { Draft } from '@/api/types'
import { initialSave, needsSave, saveReducer } from '@/lib/autosave'

export interface DraftContent {
  code: string
  blanks: Record<string, string> | null
}

interface LocalBackup extends DraftContent {
  savedAt: string
  version: number | null
}

function storageKey(userId: string, problemId: string) {
  return `ccal-draft:${userId}:${problemId}`
}

function readLocal(key: string): LocalBackup | null {
  try {
    const v = localStorage.getItem(key)
    return v ? (JSON.parse(v) as LocalBackup) : null
  } catch {
    return null
  }
}

function writeLocal(key: string, v: LocalBackup) {
  try {
    localStorage.setItem(key, JSON.stringify(v))
  } catch {
    // Storage full or blocked: server autosave still works.
  }
}

const same = (a: DraftContent, b: DraftContent) => a.code === b.code && JSON.stringify(a.blanks ?? null) === JSON.stringify(b.blanks ?? null)

/**
 * Loads the draft (server, falling back to / reconciling with a local backup)
 * and autosaves edits after a pause. Conflicts (409) pause autosave until the
 * student chooses which version to keep.
 */
export function useDraft(userId: string | undefined, problemId: string | undefined) {
  const [state, dispatch] = useReducer(saveReducer, initialSave)
  const [loaded, setLoaded] = useState<DraftContent | null | undefined>(undefined)
  const [serverDraft, setServerDraft] = useState<Draft | null>(null)
  const current = useRef<DraftContent | null>(null)
  const stateRef = useRef(state)
  stateRef.current = state
  const key = userId && problemId ? storageKey(userId, problemId) : null

  useEffect(() => {
    if (!key || !problemId) return
    let cancelled = false
    ;(async () => {
      const local = readLocal(key)
      let server: Draft | null = null
      try {
        server = await api.get<Draft>(`/me/drafts/${problemId}`)
      } catch (e) {
        if (!(e instanceof ApiError && e.status === 404)) {
          // Offline: use the local backup if any.
          if (!cancelled) {
            current.current = local ? { code: local.code, blanks: local.blanks } : null
            setLoaded(current.current)
            dispatch(local ? { type: 'edit' } : { type: 'loaded', version: null, savedAt: null })
            dispatch({ type: 'save_failed' })
          }
          return
        }
      }
      if (cancelled) return
      const serverContent = server ? { code: server.code, blanks: server.blanks ?? null } : null
      const localNewer = local && (!server?.updated_at || new Date(local.savedAt) > new Date(server.updated_at))
      if (local && localNewer && (!serverContent || !same(local, serverContent))) {
        // Unsaved local edits (e.g. went offline): restore and re-save them.
        setLoaded({ code: local.code, blanks: local.blanks })
        current.current = { code: local.code, blanks: local.blanks }
        dispatch({ type: 'loaded', version: server?.version ?? null, savedAt: server?.updated_at ?? null })
        dispatch({ type: 'edit' })
      } else {
        setLoaded(serverContent)
        current.current = serverContent
        dispatch({ type: 'loaded', version: server?.version ?? null, savedAt: server?.updated_at ?? null })
      }
    })()
    return () => {
      cancelled = true
    }
  }, [key, problemId])

  const save = useCallback(async () => {
    if (!problemId || !current.current || !key) return
    const content = current.current
    dispatch({ type: 'save_start' })
    try {
      const d = await api.put<Draft>(`/me/drafts/${problemId}`, {
        code: content.code,
        blanks: content.blanks,
        base_version: stateRef.current.version ?? undefined,
      })
      dispatch({ type: 'save_ok', version: d.version, savedAt: d.updated_at ?? new Date().toISOString() })
      writeLocal(key, { ...content, savedAt: d.updated_at ?? new Date().toISOString(), version: d.version })
    } catch (e) {
      if (e instanceof ApiError && e.status === 409) {
        const d = e.data as Draft
        setServerDraft(d)
        dispatch({ type: 'conflict', serverVersion: d.version })
      } else {
        dispatch({ type: 'save_failed' })
      }
    }
  }, [problemId, key])

  // Debounced autosave; offline state retries every 10s.
  useEffect(() => {
    if (!needsSave(state)) return
    const t = window.setTimeout(save, state.kind === 'offline' ? 10_000 : 1500)
    return () => window.clearTimeout(t)
  }, [state, save])

  const update = useCallback(
    (content: DraftContent) => {
      if (current.current && same(current.current, content)) return
      current.current = content
      if (key) writeLocal(key, { ...content, savedAt: new Date().toISOString(), version: stateRef.current.version })
      dispatch({ type: 'edit' })
    },
    [key],
  )

  /** Conflict resolution: keep mine (overwrite) or take the server copy. */
  const resolve = useCallback(
    (keepMine: boolean) => {
      if (!serverDraft) return null
      dispatch({ type: 'resolved', version: serverDraft.version, savedAt: serverDraft.updated_at ?? null, dirty: keepMine })
      setServerDraft(null)
      if (keepMine) return null
      const c = { code: serverDraft.code, blanks: serverDraft.blanks ?? null }
      current.current = c
      return c
    },
    [serverDraft],
  )

  const flush = useCallback(async () => {
    if (needsSave(stateRef.current)) await save()
  }, [save])

  return { state, loaded, update, resolve, flush }
}
