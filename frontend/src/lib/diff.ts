import type { DiffLine } from '@/api/types'

export interface RenderedDiffLine {
  marker: '+' | '−' | ' '
  /** Screen-reader / visible text label so meaning never depends on colour. */
  label: string
  kind: DiffLine['kind']
  text: string
  expected?: number | null
  actual?: number | null
}

export function renderDiffLine(l: DiffLine): RenderedDiffLine {
  switch (l.kind) {
    case 'missing':
      return { marker: '−', label: '預期有、你的輸出沒有', kind: l.kind, text: l.text, expected: l.expected_line, actual: null }
    case 'extra':
      return { marker: '+', label: '你的輸出多出來的', kind: l.kind, text: l.text, expected: null, actual: l.actual_line }
    default:
      return { marker: ' ', label: '相同', kind: l.kind, text: l.text, expected: l.expected_line, actual: l.actual_line }
  }
}

/** Makes whitespace visible in raw output: space → ·, tab → →, newline → ↵. */
export function showWhitespace(s: string): string {
  return s.replace(/ /g, '·').replace(/\t/g, '→').replace(/\n/g, '↵\n')
}

export function diffSummary(lines: DiffLine[]): { missing: number; extra: number; firstLine: number | null } {
  let missing = 0
  let extra = 0
  let firstLine: number | null = null
  for (const l of lines) {
    if (l.kind === 'missing') missing++
    if (l.kind === 'extra') extra++
    if (l.kind !== 'same' && firstLine === null) firstLine = l.expected_line ?? l.actual_line ?? null
  }
  return { missing, extra, firstLine }
}
