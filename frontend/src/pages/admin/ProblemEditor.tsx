import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ArrowDown, ArrowUp, CheckCircle2, History, Plus, Save, Send, Trash2, Undo2, Upload } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { toast } from 'sonner'

import { ApiError, api } from '@/api/client'
import type { ProblemContent, ProblemDetail, RevisionDetail, RevisionSummary, SubmissionView, TestCaseInput, ValidationReport } from '@/api/types'
import { ConceptPicker, Field, SectionCard } from '@/components/admin/Field'
import { Markdown } from '@/components/Markdown'
import { ErrorState, FieldMessage, Loading, Page, PageHeader } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { SubmissionResult } from '@/components/SubmissionResult'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Textarea } from '@/components/ui/textarea'
import { useUnsavedGuard } from '@/hooks/useUnsaved'
import { errorMessage, errorsByPointer, errorsUnder } from '@/lib/errors'
import { STATUS_LABELS, formatTime } from '@/lib/format'

import { problemStatusTone } from './Problems'

const BLANK_RE = /\/\*@blank ([a-z][a-z0-9_]{0,31})@\*\//g

function emptyContent(): ProblemContent {
  return {
    title: '',
    statement_md: '> CYCU-CAL 自建教材\n\n',
    examples: [],
    mode: 'full',
    starter_code: '#include <stdio.h>\n\nint main(void) {\n    \n    return 0;\n}\n',
    template: null,
    limits: { time_ms: 1000, memory_kb: 65536, output_kb: 64 },
    concepts: ['io', 'variables'],
    compare_mode: 'trim_trailing_whitespace',
    reference_solution: '',
    reference_blanks: null,
    tests: [
      { name: '範例', visibility: 'public', input: '', expected_output: '' },
      { name: null, visibility: 'hidden', input: '', expected_output: '' },
    ],
  }
}

/** Normalises optional fields so the request body only carries what the mode uses. */
function toRequest(c: ProblemContent): ProblemContent {
  const fill = c.mode === 'fill_in'
  return {
    ...c,
    template: fill ? (c.template ?? '') : null,
    starter_code: fill ? '' : (c.starter_code ?? ''),
    reference_solution: fill ? null : c.reference_solution || null,
    reference_blanks: fill ? (c.reference_blanks ?? {}) : null,
    tests: c.tests.map((t) => ({ ...t, name: t.name || null })),
  }
}

const mono = 'font-mono text-sm'

function TestsEditor({ tests, onChange, errors }: { tests: TestCaseInput[]; onChange: (t: TestCaseInput[]) => void; errors: Record<string, string> }) {
  const set = (i: number, patch: Partial<TestCaseInput>) => onChange(tests.map((t, j) => (j === i ? { ...t, ...patch } : t)))
  const move = (i: number, d: number) => {
    const n = [...tests]
    const [x] = n.splice(i, 1)
    n.splice(i + d, 0, x)
    onChange(n)
  }
  return (
    <div className="space-y-3">
      {tests.map((t, i) => (
        <div key={i} className="rounded-md border p-3" data-testid="test-row">
          <div className="mb-2 flex flex-wrap items-center gap-2">
            <span className="text-sm font-medium tabular-nums">#{i + 1}</span>
            <Input aria-label={`測資 ${i + 1} 名稱`} placeholder="名稱（選填）" className="h-8 max-w-48" value={t.name ?? ''} onChange={(e) => set(i, { name: e.target.value })} />
            <Select value={t.visibility ?? 'public'} onValueChange={(v) => set(i, { visibility: v as TestCaseInput['visibility'] })}>
              <SelectTrigger className="h-8 w-32" aria-label={`測資 ${i + 1} 公開或隱藏`}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="public">公開</SelectItem>
                <SelectItem value="hidden">隱藏</SelectItem>
              </SelectContent>
            </Select>
            <div className="ml-auto flex gap-1">
              <Button variant="ghost" size="icon" aria-label="上移" disabled={i === 0} onClick={() => move(i, -1)}>
                <ArrowUp aria-hidden />
              </Button>
              <Button variant="ghost" size="icon" aria-label="下移" disabled={i === tests.length - 1} onClick={() => move(i, 1)}>
                <ArrowDown aria-hidden />
              </Button>
              <Button variant="ghost" size="icon" aria-label={`刪除測資 ${i + 1}`} onClick={() => onChange(tests.filter((_, j) => j !== i))}>
                <Trash2 aria-hidden />
              </Button>
            </div>
          </div>
          <div className="grid gap-2 md:grid-cols-2">
            <Field label="輸入" error={errors[`/tests/${i}/input`]}>
              <Textarea className={mono} rows={3} value={t.input} onChange={(e) => set(i, { input: e.target.value })} />
            </Field>
            <Field label="預期輸出" error={errors[`/tests/${i}/expected_output`]}>
              <Textarea className={mono} rows={3} value={t.expected_output} onChange={(e) => set(i, { expected_output: e.target.value })} />
            </Field>
          </div>
          <label className="mt-2 flex items-center gap-2 text-xs text-muted-foreground">
            <input
              type="file"
              accept=".txt,.in,.out"
              className="sr-only"
              onChange={async (e) => {
                const f = e.target.files?.[0]
                if (f) set(i, { input: await f.text() })
              }}
            />
            <Upload aria-hidden className="size-3.5" />
            <span className="cursor-pointer underline">從檔案載入輸入</span>
          </label>
        </div>
      ))}
      <Button variant="outline" size="sm" onClick={() => onChange([...tests, { name: null, visibility: 'hidden', input: '', expected_output: '' }])}>
        <Plus aria-hidden />
        新增測資
      </Button>
    </div>
  )
}

