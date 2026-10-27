import { useQuery, useQueryClient } from '@tanstack/react-query'
import { ArrowLeftRight, CloudOff, FileCheck2, Info, Play, RotateCcw, Send, TriangleAlert } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Link, useParams } from 'react-router'
import { toast } from 'sonner'

import { api } from '@/api/client'
import type { ProblemView, SetDetail, Span, SubmissionView } from '@/api/types'
import { CodeEditor, type CodeEditorHandle, type EditorDiagnostic } from '@/components/CodeEditor'
import { Markdown } from '@/components/Markdown'
import { ErrorState, Loading, Page } from '@/components/Page'
import { SubmissionResult } from '@/components/SubmissionResult'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable'
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Textarea } from '@/components/ui/textarea'
import { useAuth } from '@/hooks/auth'
import { useDraft } from '@/hooks/useDraft'
import { useSite } from '@/hooks/useSite'
import { useMediaQuery, useSubmission } from '@/hooks/useSubmission'
import { saveLabel } from '@/lib/autosave'
import { errorMessage } from '@/lib/errors'
import { buildDoc } from '@/lib/fillin'
import { cn } from '@/lib/utils'

function Statement({ p }: { p: ProblemView }) {
  return (
    <article className="space-y-5 p-5" aria-labelledby="problem-title">
      <div className="space-y-1">
        <h1 id="problem-title" className="text-xl font-medium">
          {p.title}
        </h1>
        <p className="text-sm text-muted-foreground">
          時間限制 {p.limits.time_ms / 1000} 秒，記憶體 {Math.round(p.limits.memory_kb / 1024)} MB
          {p.mode === 'fill_in' && '，局部補寫題：只需要完成藍色底線的區塊'}
        </p>
      </div>
      <Markdown>{p.statement_md}</Markdown>
      {p.examples.length > 0 && (
        <section className="space-y-3">
          <h2 className="font-medium">範例</h2>
          {p.examples.map((ex, i) => (
            <div key={i} className="space-y-2">
              <div className="grid gap-2 sm:grid-cols-2">
                <div>
                  <div className="mb-1 text-xs text-muted-foreground">輸入</div>
                  <pre className="rounded-md border bg-code p-2 font-mono text-sm whitespace-pre-wrap">{ex.input || '（沒有輸入）'}</pre>
                </div>
                <div>
                  <div className="mb-1 text-xs text-muted-foreground">輸出</div>
                  <pre className="rounded-md border bg-code p-2 font-mono text-sm whitespace-pre-wrap">{ex.output}</pre>
                </div>
              </div>
              {ex.explanation_md && <Markdown className="text-sm text-muted-foreground">{ex.explanation_md}</Markdown>}
            </div>
          ))}
        </section>
      )}
      <section className="space-y-1.5 rounded-lg bg-muted/60 p-3 text-sm">
        <p className="flex gap-1.5">
          <Info aria-hidden className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
          <span>
            「執行」會用 {p.public_tests.length} 筆公開測資（或你的自訂輸入）；{p.hidden_test_count > 0 ? `「提交」會再加上 ${p.hidden_test_count} 筆隱藏測資正式評測。` : '「提交」會用同樣的公開測資正式評測。'}
          </span>
        </p>
        <p className="pl-5.5 text-muted-foreground">{p.compare_description}</p>
      </section>
    </article>
  )
}

function SaveStatus({ label, offline, conflict }: { label: string; offline: boolean; conflict: boolean }) {
  return (
    <span role="status" className={cn('inline-flex items-center gap-1 text-xs', offline || conflict ? 'text-warning' : 'text-muted-foreground')}>
      {offline ? <CloudOff aria-hidden className="size-3.5" /> : conflict ? <TriangleAlert aria-hidden className="size-3.5" /> : <FileCheck2 aria-hidden className="size-3.5" />}
      {label}
    </span>
  )
}

