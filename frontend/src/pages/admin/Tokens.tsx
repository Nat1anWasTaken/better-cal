import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Copy, KeyRound, Plus } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { ApiError, api } from '@/api/client'
import type { CreatedToken, TokenInfo } from '@/api/types'
import { Field, SectionCard } from '@/components/admin/Field'
import { Empty, ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { errorMessage, errorsByPointer } from '@/lib/errors'
import { formatTime } from '@/lib/format'

const SCOPES: Array<{ id: string; label: string }> = [
  { id: 'problems:read', label: '讀取題目、題目集與匯出' },
  { id: 'problems:write', label: '建立與修改題目、匯入' },
  { id: 'sets:write', label: '建立與修改題目集' },
  { id: 'publish', label: '發布題目／題目集、切換啟用的題目集' },
  { id: 'submissions:read', label: '讀取所有提交' },
  { id: 'content:write', label: '編輯教學文案' },
  { id: 'students:read', label: '讀取學生與進度' },
  { id: 'students:write', label: '管理學生帳號與組別' },
  { id: 'settings:write', label: '修改全站設定、讀取操作紀錄' },
]

export function TokensPage() {
  const qc = useQueryClient()
  const q = useQuery({ queryKey: ['tokens'], queryFn: () => api.get<TokenInfo[]>('/admin/tokens') })
  const [name, setName] = useState('')
  const [scopes, setScopes] = useState<string[]>(['problems:read', 'problems:write', 'sets:write', 'publish'])
  const [days, setDays] = useState('90')
  const [created, setCreated] = useState<CreatedToken | null>(null)
  const [errors, setErrors] = useState<Record<string, string>>({})
  const create = useMutation({
    mutationFn: () => api.post<CreatedToken>('/admin/tokens', { name, scopes, expires_in_days: days ? Number(days) : null }),
    onSuccess: (t) => {
      setCreated(t)
      setName('')
      setErrors({})
      qc.invalidateQueries({ queryKey: ['tokens'] })
    },
    onError: (e) => {
      setErrors(e instanceof ApiError ? errorsByPointer(e.errors) : {})
      toast.error(errorMessage(e))
    },
  })
  const revoke = useMutation({
    mutationFn: (id: string) => api.del(`/admin/tokens/${id}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['tokens'] })
      toast.success('已撤銷，這個 token 立即失效')
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  return (
    <Page wide>
      <PageHeader title="API token" description="給 Agent 或腳本使用，不需要保存你的密碼。只勾選需要的權限；token 只會顯示一次，伺服器不保存明文。用法：Authorization: Bearer <token>。" />
      <div className="space-y-6">
        {created && (
          <div role="status" className="space-y-2 rounded-lg border border-warning/30 bg-warning-soft p-4">
            <p className="font-medium">「{created.info.name}」已建立。這是唯一一次顯示完整 token：</p>
            <code className="block overflow-x-auto rounded bg-card p-2 font-mono text-sm" data-testid="new-token">
              {created.token}
            </code>
            <Button size="sm" variant="outline" onClick={() => navigator.clipboard.writeText(created.token).then(() => toast.success('已複製'))}>
              <Copy aria-hidden />
              複製
            </Button>
          </div>
        )}
        <SectionCard title="建立 token">
          <div className="grid gap-4 md:grid-cols-2">
            <Field label="名稱（用途）" htmlFor="tn" error={errors['/name']}>
              <Input id="tn" value={name} onChange={(e) => setName(e.target.value)} placeholder="例如：出題 Agent" />
            </Field>
            <Field label="有效天數（1–365，留空表示不過期）" htmlFor="td" error={errors['/expires_in_days']}>
              <Input id="td" type="number" value={days} onChange={(e) => setDays(e.target.value)} />
            </Field>
          </div>
          <Field label="權限" error={errors['/scopes']}>
            <div className="grid gap-1.5 sm:grid-cols-2">
              {SCOPES.map((s) => (
                <div key={s.id} className="flex items-center gap-2">
                  <Checkbox id={`sc-${s.id}`} checked={scopes.includes(s.id)} onCheckedChange={(on) => setScopes(on ? [...scopes, s.id] : scopes.filter((x) => x !== s.id))} />
                  <Label htmlFor={`sc-${s.id}`} className="font-normal">
                    {s.label} <code className="font-mono text-xs text-muted-foreground">{s.id}</code>
                  </Label>
                </div>
              ))}
            </div>
          </Field>
          <Button onClick={() => create.mutate()} disabled={!name || scopes.length === 0 || create.isPending}>
            <Plus aria-hidden />
            建立 token
          </Button>
        </SectionCard>
        {q.isLoading ? (
          <Loading />
        ) : q.error ? (
          <ErrorState error={q.error} />
        ) : q.data!.length === 0 ? (
          <Empty>還沒有建立任何 token。</Empty>
        ) : (
          <ul className="divide-y rounded-lg border bg-card">
            {q.data!.map((t) => {
              const expired = t.expires_at && new Date(t.expires_at) < new Date()
              return (
                <li key={t.id} className="flex flex-wrap items-center gap-3 px-4 py-3">
                  <KeyRound aria-hidden className="size-4 text-muted-foreground" />
                  <span className="font-medium">{t.name}</span>
                  <span className="font-mono text-xs text-muted-foreground">{t.scopes.join(' ')}</span>
                  <span className="text-sm text-muted-foreground">
                    建立 {formatTime(t.created_at)}，最後使用 {formatTime(t.last_used_at)}
                    {t.expires_at && `，到期 ${formatTime(t.expires_at)}`}
                  </span>
                  <span className="ml-auto">
                    {t.revoked_at ? (
                      <StatusPill tone="neutral" label="已撤銷" />
                    ) : expired ? (
                      <StatusPill tone="neutral" label="已過期" />
                    ) : (
                      <Button size="sm" variant="ghost" className="text-destructive" onClick={() => window.confirm(`撤銷「${t.name}」？`) && revoke.mutate(t.id)}>
                        撤銷
                      </Button>
                    )}
                  </span>
                </li>
              )
            })}
          </ul>
        )}
      </div>
    </Page>
  )
}
