import { useQuery } from '@tanstack/react-query'

import { api } from '@/api/client'
import type { AdminSet, PolicyPatch } from '@/api/types'
import { ConceptPicker, Field } from '@/components/admin/Field'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

const INHERIT = '__inherit__'

function OptNumber({ id, label, value, onChange, error, hint }: { id: string; label: string; value: number | null | undefined; onChange: (v: number | null) => void; error?: string; hint?: string }) {
  return (
    <Field label={label} htmlFor={id} error={error} hint={hint ?? '留空：沿用上一層設定'}>
      <Input id={id} type="number" min={0} className="w-32" value={value ?? ''} onChange={(e) => onChange(e.target.value === '' ? null : Number(e.target.value))} />
    </Field>
  )
}

function SetChecklist({ sets, value, onChange, idPrefix }: { sets: AdminSet[]; value: string[]; onChange: (v: string[]) => void; idPrefix: string }) {
  return (
    <div className="grid gap-1.5 sm:grid-cols-2">
      {sets.map((s) => (
        <div key={s.id} className="flex items-center gap-2">
          <Checkbox id={`${idPrefix}-${s.id}`} checked={value.includes(s.id)} onCheckedChange={(on) => onChange(on ? [...value, s.id] : value.filter((x) => x !== s.id))} />
          <Label htmlFor={`${idPrefix}-${s.id}`} variant="option">
            {s.title}
            {s.status !== 'published' && <span className="text-xs text-muted-foreground">（未發布）</span>}
          </Label>
        </div>
      ))}
    </div>
  )
}

/** Editor for a PolicyPatch: every field optional (unset = inherit). */
export function PolicyEditor({ value, onChange, errors = {}, level, idPrefix }: { value: PolicyPatch; onChange: (p: PolicyPatch) => void; errors?: Record<string, string>; level: 'site' | 'cohort' | 'student'; idPrefix: string }) {
  const sets = useQuery({ queryKey: ['admin-sets'], queryFn: () => api.get<AdminSet[]>('/admin/problem-sets') })
  const set = (p: Partial<PolicyPatch>) => onChange({ ...value, ...p })
  const restrict = value.visible_set_ids != null
  const allSets = (sets.data ?? []).filter((s) => s.status !== 'archived')
  return (
    <div className="space-y-5">
      <Field label={level === 'site' ? '所有學生都已學過的概念' : '額外已學過的概念（與上層合併）'} error={errors['/learned_concepts']}>
        <ConceptPicker idPrefix={`${idPrefix}-lc`} value={value.learned_concepts ?? []} onChange={(v) => set({ learned_concepts: v.length ? v : null })} />
      </Field>
      {level !== 'site' && (
        <>
          {level === 'cohort' && (
            <Field label="可以看到的題目集">
              <div className="flex items-center gap-2">
                <Checkbox id={`${idPrefix}-restrict`} checked={restrict} onCheckedChange={(on) => set({ visible_set_ids: on ? [] : null })} />
                <Label htmlFor={`${idPrefix}-restrict`} variant="option">
                  只顯示指定的題目集（不勾選：所有公開列出的已發布題目集）
                </Label>
              </div>
              {restrict && <SetChecklist idPrefix={`${idPrefix}-vs`} sets={allSets} value={value.visible_set_ids ?? []} onChange={(v) => set({ visible_set_ids: v })} />}
            </Field>
          )}
          <Field label="額外開放的題目集（包含不公開列出的）">
            <SetChecklist idPrefix={`${idPrefix}-es`} sets={allSets} value={value.extra_set_ids ?? []} onChange={(v) => set({ extra_set_ids: v.length ? v : null })} />
          </Field>
        </>
      )}
      <Field label="進入平台時預設看到的題目集">
        <Select value={value.default_set_id ?? INHERIT} onValueChange={(v) => set({ default_set_id: v === INHERIT ? null : v })}>
          <SelectTrigger className="w-72" aria-label="預設題目集">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={INHERIT}>{level === 'site' ? '目前啟用的題目集' : '沿用上一層設定'}</SelectItem>
            {allSets
              .filter((s) => s.status === 'published')
              .map((s) => (
                <SelectItem key={s.id} value={s.id}>
                  {s.title}
                </SelectItem>
              ))}
          </SelectContent>
        </Select>
      </Field>
      <div className="grid gap-4 sm:grid-cols-2">
        <OptNumber id={`${idPrefix}-auto`} label="一開始就顯示的提示層數" value={value.auto_reveal_hints} onChange={(v) => set({ auto_reveal_hints: v })} error={errors['/auto_reveal_hints']} />
        <OptNumber id={`${idPrefix}-max`} label="最多可以展開的提示層數" value={value.max_hint_levels} onChange={(v) => set({ max_hint_levels: v })} error={errors['/max_hint_levels']} hint="留空：沿用上一層（全部都沒設定時不限制）" />
        <OptNumber id={`${idPrefix}-runs`} label="每分鐘最多執行次數" value={value.runs_per_minute} onChange={(v) => set({ runs_per_minute: v })} error={errors['/runs_per_minute']} />
        <OptNumber id={`${idPrefix}-subs`} label="每分鐘最多提交次數" value={value.submits_per_minute} onChange={(v) => set({ submits_per_minute: v })} error={errors['/submits_per_minute']} />
      </div>
      <Field label="原始編譯器訊息">
        <Select value={value.raw_expanded == null ? INHERIT : value.raw_expanded ? 'on' : 'off'} onValueChange={(v) => set({ raw_expanded: v === INHERIT ? null : v === 'on' })}>
          <SelectTrigger className="w-72" aria-label="原始訊息預設展開">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={INHERIT}>沿用上一層設定</SelectItem>
            <SelectItem value="on">預設展開</SelectItem>
            <SelectItem value="off">預設收合</SelectItem>
          </SelectContent>
        </Select>
      </Field>
    </div>
  )
}
