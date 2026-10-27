import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { cpp } from '@codemirror/lang-cpp'
import { bracketMatching, defaultHighlightStyle, indentOnInput, syntaxHighlighting } from '@codemirror/language'
import { type Diagnostic, lintGutter, setDiagnostics } from '@codemirror/lint'
import { Annotation, EditorState, type Extension, RangeSetBuilder, StateEffect, StateField } from '@codemirror/state'
import {
  Decoration,
  type DecorationSet,
  EditorView,
  drawSelection,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from '@codemirror/view'
import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react'

import { type BlankRange, changeAllowed, extractBlanks, fixedRanges } from '@/lib/fillin'

export interface EditorDiagnostic {
  line: number
  column: number
  endLine?: number
  endColumn?: number
  severity: 'error' | 'warning' | 'info'
  message: string
}

export interface CodeEditorHandle {
  /** Moves the cursor to (line, column), scrolls there, flashes the line and focuses. */
  jumpTo: (line: number, column?: number) => void
  getDoc: () => string
  getBlanks: () => Record<string, string>
  /** Replaces the whole document (and blank ranges) programmatically. */
  reset: (doc: string, ranges?: BlankRange[]) => void
  focus: () => void
}

interface Props {
  initialDoc: string
  /** Fill-in problems: only these ranges are editable. */
  blankRanges?: BlankRange[]
  readOnly?: boolean
  diagnostics?: EditorDiagnostic[]
  onChange?: (doc: string) => void
  onRun?: () => void
  onSubmit?: () => void
  ariaLabel?: string
  className?: string
}

const programmatic = Annotation.define<boolean>()
const setRanges = StateEffect.define<BlankRange[]>()
const flashLine = StateEffect.define<number | null>()

const rangesField = StateField.define<BlankRange[]>({
  create: () => [],
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setRanges)) return e.value
    if (!tr.docChanged) return value
    // Insertions at a blank's edges grow the blank.
    return value.map((r) => ({ id: r.id, from: tr.changes.mapPos(r.from, -1), to: tr.changes.mapPos(r.to, 1) }))
  },
})

const lockedMark = Decoration.mark({ class: 'cm-locked', attributes: { title: '題目提供的程式，不能修改' } })
const blankMark = Decoration.mark({ class: 'cm-blank' })

function fillInDecorations(state: EditorState): DecorationSet {
  const ranges = state.field(rangesField)
  if (ranges.length === 0) return Decoration.none
  const b = new RangeSetBuilder<Decoration>()
  const all: Array<{ from: number; to: number; deco: Decoration }> = [
    ...fixedRanges(state.doc.toString(), ranges).map((r) => ({ ...r, deco: lockedMark })),
    ...ranges.filter((r) => r.to > r.from).map((r) => ({ from: r.from, to: r.to, deco: blankMark })),
  ].sort((a, c) => a.from - c.from)
  for (const r of all) if (r.to > r.from) b.add(r.from, r.to, r.deco)
  return b.finish()
}

const fillInDecoField = StateField.define<DecorationSet>({
  create: (s) => fillInDecorations(s),
  update: (_, tr) => fillInDecorations(tr.state),
  provide: (f) => EditorView.decorations.from(f),
})

const jumpLineDeco = Decoration.line({ class: 'cm-jump-line' })
const flashField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(flashLine)) {
        if (e.value === null) return Decoration.none
        const line = tr.state.doc.line(Math.min(Math.max(e.value, 1), tr.state.doc.lines))
        return Decoration.set([jumpLineDeco.range(line.from)])
      }
    }
    return value.map(tr.changes)
  },
  provide: (f) => EditorView.decorations.from(f),
})

/** Rejects user edits outside the blank ranges (fill-in problems). */
const lockFilter = EditorState.transactionFilter.of((tr) => {
  if (!tr.docChanged || tr.annotation(programmatic)) return tr
  const ranges = tr.startState.field(rangesField)
  if (ranges.length === 0) return tr
  let ok = true
  tr.changes.iterChangedRanges((fromA, toA) => {
    if (!changeAllowed(fromA, toA, ranges)) ok = false
  })
  return ok ? tr : []
})

