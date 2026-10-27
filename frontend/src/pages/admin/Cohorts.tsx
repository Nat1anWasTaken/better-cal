import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Plus, Save, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { ApiError, api } from '@/api/client'
import type { Cohort, PolicyPatch } from '@/api/types'
import { Field, SectionCard } from '@/components/admin/Field'
import { PolicyEditor } from '@/components/admin/PolicyEditor'
import { Empty, ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { errorMessage, errorsByPointer } from '@/lib/errors'

import { useCohorts } from './Students'

interface Form {
  id?: string
  name: string
  description: string
  policy: PolicyPatch
}

function CohortForm({ initial, onDone }: { initial: Form; onDone: () => void }) {
  const qc = useQueryClient()
  const [f, setF] = useState<Form>(initial)
  const [errors, setErrors] = useState<Record<string, string>>({})
  const save = useMutation({
    mutationFn: () => {
      const body = { name: f.name, description: f.description, policy: f.policy }
      return f.id ? api.put<Cohort>(`/admin/cohorts/${f.id}`, body) : api.post<Cohort>('/admin/cohorts', body)
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['cohorts'] })
      toast.success('組別已儲存，設定立即套用到組內學生')
      onDone()
    },
    onError: (e) => {
      setErrors(e instanceof ApiError ? errorsByPointer(e.errors) : {})
      toast.error(errorMessage(e))
    },
  })
  const del = useMutation({
    mutationFn: () => api.del(`/admin/cohorts/${f.id}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['cohorts'] })
      toast.success('已刪除組別，學生帳號保留')
      onDone()
    },
  })
  return (
    <div className="space-y-4">
      <div className="grid gap-4 md:grid-cols-2">
        <Field label="名稱" htmlFor={`cn-${f.id ?? 'new'}`} error={errors['/name']}>
          <Input id={`cn-${f.id ?? 'new'}`} value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} />
        </Field>
        <Field label="說明" htmlFor={`cd-${f.id ?? 'new'}`}>
          <Input id={`cd-${f.id ?? 'new'}`} value={f.description} onChange={(e) => setF({ ...f, description: e.target.value })} />
        </Field>
      </div>
      <PolicyEditor level="cohort" idPrefix={`cp-${f.id ?? 'new'}`} value={f.policy} onChange={(policy) => setF({ ...f, policy })} errors={errorsByPointer(Object.entries(errors).map(([pointer, message]) => ({ pointer, message, code: '' })), '/policy')} />
      <div className="flex gap-2">
        <Button onClick={() => save.mutate()} disabled={save.isPending || !f.name}>
          <Save aria-hidden />
          儲存
        </Button>
        {f.id && (
          <Button variant="ghost-destructive" onClick={() => window.confirm(`刪除組別「${f.name}」？`) && del.mutate()}>
            <Trash2 aria-hidden />
            刪除組別
          </Button>
        )}
        <Button variant="ghost" onClick={onDone}>
          取消
        </Button>
      </div>
    </div>
  )
}

export function CohortsPage() {
  const q = useCohorts()
  const [editing, setEditing] = useState<string | null>(null)
  if (q.isLoading) return <Loading />
  if (q.error) return <Page><ErrorState error={q.error} /></Page>
  return (
    <Page wide>
      <PageHeader
        title="組別"
        description="把學生分組，可以替每一組設定已學過的概念、可看到的題目集、提示政策與操作頻率。"
        actions={
          <Button onClick={() => setEditing('new')} disabled={editing === 'new'}>
            <Plus aria-hidden />
            新增組別
          </Button>
        }
      />
      <div className="space-y-4">
        {editing === 'new' && (
          <SectionCard title="新增組別">
            <CohortForm initial={{ name: '', description: '', policy: {} }} onDone={() => setEditing(null)} />
          </SectionCard>
        )}
        {q.data!.length === 0 && editing !== 'new' && <Empty>還沒有組別。</Empty>}
        {q.data!.map((c) => (
          <SectionCard
            key={c.id}
            title={c.name}
            description={`${c.member_count} 位學生${c.description ? `，${c.description}` : ''}`}
            actions={
              editing !== c.id && (
                <Button size="sm" variant="outline" onClick={() => setEditing(c.id)}>
                  編輯
                </Button>
              )
            }
          >
            {editing === c.id ? (
              <CohortForm initial={{ id: c.id, name: c.name, description: c.description, policy: c.policy }} onDone={() => setEditing(null)} />
            ) : (
              <p className="text-sm text-muted-foreground">
                {c.policy.learned_concepts?.length ? `已學概念 ${c.policy.learned_concepts.length} 項` : '沒有額外概念'}
                {c.policy.visible_set_ids ? `，只看得到 ${c.policy.visible_set_ids.length} 個題目集` : ''}
                {c.policy.auto_reveal_hints != null ? `，一開始顯示 ${c.policy.auto_reveal_hints} 層提示` : ''}
              </p>
            )}
          </SectionCard>
        ))}
      </div>
    </Page>
  )
}
