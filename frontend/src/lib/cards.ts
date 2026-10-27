import type { FeedbackCard } from '@/api/types'
import type { EditorDiagnostic } from '@/components/CodeEditor'

export interface CardGroups {
  primary: FeedbackCard | null
  /** Other errors that are not suspected knock-on effects. */
  errors: FeedbackCard[]
  /** Possibly caused by an earlier problem; shown collapsed. */
  cascades: FeedbackCard[]
  /** Runtime / wrong-answer / system cards that are not primary. */
  results: FeedbackCard[]
  /** Warnings and lints: 「建議檢查」. */
  warnings: FeedbackCard[]
}

const RESULT_CATEGORIES = new Set(['runtime', 'wrong_answer', 'system'])

export function groupCards(cards: FeedbackCard[]): CardGroups {
  const g: CardGroups = { primary: null, errors: [], cascades: [], results: [], warnings: [] }
  for (const c of cards) {
    if (c.primary && !g.primary) {
      g.primary = c
    } else if (c.maybe_cascade_of != null) {
      g.cascades.push(c)
    } else if (RESULT_CATEGORIES.has(c.category)) {
      g.results.push(c)
    } else if (c.severity === 'error') {
      g.errors.push(c)
    } else {
      g.warnings.push(c)
    }
  }
  return g
}

export type ConfidenceTone = 'confirmed' | 'likely' | 'guess'

export function confidenceTone(c: FeedbackCard): ConfidenceTone {
  if (c.confidence === 'confirmed') return 'confirmed'
  if (c.confidence === 'likely') return 'likely'
  return 'guess'
}

/** Editor squiggles for cards that point at code (knock-on errors are left out). */
export function cardsToDiagnostics(cards: FeedbackCard[]): EditorDiagnostic[] {
  return cards
    .filter((c) => c.span && c.maybe_cascade_of == null)
    .map((c) => ({
      line: c.span!.line,
      column: c.span!.column,
      endLine: c.span!.end_line,
      endColumn: c.span!.end_column,
      severity: c.severity === 'error' ? 'error' : c.severity === 'warning' ? 'warning' : 'info',
      message: c.title.replace(/`/g, ''),
    }))
}
