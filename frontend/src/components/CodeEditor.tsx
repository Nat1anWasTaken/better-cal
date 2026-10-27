import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react'

import { type BlankRange, applyEdit, changeAllowed, extractBlanks, fixedRanges } from '@/lib/fillin'
import { monaco } from '@/lib/monaco'

export interface EditorDiagnostic {
  line: number
  column: number
  endLine?: number
  endColumn?: number
  severity: 'error' | 'warning' | 'info'
  message: string
}

export interface CodeEditorHandle {
  /** Selects the line, scrolls (line, column) into view and focuses. */
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

const severities = {
  error: monaco.MarkerSeverity.Error,
  warning: monaco.MarkerSeverity.Warning,
  info: monaco.MarkerSeverity.Info,
}

function rangeOf(model: monaco.editor.ITextModel, from: number, to: number): monaco.IRange {
  const a = model.getPositionAt(from)
  const b = model.getPositionAt(to)
  return { startLineNumber: a.lineNumber, startColumn: a.column, endLineNumber: b.lineNumber, endColumn: b.column }
}

export const CodeEditor = forwardRef<CodeEditorHandle, Props>(function CodeEditor(
  { initialDoc, blankRanges, readOnly, diagnostics, onChange, onRun, onSubmit, ariaLabel = '程式編輯器', className },
  ref,
) {
  const host = useRef<HTMLDivElement>(null)
  const editor = useRef<monaco.editor.IStandaloneCodeEditor | null>(null)
  const ranges = useRef<BlankRange[]>([])
  const decorate = useRef(() => {})
  const programmatic = useRef(false)
  const callbacks = useRef({ onChange, onRun, onSubmit })
  useEffect(() => {
    callbacks.current = { onChange, onRun, onSubmit }
  })

  useEffect(() => {
    if (!host.current) return
    const model = monaco.editor.createModel(initialDoc, 'c')
    const e = monaco.editor.create(host.current, {
      model,
      theme: 'vs-dark',
      readOnly,
      automaticLayout: true,
      minimap: { enabled: false },
      fixedOverflowWidgets: true,
      ariaLabel,
    })
    editor.current = e
    const decorations = e.createDecorationsCollection()
    ranges.current = blankRanges ?? []

    decorate.current = () => {
      const rs = ranges.current
      if (rs.length === 0) return decorations.clear()
      decorations.set([
        ...fixedRanges(model.getValue(), rs).map((r) => ({ range: rangeOf(model, r.from, r.to), options: { inlineClassName: 'fillin-locked' } })),
        ...rs.filter((r) => r.to > r.from).map((r) => ({ range: rangeOf(model, r.from, r.to), options: { inlineClassName: 'fillin-blank' } })),
      ])
    }
    decorate.current()

    e.addAction({
      id: 'ccal.run',
      label: '執行',
      keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter],
      run: () => callbacks.current.onRun?.(),
    })
    e.addAction({
      id: 'ccal.submit',
      label: '提交',
      keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.Enter],
      run: () => callbacks.current.onSubmit?.(),
    })

    const sub = model.onDidChangeContent((ev) => {
      if (programmatic.current) return
      const rs = ranges.current
      if (rs.length > 0) {
        // Edits outside the blanks are reverted. Each accepted edit closes its undo
        // group, so the undo below removes exactly the rejected edit.
        if (!ev.isUndoing && !ev.isRedoing && !ev.changes.every((c) => changeAllowed(c.rangeOffset, c.rangeOffset + c.rangeLength, rs))) {
          programmatic.current = true
          model.undo()
          programmatic.current = false
          return
        }
        ranges.current = [...ev.changes]
          .sort((a, b) => b.rangeOffset - a.rangeOffset)
          .reduce((acc, c) => applyEdit(acc, c.rangeOffset, c.rangeOffset + c.rangeLength, c.text.length), rs)
        model.pushStackElement()
        decorate.current()
      }
      callbacks.current.onChange?.(model.getValue())
    })

    return () => {
      sub.dispose()
      e.dispose()
      model.dispose()
      editor.current = null
    }
    // The editor is created once; later updates go through the imperative handle.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [readOnly])

  useEffect(() => {
    const model = editor.current?.getModel()
    if (!model) return
    const last = model.getLineCount()
    monaco.editor.setModelMarkers(
      model,
      'ccal',
      (diagnostics ?? []).map((d) => {
        const line = Math.min(Math.max(d.line, 1), last)
        const endLine = Math.min(Math.max(d.endLine ?? line, line), last)
        const endColumn = d.endLine ? (d.endColumn ?? d.column) + 1 : d.column + 1
        return {
          startLineNumber: line,
          startColumn: d.column,
          endLineNumber: endLine,
          endColumn: endLine === line ? Math.max(endColumn, d.column + 1) : endColumn,
          severity: severities[d.severity],
          message: d.message,
        }
      }),
    )
  }, [diagnostics])

  useImperativeHandle(
    ref,
    () => ({
      jumpTo(line, column = 1) {
        const e = editor.current
        const model = e?.getModel()
        if (!e || !model) return
        const l = Math.min(Math.max(line, 1), model.getLineCount())
        e.setSelection({ startLineNumber: l, startColumn: 1, endLineNumber: l, endColumn: model.getLineMaxColumn(l) })
        e.revealPositionInCenter({ lineNumber: l, column })
        e.focus()
      },
      getDoc: () => editor.current?.getValue() ?? '',
      getBlanks: () => extractBlanks(editor.current?.getValue() ?? '', ranges.current),
      reset(doc, rs) {
        const e = editor.current
        const model = e?.getModel()
        if (!e || !model) return
        programmatic.current = true
        ranges.current = rs ?? []
        // Fill-in ranges cannot follow an undo past the reset, so drop the history there.
        if (ranges.current.length > 0) model.setValue(doc)
        else {
          e.pushUndoStop()
          e.executeEdits('reset', [{ range: model.getFullModelRange(), text: doc }])
          e.pushUndoStop()
        }
        programmatic.current = false
        decorate.current()
        callbacks.current.onChange?.(model.getValue())
      },
      focus: () => editor.current?.focus(),
    }),
    [],
  )

  return <div ref={host} className={className} data-testid="code-editor" />
})
