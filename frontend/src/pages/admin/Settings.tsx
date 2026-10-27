import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Save } from 'lucide-react'
import { useEffect, useState } from 'react'
import { toast } from 'sonner'

import { ApiError, api } from '@/api/client'
import type { SettingsView, SiteSettings } from '@/api/types'
import { Field, SectionCard } from '@/components/admin/Field'
import { PolicyEditor } from '@/components/admin/PolicyEditor'
import { Markdown } from '@/components/Markdown'
import { ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { useUnsavedGuard } from '@/hooks/useUnsaved'
import { errorMessage, errorsByPointer } from '@/lib/errors'

export function SettingsPage() {
  const qc = useQueryClient()
  const q = useQuery({ queryKey: ['settings'], queryFn: () => api.get<SettingsView>('/admin/settings') })
  const [s, setS] = useState<SiteSettings | null>(null)
  const [baseline, setBaseline] = useState('')
  const [errors, setErrors] = useState<Record<string, string>>({})
  useEffect(() => {
    if (q.data) {
      setS(q.data.settings)
      setBaseline(JSON.stringify(q.data.settings))
    }
  }, [q.data])
  const dirty = !!s && JSON.stringify(s) !== baseline
  useUnsavedGuard(dirty)
  const save = useMutation({
    mutationFn: () => api.put<SettingsView>('/admin/settings', s),
    onSuccess: (v) => {
      qc.setQueryData(['settings'], v)
      qc.invalidateQueries({ queryKey: ['site'] })
      setErrors({})
      toast.success('設定已儲存，立即生效')
    },
    onError: (e) => {
      setErrors(e instanceof ApiError ? errorsByPointer(e.errors) : {})
      toast.error(errorMessage(e))
    },
  })
  if (q.isLoading || (!s && !q.error)) return <Loading />
  if (q.error) return <Page><ErrorState error={q.error} /></Page>
  const v = s!
  const set = (p: Partial<SiteSettings>) => setS({ ...v, ...p })
  return (
    <Page wide>
      <PageHeader
        title="全站設定"
        description="隱藏測資的內容一律不會給學生看，這一點不能在這裡修改。"
        actions={
          <Button onClick={() => save.mutate()} disabled={!dirty || save.isPending}>
            <Save aria-hidden />
            儲存設定
          </Button>
        }
      />
      <div className="space-y-6">
        <SectionCard title="網站">
          <Field label="站名" htmlFor="sn" error={errors['/site_name']}>
            <Input id="sn" className="max-w-md" value={v.site_name} onChange={(e) => set({ site_name: e.target.value })} />
          </Field>
          <div className="grid gap-4 lg:grid-cols-2">
            <Field label="首頁公告（Markdown）" htmlFor="ann" error={errors['/announcement_md']}>
              <Textarea id="ann" rows={8} value={v.announcement_md} onChange={(e) => set({ announcement_md: e.target.value })} />
            </Field>
            <div className="space-y-1.5">
              <div className="text-sm font-medium">預覽</div>
              <div className="min-h-32 rounded-md border p-3">{v.announcement_md ? <Markdown>{v.announcement_md}</Markdown> : <span className="text-sm text-muted-foreground">（沒有公告）</span>}</div>
            </div>
          </div>
        </SectionCard>
        <SectionCard title="新題目的預設值" description="只影響之後新增的題目。">
          <div className="grid gap-4 md:grid-cols-4">
            <Field label="時間限制（毫秒）" htmlFor="dt" error={errors['/default_limits']}>
              <Input id="dt" type="number" value={v.default_limits.time_ms} onChange={(e) => set({ default_limits: { ...v.default_limits, time_ms: Number(e.target.value) } })} />
            </Field>
            <Field label="記憶體（KiB）" htmlFor="dm">
              <Input id="dm" type="number" value={v.default_limits.memory_kb} onChange={(e) => set({ default_limits: { ...v.default_limits, memory_kb: Number(e.target.value) } })} />
            </Field>
            <Field label="輸出量（KiB）" htmlFor="do">
              <Input id="do" type="number" value={v.default_limits.output_kb} onChange={(e) => set({ default_limits: { ...v.default_limits, output_kb: Number(e.target.value) } })} />
            </Field>
            <Field label="輸出比對方式">
              <Select value={v.default_compare_mode} onValueChange={(m) => set({ default_compare_mode: m as SiteSettings['default_compare_mode'] })}>
                <SelectTrigger aria-label="預設比對方式">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="trim_trailing_whitespace">忽略行尾空白</SelectItem>
                  <SelectItem value="exact">完全相同</SelectItem>
                </SelectContent>
              </Select>
            </Field>
          </div>
        </SectionCard>
        <SectionCard title="全站預設的學習設定" description="所有學生的基本設定；組別與個人設定可以覆蓋。">
          <PolicyEditor level="site" idPrefix="site" value={v.default_policy} onChange={(default_policy) => set({ default_policy })} errors={errorsByPointer(Object.entries(errors).map(([pointer, message]) => ({ pointer, message, code: '' })), '/default_policy')} />
        </SectionCard>
      </div>
    </Page>
  )
}
