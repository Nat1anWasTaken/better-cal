/** Draft autosave state machine (pure reducer, UI-independent). */

export type SaveState =
  | { kind: 'clean'; savedAt: string | null; version: number | null }
  | { kind: 'dirty'; savedAt: string | null; version: number | null }
  | { kind: 'saving'; savedAt: string | null; version: number | null; dirtyAgain: boolean }
  | { kind: 'offline'; savedAt: string | null; version: number | null }
  | { kind: 'conflict'; savedAt: string | null; version: number | null; serverVersion: number }

export type SaveEvent =
  | { type: 'loaded'; version: number | null; savedAt: string | null }
  | { type: 'edit' }
  | { type: 'save_start' }
  | { type: 'save_ok'; version: number; savedAt: string }
  | { type: 'save_failed' }
  | { type: 'conflict'; serverVersion: number }
  | { type: 'resolved'; version: number; savedAt: string | null; dirty: boolean }

export const initialSave: SaveState = { kind: 'clean', savedAt: null, version: null }

export function saveReducer(s: SaveState, e: SaveEvent): SaveState {
  const base = { savedAt: s.savedAt, version: s.version }
  switch (e.type) {
    case 'loaded':
      return { kind: 'clean', savedAt: e.savedAt, version: e.version }
    case 'edit':
      if (s.kind === 'saving') return { ...s, dirtyAgain: true }
      if (s.kind === 'conflict') return s
      return { kind: 'dirty', ...base }
    case 'save_start':
      if (s.kind === 'conflict') return s
      return { kind: 'saving', ...base, dirtyAgain: false }
    case 'save_ok':
      if (s.kind === 'saving' && s.dirtyAgain) return { kind: 'dirty', savedAt: e.savedAt, version: e.version }
      return { kind: 'clean', savedAt: e.savedAt, version: e.version }
    case 'save_failed':
      return { kind: 'offline', ...base }
    case 'conflict':
      return { kind: 'conflict', ...base, serverVersion: e.serverVersion }
    case 'resolved':
      return { kind: e.dirty ? 'dirty' : 'clean', savedAt: e.savedAt, version: e.version }
  }
}

/** Whether a save should be scheduled in this state. */
export function needsSave(s: SaveState): boolean {
  return s.kind === 'dirty' || s.kind === 'offline'
}

export function saveLabel(s: SaveState): string {
  const time = s.savedAt ? new Date(s.savedAt).toLocaleTimeString('zh-TW', { hour: '2-digit', minute: '2-digit' }) : null
  switch (s.kind) {
    case 'clean':
      return time ? `已儲存 ${time}` : '尚未修改'
    case 'dirty':
      return '有尚未儲存的修改'
    case 'saving':
      return '儲存中…'
    case 'offline':
      return '尚未儲存（離線，已暫存在此裝置）'
    case 'conflict':
      return '草稿在其他地方被修改了'
  }
}
