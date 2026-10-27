import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ArrowDown, ArrowUp, Plus, Save, Send, Star, Trash2, Undo2, X } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { toast } from 'sonner'

import { ApiError, api, qs } from '@/api/client'
import type { AdminSet, Page as PageT, ProblemSummary } from '@/api/types'
import { ConceptPicker, Field, SectionCard } from '@/components/admin/Field'
import { ErrorState, FieldMessage, Loading, Page, PageHeader } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { Label } from '@/components/ui/label'
import { useUnsavedGuard } from '@/hooks/useUnsaved'
import { errorMessage, errorsByPointer } from '@/lib/errors'
import { STATUS_LABELS } from '@/lib/format'

import { problemStatusTone } from './Problems'

interface Item {
  slug: string
  title: string
  status: string
}
interface Form {
  slug: string
  title: string
  description_md: string
  learned_concepts: string[]
  listed: boolean
  items: Item[]
}

function AddProblem({ onAdd, exclude }: { onAdd: (p: ProblemSummary) => void; exclude: string[] }) {
  const [q, setQ] = useState('')
  const res = useQuery({
    queryKey: ['admin-problems', 'picker', q],
    queryFn: () => api.get<PageT<ProblemSummary>>(`/admin/problems${qs({ q, limit: 10 })}`),
  })
  return (
    <div className="space-y-2 rounded-md border p-3">
      <Input aria-label="搜尋要加入的題目" placeholder="搜尋題目庫…" value={q} onChange={(e) => setQ(e.target.value)} />
      <ul className="max-h-60 divide-y overflow-y-auto">
        {res.data?.items
          .filter((p) => p.status !== 'archived' && !exclude.includes(p.slug))
          .map((p) => (
            <li key={p.id} className="flex items-center gap-2 py-1.5 text-sm">
              <span className="flex-1">
                {p.title} <span className="font-mono text-xs text-muted-foreground">{p.slug}</span>
              </span>
              <StatusPill tone={problemStatusTone(p.status)} label={STATUS_LABELS[p.status] ?? p.status} />
              <Button size="sm" variant="outline" onClick={() => onAdd(p)}>
                <Plus aria-hidden />
                加入
              </Button>
            </li>
          ))}
      </ul>
    </div>
  )
}

