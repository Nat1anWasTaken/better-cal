export type Tone = 'success' | 'error' | 'warning' | 'neutral' | 'progress'

export const VERDICTS: Record<string, { label: string; tone: Tone }> = {
  accepted: { label: '通過', tone: 'success' },
  wrong_answer: { label: '答案不同', tone: 'error' },
  compile_error: { label: '編譯失敗', tone: 'error' },
  runtime_error: { label: '執行時發生錯誤', tone: 'error' },
  time_limit: { label: '超過時間限制', tone: 'error' },
  memory_limit: { label: '超過記憶體限制', tone: 'error' },
  output_limit: { label: '輸出太多', tone: 'error' },
  completed: { label: '執行完成', tone: 'neutral' },
  system_error: { label: '評測服務異常', tone: 'warning' },
  skipped: { label: '未執行', tone: 'neutral' },
}

export function verdictInfo(v: string | null | undefined): { label: string; tone: Tone } {
  if (!v) return { label: '評測中', tone: 'progress' }
  return VERDICTS[v] ?? { label: v, tone: 'neutral' }
}

export const PHASES = ['queued', 'compiling', 'running', 'done'] as const
export const PHASE_LABELS: Record<string, string> = {
  queued: '等待中',
  compiling: '編譯中',
  running: '執行中',
  done: '完成',
  system_error: '服務異常',
}

export const PROGRESS: Record<string, { label: string; tone: Tone }> = {
  solved: { label: '已通過', tone: 'success' },
  credited: { label: '已認列', tone: 'success' },
  attempted: { label: '嘗試中', tone: 'warning' },
  not_started: { label: '尚未開始', tone: 'neutral' },
}

export function formatTime(iso: string | null | undefined): string {
  if (!iso) return '—'
  return new Date(iso).toLocaleString('zh-TW', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' })
}

export const STATUS_LABELS: Record<string, string> = {
  draft: '草稿',
  published: '已發布',
  archived: '已封存',
}

/** Readable form of a catalog template for lists: conditionals removed,
 * variables shown as ‹name›, backticks dropped. */
export function templatePreview(t: string): string {
  return t
    .replace(/\{[?!/][a-z_][a-z0-9_]*\}/gi, '')
    .replace(/\{([a-z][a-z0-9_]*)\}/gi, '‹$1›')
    .replace(/`/g, '')
    .trim()
}

const RUN_STATUS: Record<string, { label: string; tone: Tone }> = {
  ok: { label: '執行完成', tone: 'success' },
  non_zero_exit: { label: '結束代碼不是 0', tone: 'warning' },
  signaled: { label: '執行時崩潰', tone: 'error' },
  time_limit: { label: '超過時間限制', tone: 'error' },
  memory_limit: { label: '超過記憶體限制', tone: 'error' },
  output_limit: { label: '輸出太多', tone: 'error' },
  sandbox_error: { label: '服務異常', tone: 'neutral' },
}

/** One playground run (no expected output, so no right/wrong). */
export function runStatusInfo(status: string, exitCode?: number | null): { label: string; tone: Tone } {
  const info = RUN_STATUS[status] ?? { label: status, tone: 'neutral' as Tone }
  return status === 'non_zero_exit' && exitCode != null ? { ...info, label: `${info.label}（${exitCode}）` } : info
}