function posOf(state: EditorState, line: number, column: number): number {
  const l = state.doc.line(Math.min(Math.max(line, 1), state.doc.lines))
  return Math.min(l.from + Math.max(column - 1, 0), l.to)
}

function toCmDiagnostics(state: EditorState, ds: EditorDiagnostic[]): Diagnostic[] {
  return ds.map((d) => {
    const from = posOf(state, d.line, d.column)
    let to = d.endLine ? posOf(state, d.endLine, (d.endColumn ?? d.column) + 1) : from + 1
    if (to <= from) to = Math.min(from + 1, state.doc.line(Math.min(d.line, state.doc.lines)).to)
    return { from, to: Math.max(to, from), severity: d.severity, message: d.message }
  })
}

const theme = EditorView.theme({
  '&': { height: '100%' },
  '.cm-content': { paddingBlock: '8px' },
  '&.cm-focused': { outline: 'none' },
})

export const CodeEditor = forwardRef<CodeEditorHandle, Props>(function CodeEditor(
  { initialDoc, blankRanges, readOnly, diagnostics, onChange, onRun, onSubmit, ariaLabel = '程式編輯器', className },
  ref,
) {
  const host = useRef<HTMLDivElement>(null)
  const view = useRef<EditorView | null>(null)
  const callbacks = useRef({ onChange, onRun, onSubmit })
  useEffect(() => {
    callbacks.current = { onChange, onRun, onSubmit }
  })

  useEffect(() => {
    if (!host.current) return
    const extensions: Extension[] = [
      lineNumbers(),
      highlightActiveLineGutter(),
      history(),
      drawSelection(),
      indentOnInput(),
      bracketMatching(),
      highlightActiveLine(),
      syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
      cpp(),
      lintGutter(),
      rangesField,
      fillInDecoField,
      flashField,
      lockFilter,
      theme,
      EditorState.tabSize.of(4),
      EditorView.contentAttributes.of({ 'aria-label': ariaLabel }),
      keymap.of([
        { key: 'Mod-Enter', run: () => (callbacks.current.onRun?.(), true) },
        { key: 'Mod-Shift-Enter', run: () => (callbacks.current.onSubmit?.(), true) },
        ...defaultKeymap,
        ...historyKeymap,
        indentWithTab,
      ]),
      EditorView.updateListener.of((u) => {
        if (u.docChanged) callbacks.current.onChange?.(u.state.doc.toString())
      }),
    ]
    if (readOnly) extensions.push(EditorState.readOnly.of(true), EditorView.editable.of(false))
    const v = new EditorView({ parent: host.current, state: EditorState.create({ doc: initialDoc, extensions }) })
    if (blankRanges?.length) v.dispatch({ effects: setRanges.of(blankRanges) })
    view.current = v
    return () => {
      v.destroy()
      view.current = null
    }
    // The editor is created once; later updates go through the imperative handle.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [readOnly])

  useEffect(() => {
    const v = view.current
    if (!v) return
    v.dispatch(setDiagnostics(v.state, toCmDiagnostics(v.state, diagnostics ?? [])))
  }, [diagnostics])

  useImperativeHandle(
    ref,
    () => ({
      jumpTo(line, column = 1) {
        const v = view.current
        if (!v) return
        const pos = posOf(v.state, line, column)
        v.dispatch({ selection: { anchor: pos }, effects: [EditorView.scrollIntoView(pos, { y: 'center' }), flashLine.of(line)] })
        v.focus()
        window.setTimeout(() => view.current?.dispatch({ effects: flashLine.of(null) }), 2500)
      },
      getDoc: () => view.current?.state.doc.toString() ?? '',
      getBlanks() {
        const v = view.current
        if (!v) return {}
        return extractBlanks(v.state.doc.toString(), v.state.field(rangesField))
      },
      reset(doc, ranges) {
        const v = view.current
        if (!v) return
        v.dispatch({
          changes: { from: 0, to: v.state.doc.length, insert: doc },
          annotations: programmatic.of(true),
          effects: setRanges.of(ranges ?? []),
        })
      },
      focus: () => view.current?.focus(),
    }),
    [],
  )

  return <div ref={host} className={className} data-testid="code-editor" />
})