export function SetEditorPage() {
  const { slug: routeSlug } = useParams()
  const isNew = !routeSlug
  const nav = useNavigate()
  const qc = useQueryClient()
  const q = useQuery({ queryKey: ['admin-set', routeSlug], queryFn: () => api.get<AdminSet>(`/admin/problem-sets/${routeSlug}`), enabled: !isNew })
  const [form, setForm] = useState<Form | null>(isNew ? { slug: '', title: '', description_md: 'CYCU-CAL 自建教材。', learned_concepts: ['io', 'variables'], listed: true, items: [] } : null)
  const [baseline, setBaseline] = useState('')
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [adding, setAdding] = useState(false)

  useEffect(() => {
    if (q.data) {
      const f: Form = {
        slug: q.data.slug,
        title: q.data.title,
        description_md: q.data.description_md,
        learned_concepts: q.data.learned_concepts,
        listed: q.data.listed,
        items: q.data.items.map((i) => ({ slug: i.slug, title: i.title, status: i.problem_status })),
      }
      setForm(f)
      setBaseline(JSON.stringify(f))
    }
  }, [q.data])
  const dirty = !!form && JSON.stringify(form) !== baseline && (!isNew || form.title !== '')
  useUnsavedGuard(dirty)

  const save = useMutation({
    mutationFn: async () => {
      const body = { slug: form!.slug, title: form!.title, description_md: form!.description_md, learned_concepts: form!.learned_concepts, listed: form!.listed, problems: form!.items.map((i) => i.slug) }
      return isNew
        ? api.post<AdminSet>('/admin/problem-sets', body, { headers: { 'Idempotency-Key': crypto.randomUUID() } })
        : api.put<AdminSet>(`/admin/problem-sets/${routeSlug}`, body)
    },
    onSuccess: (s) => {
      setErrors({})
      setBaseline(JSON.stringify(form))
      qc.invalidateQueries({ queryKey: ['admin-sets'] })
      qc.setQueryData(['admin-set', s.slug], s)
      toast.success('題目集已儲存')
      if (isNew) nav(`/admin/sets/${s.slug}`, { replace: true })
    },
    onError: (e) => {
      setErrors(e instanceof ApiError ? errorsByPointer(e.errors) : {})
      toast.error(errorMessage(e))
    },
  })
  const act = useMutation({
    mutationFn: (kind: 'publish' | 'unpublish' | 'activate' | 'archive') => {
      if (kind === 'activate') return api.put('/admin/site/active-problem-set', { problem_set: routeSlug })
      if (kind === 'archive') return api.del(`/admin/problem-sets/${routeSlug}`)
      return api.post(`/admin/problem-sets/${routeSlug}/${kind}`)
    },
    onSuccess: (_, kind) => {
      qc.invalidateQueries({ queryKey: ['admin-set', routeSlug] })
      qc.invalidateQueries({ queryKey: ['admin-sets'] })
      qc.invalidateQueries({ queryKey: ['site'] })
      toast.success({ publish: '已發布', unpublish: '已改回草稿', activate: '已設為目前啟用的題目集，學生端會立即看到', archive: '已封存' }[kind])
      if (kind === 'archive') nav('/admin/sets')
    },
    onError: (e) => {
      setErrors(e instanceof ApiError ? errorsByPointer(e.errors) : {})
      toast.error(errorMessage(e))
    },
  })

  if (!isNew && q.isLoading) return <Loading />
  if (q.error) return <Page><ErrorState error={q.error} /></Page>
  if (!form) return <Loading />
  const set = (p: Partial<Form>) => setForm({ ...form, ...p })
  const move = (i: number, d: number) => {
    const items = [...form.items]
    const [x] = items.splice(i, 1)
    items.splice(i + d, 0, x)
    set({ items })
  }
  const s = q.data

  return (
    <Page wide>
      <PageHeader
        back={
          <Link to="/admin/sets" className="text-sm text-muted-foreground hover:text-foreground">
            題目集
          </Link>
        }
        title={isNew ? '新增題目集' : form.title}
        description={
          s && (
            <span className="flex flex-wrap items-center gap-2 text-sm">
              <StatusPill tone={problemStatusTone(s.status)} label={STATUS_LABELS[s.status] ?? s.status} />
              {s.active && (
                <span className="inline-flex items-center gap-1 text-primary">
                  <Star aria-hidden className="size-4" />
                  目前啟用：學生進入平台時預設看到這個題目集
                </span>
              )}
            </span>
          )
        }
      />
      <div className="sticky top-12 z-20 -mx-4 mb-6 flex flex-wrap gap-2 border-b bg-background/95 px-4 py-2 backdrop-blur sm:-mx-6 sm:px-6">
        <Button onClick={() => save.mutate()} disabled={save.isPending || (!dirty && !isNew)}>
          <Save aria-hidden />
          {isNew ? '建立草稿' : '儲存'}
        </Button>
        {!isNew && s && (
          <>
            {s.status !== 'published' ? (
              <Button variant="outline" disabled={dirty || act.isPending} onClick={() => act.mutate('publish')}>
                <Send aria-hidden />
                發布
              </Button>
            ) : (
              <Button variant="ghost" disabled={act.isPending || s.active} onClick={() => act.mutate('unpublish')}>
                <Undo2 aria-hidden />
                改回草稿
              </Button>
            )}
            <Button variant="outline" disabled={dirty || act.isPending || s.active || s.status !== 'published'} onClick={() => act.mutate('activate')} data-testid="activate-set">
              <Star aria-hidden />
              設為目前啟用
            </Button>
            <Button variant="ghost" className="text-destructive" disabled={act.isPending || s.active} onClick={() => window.confirm('封存這個題目集？題目本身不受影響。') && act.mutate('archive')}>
              <Trash2 aria-hidden />
              封存
            </Button>
          </>
        )}
        {dirty && <span className="ml-auto self-center text-sm text-muted-foreground">有尚未儲存的修改</span>}
      </div>
      <div className="grid gap-6 lg:grid-cols-2">
        <SectionCard title="基本資料">
          {isNew && (
            <Field label="Slug（建立後不能修改）" htmlFor="sslug" error={errors['/slug']}>
              <Input id="sslug" className="font-mono" value={form.slug} onChange={(e) => set({ slug: e.target.value })} />
            </Field>
          )}
          <Field label="名稱" htmlFor="stitle" error={errors['/title']}>
            <Input id="stitle" value={form.title} onChange={(e) => set({ title: e.target.value })} />
          </Field>
          <Field label="說明（Markdown）" htmlFor="sdesc">
            <Textarea id="sdesc" rows={4} value={form.description_md} onChange={(e) => set({ description_md: e.target.value })} />
          </Field>
          <div className="flex items-center gap-2">
            <Switch id="slisted" checked={form.listed} onCheckedChange={(listed) => set({ listed })} />
            <Label htmlFor="slisted" className="font-normal">
              在學生的題目集列表中公開列出（關閉時只有被指定的組別或學生看得到）
            </Label>
          </div>
          <Field label="這個題目集假設學生已學過的概念">
            <ConceptPicker idPrefix="sc" value={form.learned_concepts} onChange={(learned_concepts) => set({ learned_concepts })} />
          </Field>
        </SectionCard>
        <SectionCard
          title={`題目（${form.items.length}）`}
          description="依照順序顯示給學生。已發布的題目集只能包含已發布的題目。"
          actions={
            <Button size="sm" variant="outline" onClick={() => setAdding((a) => !a)}>
              {adding ? <X aria-hidden /> : <Plus aria-hidden />}
              {adding ? '完成' : '加入題目'}
            </Button>
          }
        >
          {adding && <AddProblem exclude={form.items.map((i) => i.slug)} onAdd={(p) => set({ items: [...form.items, { slug: p.slug, title: p.title, status: p.status }] })} />}
          <ol className="divide-y rounded-md border">
            {form.items.map((it, i) => (
              <li key={it.slug} className="flex items-center gap-2 px-3 py-2 text-sm">
                <span className="w-5 text-right tabular-nums text-muted-foreground">{i + 1}</span>
                <span className="flex-1">
                  {it.title} <span className="font-mono text-xs text-muted-foreground">{it.slug}</span>
                </span>
                <StatusPill tone={problemStatusTone(it.status)} label={STATUS_LABELS[it.status] ?? it.status} />
                <Button variant="ghost" size="icon" aria-label="上移" disabled={i === 0} onClick={() => move(i, -1)}>
                  <ArrowUp aria-hidden />
                </Button>
                <Button variant="ghost" size="icon" aria-label="下移" disabled={i === form.items.length - 1} onClick={() => move(i, 1)}>
                  <ArrowDown aria-hidden />
                </Button>
                <Button variant="ghost" size="icon" aria-label={`從題目集移除 ${it.title}`} onClick={() => set({ items: form.items.filter((x) => x.slug !== it.slug) })}>
                  <X aria-hidden />
                </Button>
              </li>
            ))}
            {form.items.length === 0 && <li className="px-3 py-4 text-sm text-muted-foreground">還沒有題目，按「加入題目」從題目庫挑選。</li>}
          </ol>
          {Object.entries(errors)
            .filter(([k]) => k.startsWith('/problems'))
            .map(([k, v]) => (
              <FieldMessage key={k} message={`第 ${Number(k.split('/')[2] ?? 0) + 1} 題：${v}`} />
            ))}
        </SectionCard>
      </div>
    </Page>
  )
}
