import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Save, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { toast } from 'sonner'

import { ApiError, api } from '@/api/client'
import type { CustomRule } from '@/api/types'
import { Field, SectionCard } from '@/components/admin/Field'
import { StringList } from '@/components/admin/StringList'
import { Loading, Page, PageHeader } from '@/components/Page'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { errorMessage, errorsByPointer } from '@/lib/errors'

const blank = (id: string): CustomRule => ({
  id,
  kind: 'error',
  option: null,
  message_regex: '',
  enabled: true,
  priority: 0,
  title: '',
  what: '',
  cause: '',
  where: '',
  why: '',
  next_steps: [],
  hints: [],
})

export function CustomRulePage() {
  const { id: routeId = 'new' } = useParams()
  const isNew = routeId === 'new'
  const nav = useNavigate()
  const qc = useQueryClient()
  // Draft (if any) takes precedence over the published version.
  const hist = useQuery({
    queryKey: ['custom-rule', routeId],
    queryFn: () => api.get<{ published: CustomRule | null; draft: CustomRule | null; delete_pending: boolean }>(`/admin/catalog/custom-rules/${routeId}`),
    enabled: !isNew,
  })
  const [rule, setRule] = useState<CustomRule>(blank('custom.'))
  const [positive, setPositive] = useState<string[]>([''])
  const [negative, setNegative] = useState<string[]>([])
  const [errors, setErrors] = useState<Record<string, string>>({})
  useEffect(() => {
    const latest = hist.data?.draft ?? hist.data?.published ?? undefined
    if (latest) setRule(latest)
    else if (!isNew) setRule(blank(routeId))
  }, [hist.data, isNew, routeId])

  const save = useMutation({
    mutationFn: () => api.put<{ matched_messages: string[] }>(`/admin/catalog/custom-rules/${rule.id}`, { rule, positive_examples: positive, negative_examples: negative }),
    onSuccess: (r) => {
      setErrors({})
      toast.success(`草稿已儲存（比對到：${r.matched_messages.join('；')}）。發布後生效。`)
      qc.invalidateQueries({ queryKey: ['catalog'] })
      if (isNew) nav(`/admin/catalog/custom/${rule.id}`, { replace: true })
    },
    onError: (e) => {
      setErrors(e instanceof ApiError ? errorsByPointer(e.errors) : {})
      toast.error(errorMessage(e))
    },
  })
  const remove = useMutation({
    mutationFn: () => api.del(`/admin/catalog/custom-rules/${routeId}`),
    onSuccess: () => {
      toast.success('已安排移除，發布後生效')
      qc.invalidateQueries({ queryKey: ['catalog'] })
      nav('/admin/catalog')
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  if (!isNew && hist.isLoading) return <Loading />
  const set = (p: Partial<CustomRule>) => setRule({ ...rule, ...p })
  const e = errors

  return (
    <Page wide>
      <PageHeader
        back={
          <Link to="/admin/catalog" className="text-sm text-muted-foreground hover:text-foreground">
            教學文案
          </Link>
        }
        title={isNew ? '新增自訂規則' : rule.title || routeId}
        description="自訂規則用來說明系統還沒有專屬說明的編譯器訊息。它只能比對訊息文字，所以畫面上一律標示為「推測」。儲存時會用真實編譯器編譯你提供的範例，確認規則真的比對得到。"
      />
      <div className="space-y-6">
        <SectionCard title="比對條件">
          <Field label="規則 id（以 custom. 開頭）" htmlFor="cid" error={e['/rule/id']}>
            <Input id="cid" className="max-w-sm font-mono" value={rule.id} disabled={!isNew} onChange={(ev) => set({ id: ev.target.value })} />
          </Field>
          <div className="grid gap-4 md:grid-cols-3">
            <Field label="訊息種類">
              <Select value={rule.kind} onValueChange={(v) => set({ kind: v as CustomRule['kind'] })}>
                <SelectTrigger aria-label="訊息種類">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="error">錯誤</SelectItem>
                  <SelectItem value="warning">警告</SelectItem>
                  <SelectItem value="any">兩者皆可</SelectItem>
                </SelectContent>
              </Select>
            </Field>
            <Field label="警告類別（選填，例如 -Wformat=）" htmlFor="copt">
              <Input id="copt" className="font-mono" value={rule.option ?? ''} onChange={(ev) => set({ option: ev.target.value || null })} />
            </Field>
            <div className="flex items-center gap-2 pt-6">
              <Switch id="cen" checked={rule.enabled} onCheckedChange={(enabled) => set({ enabled })} />
              <Label htmlFor="cen" className="font-normal">
                啟用
              </Label>
            </div>
          </div>
          <Field label="訊息正規表達式" htmlFor="cre" error={e['/rule/message_regex']} hint="具名群組會變成文字裡可用的變數，例如 (?P<kw>break|continue) 可以在文字中寫 {kw}。">
            <Input id="cre" className="font-mono" value={rule.message_regex} onChange={(ev) => set({ message_regex: ev.target.value })} />
          </Field>
        </SectionCard>
        <SectionCard title="範例程式">
          <Field label="應該觸發這個規則的程式（至少一個）" error={e['/positive_examples']}>
            <StringList items={positive} onChange={setPositive} label={(i) => `正例 ${i + 1}`} addLabel="新增正例" errors={e} basePointer="/positive_examples" rows={6} />
          </Field>
          <Field label="不應該觸發的程式（選填）">
            <StringList items={negative} onChange={setNegative} label={(i) => `反例 ${i + 1}`} addLabel="新增反例" errors={e} basePointer="/negative_examples" rows={6} />
          </Field>
        </SectionCard>
        <SectionCard title="說明文字">
          {(['title', 'what', 'cause', 'where', 'why'] as const).map((k) => (
            <Field key={k} label={{ title: '標題', what: '發生什麼事', cause: '可能原因', where: '從哪裡看起', why: '為什麼' }[k]} error={e[`/rule/${k}`]}>
              <Textarea rows={k === 'title' ? 1 : 3} value={rule[k]} onChange={(ev) => set({ [k]: ev.target.value })} />
            </Field>
          ))}
          <Field label="下一步">
            <StringList items={rule.next_steps} onChange={(next_steps) => set({ next_steps })} label={(i) => `步驟 ${i + 1}`} addLabel="新增步驟" />
          </Field>
          <Field label="逐層提示">
            <StringList items={rule.hints} onChange={(hints) => set({ hints })} label={(i) => `提示 ${i + 1}`} addLabel="新增提示" />
          </Field>
        </SectionCard>
        <div className="flex gap-2">
          <Button onClick={() => save.mutate()} disabled={save.isPending}>
            <Save aria-hidden />
            {save.isPending ? '編譯範例並檢查中…' : '檢查並儲存草稿'}
          </Button>
          {!isNew && (
            <Button variant="ghost" className="text-destructive" onClick={() => remove.mutate()}>
              <Trash2 aria-hidden />
              移除這個規則
            </Button>
          )}
        </div>
      </div>
    </Page>
  )
}
