import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Plus, RotateCcw, Save, Trash2, Undo2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Link, useParams } from 'react-router'
import { toast } from 'sonner'

import { ApiError, api } from '@/api/client'
import type { RuleEditorView, RuleEntry, RuleVariant } from '@/api/types'
import { ConceptPicker, Field, SectionCard } from '@/components/admin/Field'
import { PreviewPanel, SAMPLES } from '@/components/admin/PreviewPanel'
import { StringList } from '@/components/admin/StringList'
import { ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { useUnsavedGuard } from '@/hooks/useUnsaved'
import { errorMessage, errorsByPointer } from '@/lib/errors'
import { templatePreview } from '@/lib/format'

const TEXT_FIELDS: Array<{ key: 'title' | 'what' | 'cause' | 'where' | 'why'; label: string; hint: string; rows: number }> = [
  { key: 'title', label: '標題', hint: '卡片最上方的一句話', rows: 1 },
  { key: 'what', label: '發生什麼事', hint: '編譯器確認的事實，用白話說明', rows: 3 },
  { key: 'cause', label: '可能原因', hint: '系統的推測；畫面會標示信心程度', rows: 3 },
  { key: 'where', label: '從哪裡看起', hint: '告訴學生該看哪一行', rows: 2 },
  { key: 'why', label: '為什麼會這樣', hint: '只補足理解這個問題需要的最少概念', rows: 3 },
]

/** Which preview sample best demonstrates a rule. */
function sampleFor(id: string) {
  if (id.includes('semicolon')) return 'semicolon'
  if (id.startsWith('format.scanf')) return 'scanf'
  if (id === 'ident.undeclared') return 'undeclared'
  if (id.startsWith('func.misspelled')) return 'printf'
  if (id.includes('brace')) return 'brace'
  if (id.includes('while_scanf')) return 'while-scanf'
  if (id.includes('fullwidth')) return 'fullwidth'
  return SAMPLES[0].id
}

function VariantEditor({ v, onChange, onRemove, errors, base }: { v: RuleVariant; onChange: (v: RuleVariant) => void; onRemove: () => void; errors: Record<string, string>; base: string }) {
  return (
    <div className="space-y-3 rounded-md border p-3">
      <div className="flex items-center">
        <span className="text-sm font-medium">當學生已學過以下全部概念時，改用這些文字</span>
        <Button variant="ghost" size="sm" className="ml-auto" onClick={onRemove}>
          <Trash2 aria-hidden />
          刪除這個版本
        </Button>
      </div>
      <ConceptPicker idPrefix={`${base}-c`} value={v.when_concepts} onChange={(when_concepts) => onChange({ ...v, when_concepts })} />
      {TEXT_FIELDS.map((f) => (
        <Field key={f.key} label={`${f.label}（留空沿用基本版本）`} error={errors[`${base}/${f.key}`]}>
          <Textarea rows={f.rows} value={v[f.key] ?? ''} onChange={(e) => onChange({ ...v, [f.key]: e.target.value || null })} />
        </Field>
      ))}
      <Field label="提示（留空沿用基本版本）">
        <StringList items={v.hints ?? []} onChange={(hints) => onChange({ ...v, hints: hints.length ? hints : null })} label={(i) => `提示 ${i + 1}`} addLabel="新增提示" errors={errors} basePointer={`${base}/hints`} />
      </Field>
    </div>
  )
}

export function RuleEditorPage() {
  const { id = '' } = useParams()
  const qc = useQueryClient()
  const q = useQuery({ queryKey: ['rule', id], queryFn: () => api.get<RuleEditorView>(`/admin/catalog/rules/${id}`) })
  const [form, setForm] = useState<RuleEntry | null>(null)
  const [baseline, setBaseline] = useState('')
  const [errors, setErrors] = useState<Record<string, string>>({})
  useEffect(() => {
    if (q.data) {
      const f = q.data.draft ?? q.data.published
      setForm(f)
      setBaseline(JSON.stringify(f))
    }
  }, [q.data])
  const dirty = !!form && JSON.stringify(form) !== baseline
  useUnsavedGuard(dirty)

  const onDone = (r: RuleEditorView, msg: string) => {
    qc.setQueryData(['rule', id], r)
    qc.invalidateQueries({ queryKey: ['catalog'] })
    setErrors({})
    toast.success(msg)
  }
  const save = useMutation({
    mutationFn: () => api.put<RuleEditorView>(`/admin/catalog/rules/${id}`, form),
    onSuccess: (r) => onDone(r, '草稿已儲存。按「發布全部修改」後學生才會看到。'),
    onError: (e) => {
      setErrors(e instanceof ApiError ? errorsByPointer(e.errors) : {})
      toast.error(errorMessage(e))
    },
  })
  const discard = useMutation({
    mutationFn: async () => {
      await api.del(`/admin/catalog/drafts/${encodeURIComponent(`rule:${id}`)}`)
      return api.get<RuleEditorView>(`/admin/catalog/rules/${id}`)
    },
    onSuccess: (r) => onDone(r, '已捨棄草稿'),
    onError: (e) => toast.error(errorMessage(e)),
  })
  const reset = useMutation({
    mutationFn: () => api.post<RuleEditorView>(`/admin/catalog/rules/${id}/reset`),
    onSuccess: (r) => onDone(r, '已安排恢復成內建文字，發布後生效'),
    onError: (e) => toast.error(errorMessage(e)),
  })

  if (q.isLoading || (!form && !q.error)) return <Loading />
  if (q.error) return <Page><ErrorState error={q.error} /></Page>
  const r = q.data!
  const f = form!
  const set = (p: Partial<RuleEntry>) => setForm({ ...f, ...p })

  return (
    <Page wide>
      <PageHeader
        back={
          <Link to="/admin/catalog" className="text-sm text-muted-foreground hover:text-foreground">
            教學文案
          </Link>
        }
        title={templatePreview(f.title) || id}
        description={<span className="font-mono text-sm">{id}</span>}
      />
      <div className="sticky top-12 z-20 -mx-4 mb-6 flex flex-wrap items-center gap-2 border-b bg-background/95 px-4 py-2 backdrop-blur sm:-mx-6 sm:px-6">
        <Button onClick={() => save.mutate()} disabled={!dirty || save.isPending} data-testid="save-rule">
          <Save aria-hidden />
          儲存草稿
        </Button>
        {r.draft && (
          <Button variant="ghost" onClick={() => discard.mutate()} disabled={discard.isPending}>
            <Undo2 aria-hidden />
            捨棄草稿
          </Button>
        )}
        <Button variant="ghost" onClick={() => reset.mutate()} disabled={reset.isPending}>
          <RotateCcw aria-hidden />
          恢復內建文字
        </Button>
        <span className="ml-auto text-sm text-muted-foreground" role="status">
          {dirty ? '有尚未儲存的修改' : r.reset_pending ? '發布後將恢復成內建文字' : r.draft ? '草稿已儲存，尚未發布' : '與學生目前看到的相同'}
        </span>
      </div>

      <div className="grid gap-6 xl:grid-cols-main-aside">
        <div className="space-y-6">
          <SectionCard title="文字" description="可以使用 Markdown 與下面列出的變數，例如 {line}。{?var}…{/var} 只在變數有值時顯示，{!var}…{/var} 則相反。">
            <div className="flex flex-wrap items-center gap-1.5">
              <span className="text-sm text-muted-foreground">可用變數：</span>
              {r.vars.map((v) => (
                <code key={v} className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs">{`{${v}}`}</code>
              ))}
            </div>
            <div className="flex flex-wrap gap-6">
              <div className="flex items-center gap-2">
                <Switch id="enabled" checked={f.enabled} onCheckedChange={(enabled) => set({ enabled })} />
                <Label htmlFor="enabled" className="font-normal">
                  啟用（停用時顯示通用說明卡）
                </Label>
              </div>
              <Field label="排序權重（-100～100，越大越優先）" htmlFor="prio" error={errors['/priority']} className="w-56">
                <Input id="prio" type="number" value={f.priority} onChange={(e) => set({ priority: Number(e.target.value) })} />
              </Field>
            </div>
            {TEXT_FIELDS.map((t) => (
              <Field key={t.key} label={t.label} hint={t.hint} htmlFor={`f-${t.key}`} error={errors[`/${t.key}`]}>
                {t.rows === 1 ? (
                  <Input id={`f-${t.key}`} value={f[t.key]} onChange={(e) => set({ [t.key]: e.target.value })} data-testid={`rule-${t.key}`} />
                ) : (
                  <Textarea id={`f-${t.key}`} rows={t.rows} value={f[t.key]} onChange={(e) => set({ [t.key]: e.target.value })} />
                )}
              </Field>
            ))}
            <Field label="下一步可以做什麼">
              <StringList items={f.next_steps} onChange={(next_steps) => set({ next_steps })} label={(i) => `步驟 ${i + 1}`} addLabel="新增步驟" errors={errors} basePointer="/next_steps" />
            </Field>
            <Field label="逐層提示" hint="由淺到深：先給檢查方向，最後才給一般化的小範例，不要直接給學生程式的答案。">
              <StringList items={f.hints} onChange={(hints) => set({ hints })} label={(i) => `提示 ${i + 1}`} addLabel="新增提示" errors={errors} basePointer="/hints" rows={3} />
            </Field>
          </SectionCard>
          <SectionCard
            title="依已學概念調整的版本"
            description="例如學過指標的學生，可以使用更精確的術語。符合的版本會覆蓋基本版本中有填寫的欄位。"
            actions={
              <Button size="sm" variant="outline" onClick={() => set({ variants: [...f.variants, { when_concepts: [] }] })}>
                <Plus aria-hidden />
                新增版本
              </Button>
            }
          >
            {f.variants.length === 0 && <p className="text-sm text-muted-foreground">目前所有學生都看到同一種說法。</p>}
            {f.variants.map((v, i) => (
              <VariantEditor
                key={i}
                v={v}
                base={`/variants/${i}`}
                errors={errors}
                onChange={(nv) => set({ variants: f.variants.map((x, j) => (j === i ? nv : x)) })}
                onRemove={() => set({ variants: f.variants.filter((_, j) => j !== i) })}
              />
            ))}
          </SectionCard>
          <PreviewPanel initialSample={sampleFor(id)} />
        </div>
        <aside className="space-y-3">
          <h2 className="font-medium">內建文字（對照用）</h2>
          {r.builtin ? (
            <dl className="space-y-2 rounded-lg border bg-card p-3 text-sm">
              {TEXT_FIELDS.map((t) => (
                <div key={t.key}>
                  <dt className="text-xs text-muted-foreground">{t.label}</dt>
                  <dd className={r.builtin![t.key] !== f[t.key] ? 'rounded bg-warning-soft px-1' : ''}>{r.builtin![t.key] || '—'}</dd>
                </div>
              ))}
              <div>
                <dt className="text-xs text-muted-foreground">提示</dt>
                <dd>
                  <ol className="list-decimal pl-4">
                    {r.builtin.hints.map((h, i) => (
                      <li key={i} className={h !== f.hints[i] ? 'rounded bg-warning-soft px-1' : ''}>
                        {h}
                      </li>
                    ))}
                  </ol>
                </dd>
              </div>
            </dl>
          ) : (
            <p className="text-sm text-muted-foreground">沒有內建文字。</p>
          )}
          <p className="text-xs text-muted-foreground">標示底色的欄位與內建文字不同。</p>
        </aside>
      </div>
    </Page>
  )
}