export function ProblemPage() {
  const { setId, slug = '' } = useParams()
  const { me } = useAuth()
  const qc = useQueryClient()
  const site = useSite()
  const desktop = useMediaQuery('(min-width: 1024px)')
  const problem = useQuery({ queryKey: ['problem', slug], queryFn: () => api.get<ProblemView>(`/problems/${slug}`) })
  const set = useQuery({ queryKey: ['set', setId], queryFn: () => api.get<SetDetail>(`/problem-sets/${setId}`), enabled: !!setId })
  const p = problem.data
  const draft = useDraft(me?.user.id, p?.id)
  const editor = useRef<CodeEditorHandle>(null)
  const [subId, setSubId] = useState<string | null>(null)
  const { submission, timedOut, revealHint, rejudge } = useSubmission(subId)
  const [busy, setBusy] = useState<null | 'run' | 'submit'>(null)
  const [useCustom, setUseCustom] = useState(false)
  const [stdin, setStdin] = useState('')
  const [tab, setTab] = useState('editor')
  const [confirmReset, setConfirmReset] = useState(false)

  // The document survives layout switches (the editor remounts between desktop and tabs).
  const doc = useRef<{ code: string; blanks: Record<string, string> | null } | null>(null)
  const initial = useMemo(() => {
    if (!p || draft.loaded === undefined) return null
    if (p.mode === 'fill_in' && p.segments) {
      const built = buildDoc(p.segments, draft.loaded?.blanks ?? undefined)
      return { doc: built.doc, ranges: built.ranges }
    }
    return { doc: draft.loaded?.code || p.initial_code, ranges: undefined }
  }, [p, draft.loaded])

  const onChange = useCallback(
    (code: string) => {
      const blanks = p?.mode === 'fill_in' ? (editor.current?.getBlanks() ?? null) : null
      doc.current = { code, blanks }
      draft.update({ code: p?.mode === 'fill_in' ? '' : code, blanks })
    },
    [p?.mode, draft],
  )

  // Remember which set was active when the page opened, to announce switches.
  const initialActive = useRef<string | null | undefined>(undefined)
  useEffect(() => {
    if (site.data && initialActive.current === undefined) initialActive.current = site.data.active_set_id ?? null
  }, [site.data])
  const activeChanged =
    site.data && initialActive.current !== undefined && site.data.active_set_id !== initialActive.current && site.data.active_set_id !== setId
  const activeSet = useQuery({
    queryKey: ['set', site.data?.active_set_id],
    queryFn: () => api.get<SetDetail>(`/problem-sets/${site.data!.active_set_id}`),
    enabled: !!activeChanged && !!site.data?.active_set_id,
  })

  const send = useCallback(
    async (kind: 'run' | 'submit') => {
      if (!p || busy) return
      const code = editor.current?.getDoc() ?? ''
      const body: Record<string, unknown> = { problem_id: p.id, set_id: setId ?? null, kind }
      if (p.mode === 'fill_in') body.blanks = editor.current?.getBlanks() ?? {}
      else body.code = code
      if (kind === 'run' && useCustom) body.stdin = stdin
      setBusy(kind)
      try {
        void draft.flush()
        const s = await api.post<SubmissionView>('/submissions', body, { headers: { 'Idempotency-Key': crypto.randomUUID() } })
        qc.setQueryData(['submission', s.id], s)
        setSubId(s.id)
        setTab('result')
      } catch (e) {
        toast.error(errorMessage(e))
      } finally {
        setBusy(null)
      }
    },
    [p, busy, setId, useCustom, stdin, draft, qc],
  )

  const onJump = useCallback(
    (span: Span) => {
      setTab('editor')
      // Wait for the tab to show the editor on narrow screens.
      window.setTimeout(() => editor.current?.jumpTo(span.line, span.column), desktop ? 0 : 50)
    },
    [desktop],
  )

  const diagnostics: EditorDiagnostic[] = useMemo(
    () =>
      (submission?.cards ?? [])
        .filter((c) => c.span && c.maybe_cascade_of == null)
        .map((c) => ({
          line: c.span!.line,
          column: c.span!.column,
          endLine: c.span!.end_line,
          endColumn: c.span!.end_column,
          severity: c.severity === 'error' ? 'error' : c.severity === 'warning' ? 'warning' : 'info',
          message: c.title.replace(/`/g, ''),
        })),
    [submission],
  )

  const resolveConflict = (keepMine: boolean) => {
    const c = draft.resolve(keepMine)
    if (c && p) {
      if (p.mode === 'fill_in' && p.segments) {
        const built = buildDoc(p.segments, c.blanks ?? undefined)
        editor.current?.reset(built.doc, built.ranges)
      } else editor.current?.reset(c.code)
    }
  }

  const resetToStarter = () => {
    if (!p) return
    if (p.mode === 'fill_in' && p.segments) {
      const built = buildDoc(p.segments)
      editor.current?.reset(built.doc, built.ranges)
    } else editor.current?.reset(p.initial_code)
    setConfirmReset(false)
  }

  if (problem.isLoading) return <Loading />
  if (problem.error)
    return (
      <Page>
        <ErrorState error={problem.error} />
      </Page>
    )
  if (!p || !initial) return <Loading label="載入你的程式草稿…" />

  const startDoc = doc.current ? (p.mode === 'fill_in' && p.segments ? buildDoc(p.segments, doc.current.blanks ?? undefined) : { doc: doc.current.code, ranges: undefined }) : initial
  const s = draft.state

  const toolbar = (
    <div className="flex flex-wrap items-center gap-2 border-b bg-card px-3 py-2">
      <SaveStatus label={saveLabel(s)} offline={s.kind === 'offline'} conflict={s.kind === 'conflict'} />
      <Button variant="ghost" size="sm" className="ml-auto" aria-label="恢復起始程式" onClick={() => setConfirmReset(true)}>
        <RotateCcw aria-hidden />
        <span className="hidden xl:inline">恢復起始程式</span>
      </Button>
      <Button variant="outline" size="sm" onClick={() => send('run')} disabled={!!busy} title="Ctrl + Enter" data-testid="run-button">
        <Play aria-hidden />
        {busy === 'run' ? '送出中…' : '執行'}
      </Button>
      <Button size="sm" onClick={() => send('submit')} disabled={!!busy} title="Ctrl + Shift + Enter" data-testid="submit-button">
        <Send aria-hidden />
        {busy === 'submit' ? '送出中…' : '提交'}
      </Button>
    </div>
  )

  const editorPane = (
    <div className="flex h-full min-h-0 flex-col">
      {toolbar}
      {s.kind === 'conflict' && (
        <div role="alert" className="flex flex-wrap items-center gap-2 border-b border-warning/30 bg-warning-soft px-3 py-2 text-sm">
          <ArrowLeftRight aria-hidden className="size-4 text-warning" />
          這一題的草稿在其他分頁或裝置被修改了。要保留哪一份？
          <Button size="sm" variant="outline" onClick={() => resolveConflict(true)}>
            保留目前這份
          </Button>
          <Button size="sm" variant="ghost" onClick={() => resolveConflict(false)}>
            載入另一份
          </Button>
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-hidden">
        <CodeEditor
          key={String(desktop)}
          ref={editor}
          initialDoc={startDoc.doc}
          blankRanges={startDoc.ranges}
          diagnostics={diagnostics}
          onChange={onChange}
          onRun={() => send('run')}
          onSubmit={() => send('submit')}
          className="h-full"
          ariaLabel={`${p.title} 的程式碼`}
        />
      </div>
      <div className="border-t bg-card px-3 py-2">
        <div className="flex items-center gap-2">
          <Switch id="custom-stdin" checked={useCustom} onCheckedChange={setUseCustom} />
          <Label htmlFor="custom-stdin" variant="option">
            用自己的輸入執行
          </Label>
          <span className="ml-auto hidden text-xs text-muted-foreground md:inline">Ctrl+Enter 執行 · Ctrl+Shift+Enter 提交</span>
        </div>
        {useCustom && (
          <Textarea
            aria-label="自訂輸入（stdin）"
            variant="code"
            className="mt-2 h-20"
            placeholder="在這裡輸入要給程式的資料，例如：3 5"
            value={stdin}
            onChange={(e) => setStdin(e.target.value)}
          />
        )}
      </div>
    </div>
  )

  const resultPane = (
    <div className="h-full overflow-y-auto p-4" aria-label="執行結果">
      {submission ? (
        <SubmissionResult submission={submission} onJump={onJump} onRevealHint={revealHint} onRejudge={rejudge} timedOut={timedOut} />
      ) : (
        <div className="space-y-2 pt-6 text-muted-foreground">
          <p>按「執行」用公開測資檢查你的程式，結果和說明會出現在這裡。</p>
          <p className="text-sm">如果有錯誤，系統會告訴你從哪裡看起，並一步一步給提示。</p>
        </div>
      )}
    </div>
  )

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b px-4 py-1.5 text-sm">
        {setId && (
          <Link to={`/sets/${setId}`} className="text-muted-foreground hover:text-foreground">
            {set.data?.title ?? '題目集'}
          </Link>
        )}
        <span aria-hidden className="text-muted-foreground">/</span>
        <span className="font-medium">{p.title}</span>
      </div>
      {activeChanged && (
        <div role="status" className="flex flex-wrap items-center gap-2 border-b border-primary/30 bg-primary-soft px-4 py-2 text-sm" data-testid="active-set-banner">
          <Info aria-hidden className="size-4 text-primary" />
          讀書會目前的題目集已改為「{activeSet.data?.title ?? '新的題目集'}」。你的程式碼已保留，正在評測的提交不受影響。
          <Link className="font-medium text-primary underline" to={`/sets/${site.data?.active_set_id}`}>
            前往新的題目集
          </Link>
        </div>
      )}
      {desktop ? (
        <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
          <ResizablePanel defaultSize="30%" minSize="18%">
            <div className="h-full overflow-y-auto">
              <Statement p={p} />
            </div>
          </ResizablePanel>
          <ResizableHandle withHandle aria-label="調整題目欄寬度" />
          <ResizablePanel defaultSize="40%" minSize="25%">
            {editorPane}
          </ResizablePanel>
          <ResizableHandle withHandle aria-label="調整結果欄寬度" />
          <ResizablePanel defaultSize="30%" minSize="20%">
            {resultPane}
          </ResizablePanel>
        </ResizablePanelGroup>
      ) : (
        <Tabs value={tab} onValueChange={setTab} variant="flush" className="flex min-h-0 flex-1 flex-col">
          <TabsList className="mx-3 my-2 w-auto">
            <TabsTrigger value="statement">題目</TabsTrigger>
            <TabsTrigger value="editor">程式</TabsTrigger>
            <TabsTrigger value="result">結果</TabsTrigger>
          </TabsList>
          <TabsContent value="statement" className="min-h-0 flex-1 overflow-y-auto">
            <Statement p={p} />
          </TabsContent>
          <TabsContent value="editor" forceMount className="min-h-editor flex-1 data-[state=inactive]:hidden">
            {editorPane}
          </TabsContent>
          <TabsContent value="result" className="min-h-0 flex-1 overflow-y-auto">
            {resultPane}
          </TabsContent>
        </Tabs>
      )}
      <AlertDialog open={confirmReset} onOpenChange={setConfirmReset}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>恢復成起始程式？</AlertDialogTitle>
            <AlertDialogDescription>目前編輯器裡的程式會被換成題目一開始提供的內容。你之前的提交紀錄不受影響。</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              onClick={resetToStarter}
            >
              恢復起始程式
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