function RevisionsDialog({ slug, open, onOpenChange }: { slug: string; open: boolean; onOpenChange: (o: boolean) => void }) {
  const [view, setView] = useState<number | null>(null)
  const list = useQuery({ queryKey: ['revisions', slug], queryFn: () => api.get<RevisionSummary[]>(`/admin/problems/${slug}/revisions`), enabled: open })
  const rev = useQuery({ queryKey: ['revision', slug, view], queryFn: () => api.get<RevisionDetail>(`/admin/problems/${slug}/revisions/${view}`), enabled: open && view != null })
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-dialog overflow-y-auto sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>版本紀錄</DialogTitle>
        </DialogHeader>
        <p className="text-sm text-muted-foreground">每次儲存都會建立一個不可修改的版本。學生的提交會記錄它是用哪個版本評測的。</p>
        <ul className="divide-y rounded-md border text-sm">
          {list.data?.map((r) => (
            <li key={r.number} className="flex flex-wrap items-center gap-3 px-3 py-2">
              <span className="font-medium tabular-nums">版本 {r.number}</span>
              {r.published && <StatusPill tone="success" label="學生目前看到" />}
              <span className="text-muted-foreground">{formatTime(r.created_at)}</span>
              <span className="text-muted-foreground">提交 {r.submission_count} 筆</span>
              <Button size="sm" variant="ghost" className="ml-auto" onClick={() => setView(r.number)}>
                查看
              </Button>
            </li>
          ))}
        </ul>
        {rev.data && (
          <div className="space-y-2">
            <h3 className="font-medium">版本 {rev.data.number}：{rev.data.content.title}</h3>
            <pre className="max-h-80 overflow-auto rounded-md bg-muted p-3 font-mono text-xs">{JSON.stringify(rev.data.content, null, 2)}</pre>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}

function TestRunPanel({ slug, mode, blankIds }: { slug: string; mode: string; blankIds: string[] }) {
  const [code, setCode] = useState('')
  const [blanks, setBlanks] = useState<Record<string, string>>({})
  const [result, setResult] = useState<SubmissionView | null>(null)
  const run = useMutation({
    mutationFn: (useReference: boolean) =>
      api.post<SubmissionView>(`/admin/problems/${slug}/test-run`, useReference ? { use_reference: true } : mode === 'fill_in' ? { blanks } : { code }),
    onSuccess: setResult,
    onError: (e) => toast.error(errorMessage(e)),
  })
  return (
    <SectionCard title="試跑" description="用最新儲存的版本、包含隱藏測資，看到完整結果。">
      {mode === 'fill_in' ? (
        blankIds.map((id) => (
          <Field key={id} label={`區塊 ${id}`}>
            <Textarea className={mono} rows={4} value={blanks[id] ?? ''} onChange={(e) => setBlanks({ ...blanks, [id]: e.target.value })} />
          </Field>
        ))
      ) : (
        <Field label="程式">
          <Textarea className={mono} rows={8} value={code} onChange={(e) => setCode(e.target.value)} placeholder="貼上要測試的 C 程式（例如常見的錯誤寫法）" />
        </Field>
      )}
      <div className="flex gap-2">
        <Button variant="outline" onClick={() => run.mutate(false)} disabled={run.isPending}>
          試跑這段程式
        </Button>
        <Button variant="ghost" onClick={() => run.mutate(true)} disabled={run.isPending}>
          試跑參考解答
        </Button>
      </div>
      {run.isPending && <p className="text-sm text-muted-foreground">評測中…</p>}
      {result && <SubmissionResult submission={result} />}
    </SectionCard>
  )
}

export function ProblemEditorPage() {
  const { slug: routeSlug } = useParams()
  const isNew = !routeSlug
  const nav = useNavigate()
  const qc = useQueryClient()
  const detail = useQuery({ queryKey: ['admin-problem', routeSlug], queryFn: () => api.get<ProblemDetail>(`/admin/problems/${routeSlug}`), enabled: !isNew })
  const [slug, setSlug] = useState('')
  const [content, setContent] = useState<ProblemContent | null>(isNew ? emptyContent() : null)
  const [baseline, setBaseline] = useState<string>(isNew ? JSON.stringify(emptyContent()) : '')
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [report, setReport] = useState<ValidationReport | null>(null)
  const [revOpen, setRevOpen] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)

  useEffect(() => {
    if (detail.data) {
      setContent(detail.data.head_revision.content)
      setBaseline(JSON.stringify(detail.data.head_revision.content))
    }
  }, [detail.data])

  const dirty = !!content && JSON.stringify(content) !== baseline
  useUnsavedGuard(dirty)
  const blankIds = useMemo(() => [...(content?.template ?? '').matchAll(BLANK_RE)].map((m) => m[1]), [content?.template])

  if (!isNew && detail.isLoading) return <Loading />
  if (!isNew && detail.error) return <Page><ErrorState error={detail.error} /></Page>
  if (!content) return <Loading />
  const d = detail.data
  const set = (patch: Partial<ProblemContent>) => setContent({ ...content, ...patch })

  const save = async () => {
    setBusy('save')
    setErrors({})
    try {
      if (isNew) {
        const r = await api.post<ProblemDetail>('/admin/problems', { slug, content: toRequest(content) }, { headers: { 'Idempotency-Key': crypto.randomUUID() } })
        setBaseline(JSON.stringify(content))
        toast.success('題目已建立（草稿）')
        qc.invalidateQueries({ queryKey: ['admin-problems'] })
        nav(`/admin/problems/${r.slug}`, { replace: true })
      } else {
        const r = await api.put<ProblemDetail>(`/admin/problems/${routeSlug}`, { content: toRequest(content) }, { headers: { 'If-Match': String(d!.head_revision.number) } })
        qc.setQueryData(['admin-problem', routeSlug], r)
        toast.success(r.revision_created ? `已儲存為版本 ${r.head_revision.number}` : '內容沒有變更')
      }
    } catch (e) {
      if (e instanceof ApiError && e.status === 412) {
        toast.error(`${e.message}（你的修改還在畫面上，可以先複製下來）`)
      } else {
        setErrors({ ...errorsByPointer(e instanceof ApiError ? e.errors : [], '/content'), ...(e instanceof ApiError ? errorsByPointer(e.errors.filter((x) => x.pointer === '/slug')) : {}) })
        toast.error(errorMessage(e))
      }
    } finally {
      setBusy(null)
    }
  }

  const action = async (kind: 'validate' | 'publish' | 'unpublish' | 'archive') => {
    setBusy(kind)
    try {
      if (kind === 'validate') {
        const r = await api.post<ValidationReport>(`/admin/problems/${routeSlug}/validate`, {})
        setReport(r)
        setErrors(errorsByPointer(r.errors, '/content'))
        toast[r.ok ? 'success' : 'error'](r.ok ? '驗證通過，可以發布' : '驗證沒有通過，請看下方說明')
      } else if (kind === 'archive') {
        if (!window.confirm('封存後學生將看不到這一題，過去的提交紀錄會保留。確定要封存嗎？')) return
        await api.del(`/admin/problems/${routeSlug}`)
        toast.success('已封存')
        qc.invalidateQueries({ queryKey: ['admin-problem', routeSlug] })
      } else {
        const r = await api.post<ProblemDetail>(`/admin/problems/${routeSlug}/${kind}`, {})
        qc.setQueryData(['admin-problem', routeSlug], r)
        toast.success(kind === 'publish' ? `已發布版本 ${r.published_revision_number}` : '已取消發布')
      }
    } catch (e) {
      if (e instanceof ApiError) setErrors(errorsByPointer(e.errors, '/content'))
      toast.error(errorMessage(e))
    } finally {
      setBusy(null)
    }
  }

  const e = errors
  return (
    <Page wide>
      <PageHeader
        back={
          <Link to="/admin/problems" className="text-sm text-muted-foreground hover:text-foreground">
            題目庫
          </Link>
        }
        title={isNew ? '新增題目' : content.title || routeSlug}
        description={
          d && (
            <span className="flex flex-wrap items-center gap-2 text-sm">
              <StatusPill tone={problemStatusTone(d.status)} label={STATUS_LABELS[d.status] ?? d.status} />
              最新版本 {d.head_revision.number}
              {d.published_revision_number != null ? `，學生看到版本 ${d.published_revision_number}` : '，尚未發布'}
              {d.sets.length > 0 && `，用於 ${d.sets.map((s) => s.title).join('、')}`}
            </span>
          )
        }
        actions={
          !isNew && (
            <Button variant="ghost" size="sm" onClick={() => setRevOpen(true)}>
              <History aria-hidden />
              版本紀錄
            </Button>
          )
        }
      />

      <div className="sticky top-12 z-20 -mx-4 mb-6 flex flex-wrap items-center gap-2 border-b bg-background/95 px-4 py-2 backdrop-blur sm:-mx-6 sm:px-6">
        <Button onClick={save} disabled={busy !== null || (!dirty && !isNew)} data-testid="save-problem">
          <Save aria-hidden />
          {busy === 'save' ? '儲存中…' : isNew ? '建立草稿' : '儲存新版本'}
        </Button>
        {!isNew && (
          <>
            <Button variant="outline" onClick={() => action('validate')} disabled={busy !== null || dirty}>
              <CheckCircle2 aria-hidden />
              {busy === 'validate' ? '驗證中…' : '驗證'}
            </Button>
            <Button variant="outline" onClick={() => action('publish')} disabled={busy !== null || dirty}>
              <Send aria-hidden />
              {busy === 'publish' ? '驗證並發布中…' : '發布最新版本'}
            </Button>
            {d?.status === 'published' && (
              <Button variant="ghost" onClick={() => action('unpublish')} disabled={busy !== null}>
                <Undo2 aria-hidden />
                取消發布
              </Button>
            )}
            <Button variant="ghost" className="text-destructive" onClick={() => action('archive')} disabled={busy !== null || d?.status === 'archived'}>
              <Trash2 aria-hidden />
              封存
            </Button>
          </>
        )}
        <span className="ml-auto text-sm text-muted-foreground" role="status">
          {dirty ? '有尚未儲存的修改' : isNew ? '' : '已儲存'}
        </span>
      </div>

      {report && (
        <div role="status" className={`mb-6 rounded-lg border p-4 ${report.ok ? 'border-success/30 bg-success-soft' : 'border-destructive/30 bg-danger-soft'}`}>
          <p className="font-medium">{report.ok ? `版本 ${report.revision_number} 驗證通過：參考解答通過所有測資。` : `版本 ${report.revision_number} 還不能發布：`}</p>
          {report.errors.length > 0 && (
            <ul className="mt-2 list-disc pl-5 text-sm">
              {report.errors.map((x, i) => (
                <li key={i}>
                  <span className="font-mono text-xs">{x.pointer}</span>：{x.message}
                </li>
              ))}
            </ul>
          )}
          {report.reference_run && (
            <details className="mt-2">
              <summary className="cursor-pointer text-sm">參考解答的評測結果</summary>
              <div className="mt-2">
                <SubmissionResult submission={report.reference_run as unknown as SubmissionView} />
              </div>
            </details>
          )}
        </div>
      )}

      <div className="space-y-6">
        <SectionCard title="基本資料">
          {isNew && (
            <Field label="Slug（穩定識別碼，建立後不能修改）" htmlFor="slug" error={e['/slug']} hint="小寫英文、數字與連字號，例如 a-plus-b">
              <Input id="slug" className="max-w-sm font-mono" value={slug} onChange={(ev) => setSlug(ev.target.value)} />
            </Field>
          )}
          <Field label="題目名稱" htmlFor="title" error={e['/title']}>
            <Input id="title" value={content.title} onChange={(ev) => set({ title: ev.target.value })} />
          </Field>
          <div className="grid gap-4 md:grid-cols-3">
            <Field label="作答方式" error={e['/mode']}>
              <Select value={content.mode ?? 'full'} onValueChange={(v) => set({ mode: v as ProblemContent['mode'], template: v === 'fill_in' ? (content.template ?? `${content.starter_code ?? ''}`) : content.template })}>
                <SelectTrigger aria-label="作答方式">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="full">完整程式</SelectItem>
                  <SelectItem value="fill_in">局部補寫</SelectItem>
                </SelectContent>
              </Select>
            </Field>
            <Field label="輸出比對方式">
              <Select value={content.compare_mode ?? 'trim_trailing_whitespace'} onValueChange={(v) => set({ compare_mode: v as ProblemContent['compare_mode'] })}>
                <SelectTrigger aria-label="輸出比對方式">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="trim_trailing_whitespace">忽略行尾空白與結尾空行</SelectItem>
                  <SelectItem value="exact">完全相同</SelectItem>
                </SelectContent>
              </Select>
            </Field>
          </div>
          <div className="grid gap-4 md:grid-cols-3">
            <Field label="時間限制（毫秒）" htmlFor="tl" error={e['/limits/time_ms']}>
              <Input id="tl" type="number" value={content.limits?.time_ms ?? 1000} onChange={(ev) => set({ limits: { ...content.limits!, time_ms: Number(ev.target.value) } })} />
            </Field>
            <Field label="記憶體限制（KiB）" htmlFor="ml" error={e['/limits/memory_kb']}>
              <Input id="ml" type="number" value={content.limits?.memory_kb ?? 65536} onChange={(ev) => set({ limits: { ...content.limits!, memory_kb: Number(ev.target.value) } })} />
            </Field>
            <Field label="輸出量限制（KiB）" htmlFor="ol" error={e['/limits/output_kb']}>
              <Input id="ol" type="number" value={content.limits?.output_kb ?? 64} onChange={(ev) => set({ limits: { ...content.limits!, output_kb: Number(ev.target.value) } })} />
            </Field>
          </div>
          <Field label="這一題假設學生已學過的概念（影響錯誤說明的措辭）" error={errorsUnder(e, '/concepts')[0]}>
            <ConceptPicker idPrefix="pc" value={content.concepts ?? []} onChange={(concepts) => set({ concepts })} />
          </Field>
        </SectionCard>

        <SectionCard title="題敘" description="支援 Markdown（不允許 HTML）。">
          <Tabs defaultValue="edit">
            <TabsList>
              <TabsTrigger value="edit">編輯</TabsTrigger>
              <TabsTrigger value="preview">預覽</TabsTrigger>
            </TabsList>
            <TabsContent value="edit">
              <Textarea aria-label="題敘 Markdown" rows={12} value={content.statement_md} onChange={(ev) => set({ statement_md: ev.target.value })} />
            </TabsContent>
            <TabsContent value="preview" className="rounded-md border p-4">
              <Markdown>{content.statement_md}</Markdown>
            </TabsContent>
          </Tabs>
          <FieldMessage message={e['/statement_md']} />
          <div className="space-y-2">
            <div className="text-sm font-medium">範例</div>
            {(content.examples ?? []).map((ex, i) => (
              <div key={i} className="grid gap-2 rounded-md border p-3 md:grid-cols-pair-action">
                <Textarea aria-label={`範例 ${i + 1} 輸入`} className={mono} rows={2} placeholder="輸入" value={ex.input} onChange={(ev) => set({ examples: content.examples!.map((x, j) => (j === i ? { ...x, input: ev.target.value } : x)) })} />
                <Textarea aria-label={`範例 ${i + 1} 輸出`} className={mono} rows={2} placeholder="輸出" value={ex.output} onChange={(ev) => set({ examples: content.examples!.map((x, j) => (j === i ? { ...x, output: ev.target.value } : x)) })} />
                <Button variant="ghost" size="icon" aria-label={`刪除範例 ${i + 1}`} onClick={() => set({ examples: content.examples!.filter((_, j) => j !== i) })}>
                  <Trash2 aria-hidden />
                </Button>
              </div>
            ))}
            <Button variant="outline" size="sm" onClick={() => set({ examples: [...(content.examples ?? []), { input: '', output: '' }] })}>
              <Plus aria-hidden />
              新增範例
            </Button>
          </div>
        </SectionCard>

        <SectionCard title={content.mode === 'fill_in' ? '補寫範本與參考解答' : '起始程式與參考解答'} description="參考解答只有管理者看得到，發布前系統會用它跑過所有測資。">
          {content.mode === 'fill_in' ? (
            <>
              <Field
                label="範本"
                error={e['/template']}
                hint="用 /*@blank 名稱@*/ 預設內容 /*@end@*/ 標出學生可以修改的區塊，其餘部分學生不能修改。"
              >
                <Textarea className={mono} rows={14} value={content.template ?? ''} onChange={(ev) => set({ template: ev.target.value })} />
              </Field>
              <p className="text-sm text-muted-foreground">偵測到的區塊：{blankIds.length ? blankIds.join('、') : '（沒有）'}</p>
              {blankIds.map((id) => (
                <Field key={id} label={`參考解答：區塊 ${id}`} error={e['/reference_blanks']}>
                  <Textarea className={mono} rows={4} value={content.reference_blanks?.[id] ?? ''} onChange={(ev) => set({ reference_blanks: { ...(content.reference_blanks ?? {}), [id]: ev.target.value } })} />
                </Field>
              ))}
            </>
          ) : (
            <>
              <Field label="起始程式（學生打開題目時看到的內容）" error={e['/starter_code']}>
                <Textarea className={mono} rows={10} value={content.starter_code ?? ''} onChange={(ev) => set({ starter_code: ev.target.value })} />
              </Field>
              <Field label="參考解答" error={e['/reference_solution']}>
                <Textarea className={mono} rows={12} value={content.reference_solution ?? ''} onChange={(ev) => set({ reference_solution: ev.target.value })} />
              </Field>
            </>
          )}
        </SectionCard>

        <SectionCard title={`測資（${content.tests.length} 筆）`} description="公開測資：學生按「執行」時使用，看得到內容。隱藏測資：只在「提交」時使用，學生只會看到通過與否。">
          <FieldMessage message={e['/tests']} />
          <TestsEditor tests={content.tests} onChange={(tests) => set({ tests })} errors={e} />
        </SectionCard>

        {!isNew && routeSlug && <TestRunPanel slug={routeSlug} mode={content.mode ?? 'full'} blankIds={blankIds} />}
      </div>
      {!isNew && routeSlug && <RevisionsDialog slug={routeSlug} open={revOpen} onOpenChange={setRevOpen} />}
    </Page>
  )
}
