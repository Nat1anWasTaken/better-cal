import type { Segment } from '@/api/types'

export interface BlankRange {
  id: string
  from: number
  to: number
}

/** Builds the editor document from template segments and the student's
 * blanks (falling back to each blank's default text). */
export function buildDoc(segments: Segment[], blanks: Record<string, string> = {}): { doc: string; ranges: BlankRange[] } {
  let doc = ''
  const ranges: BlankRange[] = []
  for (const s of segments) {
    if (s.kind === 'fixed') {
      doc += s.text
    } else {
      const text = blanks[s.id] ?? s.default
      ranges.push({ id: s.id, from: doc.length, to: doc.length + text.length })
      doc += text
    }
  }
  return { doc, ranges }
}

/** Reads each blank's current text from the document. */
export function extractBlanks(doc: string, ranges: BlankRange[]): Record<string, string> {
  const out: Record<string, string> = {}
  for (const r of ranges) out[r.id] = doc.slice(r.from, r.to)
  return out
}

/** A change [from, to] is allowed only if it lies within one blank (ends inclusive,
 * so typing at the start or end of a blank, or into an empty blank, works). */
export function changeAllowed(from: number, to: number, ranges: BlankRange[]): boolean {
  return ranges.some((r) => from >= r.from && to <= r.to)
}

/** Shifts ranges after an edit of `[from, to)` replaced by `insertLen` chars
 * inside one blank. Applied to Monaco's content changes. */
export function applyEdit(ranges: BlankRange[], from: number, to: number, insertLen: number): BlankRange[] {
  const delta = insertLen - (to - from)
  return ranges.map((r) => {
    if (from >= r.from && to <= r.to) return { ...r, to: r.to + delta }
    if (r.from >= to) return { ...r, from: r.from + delta, to: r.to + delta }
    return r
  })
}

/** 1-based line numbers covered by fixed text (for greying out). */
export function fixedRanges(doc: string, ranges: BlankRange[]): Array<{ from: number; to: number }> {
  const out: Array<{ from: number; to: number }> = []
  let pos = 0
  for (const r of [...ranges].sort((a, b) => a.from - b.from)) {
    if (r.from > pos) out.push({ from: pos, to: r.from })
    pos = r.to
  }
  if (pos < doc.length) out.push({ from: pos, to: doc.length })
  return out
}
