import { describe, expect, it } from 'vitest'

import type { FeedbackCard, Segment } from '@/api/types'

import { initialSave, needsSave, saveLabel, saveReducer } from './autosave'
import { groupCards } from './cards'
import { diffSummary, renderDiffLine, showWhitespace } from './diff'
import { errorsByPointer, errorsUnder } from './errors'
import { applyEdit, buildDoc, changeAllowed, extractBlanks, fixedRanges } from './fillin'
import { DEFAULT_PREFERENCES, editorOptions, monacoTheme } from './preferences'

describe('diff rendering', () => {
  it('labels missing and extra lines with markers, not only colour', () => {
    const m = renderDiffLine({ kind: 'missing', expected_line: 2, actual_line: null, text: 'Done↵' })
    expect(m.marker).toBe('−')
    expect(m.label).toContain('預期有')
    const x = renderDiffLine({ kind: 'extra', expected_line: null, actual_line: 1, text: 'Sum:··3↵' })
    expect(x.marker).toBe('+')
    expect(x.label).toContain('多出來')
  })
  it('makes whitespace visible', () => {
    expect(showWhitespace('a b\t\n')).toBe('a·b→↵\n')
  })
  it('summarises the first differing line', () => {
    expect(
      diffSummary([
        { kind: 'same', expected_line: 1, actual_line: 1, text: 'x' },
        { kind: 'missing', expected_line: 2, actual_line: null, text: 'y' },
        { kind: 'extra', expected_line: null, actual_line: 2, text: 'z' },
      ]),
    ).toEqual({ missing: 1, extra: 1, firstLine: 2 })
  })
})

describe('fill-in blanks', () => {
  const segments: Segment[] = [
    { kind: 'fixed', text: 'int max(int a, int b) {\n' },
    { kind: 'blank', id: 'body', default: '    return 0;\n' },
    { kind: 'fixed', text: '}\n' },
  ]
  it('builds the document with editable ranges', () => {
    const { doc, ranges } = buildDoc(segments)
    expect(doc).toBe('int max(int a, int b) {\n    return 0;\n}\n')
    expect(ranges).toEqual([{ id: 'body', from: 24, to: 38 }])
    expect(fixedRanges(doc, ranges)).toEqual([
      { from: 0, to: 24 },
      { from: 38, to: 40 },
    ])
  })
  it('uses saved blanks and extracts them back', () => {
    const { doc, ranges } = buildDoc(segments, { body: '  return a;\n' })
    expect(extractBlanks(doc, ranges)).toEqual({ body: '  return a;\n' })
  })
  it('allows edits only inside blanks (ends inclusive)', () => {
    const { ranges } = buildDoc(segments)
    expect(changeAllowed(24, 24, ranges)).toBe(true)
    expect(changeAllowed(38, 38, ranges)).toBe(true)
    expect(changeAllowed(30, 35, ranges)).toBe(true)
    expect(changeAllowed(10, 12, ranges)).toBe(false)
    expect(changeAllowed(20, 30, ranges)).toBe(false)
    expect(changeAllowed(39, 39, ranges)).toBe(false)
  })
  it('keeps ranges in sync after edits', () => {
    const { doc, ranges } = buildDoc([...segments, { kind: 'blank', id: 'tail', default: '' }])
    const next = applyEdit(ranges, 28, 28, 5)
    expect(next[0]).toEqual({ id: 'body', from: 24, to: 43 })
    expect(next[1].from).toBe(doc.length + 5)
  })
})

