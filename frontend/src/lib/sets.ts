import type { SetProblemItem } from '@/api/types'

export function isDone(progress: string): boolean {
  return progress === 'solved' || progress === 'credited'
}

/** Where the current problem sits in its set, with its neighbours by position. */
export function setPosition(problems: SetProblemItem[], slug: string) {
  const sorted = [...problems].sort((a, b) => a.position - b.position)
  const index = sorted.findIndex((p) => p.slug === slug)
  if (index < 0) return null
  return { index, total: sorted.length, prev: sorted[index - 1] ?? null, next: sorted[index + 1] ?? null }
}

/** The problem to suggest after `slug` is solved: the first unfinished one after it,
 * wrapping around to the start; null when everything else is done. */
export function nextUnsolved(problems: SetProblemItem[], slug: string): SetProblemItem | null {
  const sorted = [...problems].sort((a, b) => a.position - b.position)
  const index = sorted.findIndex((p) => p.slug === slug)
  const ordered = index < 0 ? sorted : [...sorted.slice(index + 1), ...sorted.slice(0, index)]
  return ordered.find((p) => p.slug !== slug && !isDone(p.progress)) ?? null
}
