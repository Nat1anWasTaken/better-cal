import type { UserPreferences } from '@/api/types'
import type { monaco } from '@/lib/monaco'

/** Matches the server's defaults (backend/crates/api/src/preferences.rs). */
export const DEFAULT_PREFERENCES: UserPreferences = {
  editor_theme: 'auto',
  keymap: 'default',
  font_size: 14,
  tab_size: 4,
  word_wrap: false,
  line_numbers: 'on',
  minimap: false,
}

const themes = { light: 'vs', dark: 'vs-dark', hc_dark: 'hc-black', hc_light: 'hc-light' } as const

/** The Monaco theme for a preference; `auto` follows the site's light/dark theme. */
export function monacoTheme(t: UserPreferences['editor_theme'], siteDark: boolean): string {
  return t === 'auto' ? (siteDark ? 'vs-dark' : 'vs') : themes[t]
}

export function editorOptions(p: UserPreferences): monaco.editor.IEditorOptions {
  return {
    fontSize: p.font_size,
    wordWrap: p.word_wrap ? 'on' : 'off',
    lineNumbers: p.line_numbers,
    minimap: { enabled: p.minimap },
  }
}