describe('autosave state machine', () => {
  it('goes dirty → saving → clean', () => {
    let s = saveReducer(initialSave, { type: 'loaded', version: 1, savedAt: null })
    s = saveReducer(s, { type: 'edit' })
    expect(needsSave(s)).toBe(true)
    s = saveReducer(s, { type: 'save_start' })
    expect(saveLabel(s)).toBe('儲存中…')
    s = saveReducer(s, { type: 'save_ok', version: 2, savedAt: '2026-10-26T10:00:00Z' })
    expect(s.kind).toBe('clean')
    expect(s.version).toBe(2)
    expect(saveLabel(s)).toMatch(/^已儲存/)
  })
  it('stays dirty when edited during a save', () => {
    let s = saveReducer(initialSave, { type: 'edit' })
    s = saveReducer(s, { type: 'save_start' })
    s = saveReducer(s, { type: 'edit' })
    s = saveReducer(s, { type: 'save_ok', version: 3, savedAt: '2026-10-26T10:00:00Z' })
    expect(s.kind).toBe('dirty')
  })
  it('reports offline and retries, and holds on conflict', () => {
    let s = saveReducer(initialSave, { type: 'edit' })
    s = saveReducer(s, { type: 'save_start' })
    s = saveReducer(s, { type: 'save_failed' })
    expect(saveLabel(s)).toContain('離線')
    expect(needsSave(s)).toBe(true)
    s = saveReducer(s, { type: 'conflict', serverVersion: 9 })
    expect(needsSave(s)).toBe(false)
    expect(saveReducer(s, { type: 'edit' }).kind).toBe('conflict')
    s = saveReducer(s, { type: 'resolved', version: 9, savedAt: null, dirty: true })
    expect(s.kind).toBe('dirty')
  })
})

describe('api error pointers', () => {
  const errs = [
    { pointer: '/content/title', code: 'required', message: '題目名稱不可空白' },
    { pointer: '/content/tests/1/input', code: 'too_long', message: '太長' },
    { pointer: '/content/tests/1/input', code: 'x', message: '另一個' },
    { pointer: '/slug', code: 'invalid_slug', message: 'slug 錯' },
  ]
  it('maps by pointer with prefix stripping and merges duplicates', () => {
    const m = errorsByPointer(errs, '/content')
    expect(m['/title']).toBe('題目名稱不可空白')
    expect(m['/tests/1/input']).toBe('太長；另一個')
    expect(m['/slug']).toBeUndefined()
    expect(errorsUnder(m, '/tests')).toEqual(['太長；另一個'])
  })
})

describe('card grouping', () => {
  const card = (over: Partial<FeedbackCard>): FeedbackCard => ({
    finding_id: 1,
    rule: 'r',
    category: 'compile_error',
    severity: 'error',
    confidence: 'likely',
    confidence_label: '很可能',
    title: 't',
    what: 'w',
    next_steps: [],
    hints: [],
    hints_total: 0,
    hints_allowed: 0,
    precision: 'exact',
    in_fixed_region: false,
    primary: false,
    raw: [],
    ...over,
  })
  it('separates primary, cascades, results and warnings', () => {
    const g = groupCards([
      card({ finding_id: 1, primary: true }),
      card({ finding_id: 2, maybe_cascade_of: 1 }),
      card({ finding_id: 3 }),
      card({ finding_id: 4, severity: 'warning', category: 'warning' }),
      card({ finding_id: 5, category: 'runtime' }),
    ])
    expect(g.primary?.finding_id).toBe(1)
    expect(g.cascades.map((c) => c.finding_id)).toEqual([2])
    expect(g.errors.map((c) => c.finding_id)).toEqual([3])
    expect(g.warnings.map((c) => c.finding_id)).toEqual([4])
    expect(g.results.map((c) => c.finding_id)).toEqual([5])
  })
})

describe('template preview', async () => {
  const { templatePreview } = await import('./format')
  it('strips conditionals and marks variables', () => {
    expect(templatePreview('{?keyword}`{keyword}` 後面{/keyword}{!keyword}這裡{/keyword}')).toBe('‹keyword› 後面這裡')
  })
})

describe('preferences', () => {
  it('maps editor themes, following the site theme for auto', () => {
    expect(monacoTheme('auto', true)).toBe('vs-dark')
    expect(monacoTheme('auto', false)).toBe('vs')
    expect(monacoTheme('light', true)).toBe('vs')
    expect(monacoTheme('hc_dark', false)).toBe('hc-black')
    expect(monacoTheme('hc_light', true)).toBe('hc-light')
  })
  it('turns preferences into Monaco options', () => {
    expect(editorOptions({ ...DEFAULT_PREFERENCES, font_size: 18, word_wrap: true, line_numbers: 'relative', minimap: true })).toEqual({
      fontSize: 18,
      wordWrap: 'on',
      lineNumbers: 'relative',
      minimap: { enabled: true },
    })
  })
})
