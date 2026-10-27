import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { KeyRound, LogOut, Save, UserCheck, UserX } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Link, useParams } from 'react-router'
import { toast } from 'sonner'

import { ApiError, api } from '@/api/client'
import type { AdminUser, PolicyPatch, PolicyView, UserProblemProgress } from '@/api/types'
import { SectionCard, conceptLabel, useConcepts } from '@/components/admin/Field'
import { PolicyEditor } from '@/components/admin/PolicyEditor'
import { ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { errorMessage, errorsByPointer } from '@/lib/errors'
import { formatTime, verdictInfo } from '@/lib/format'

import { useCohorts } from './Students'

const NONE = '__none__'
const SOURCE: Record<string, string> = { site: '全站預設', cohort: '組別設定', student: '個人設定' }

async function findUser(id: string): Promise<AdminUser> {
  return api.get<AdminUser>(`/admin/users/${id}`)
}

function Effective({ v }: { v: PolicyView }) {
  const concepts = useConcepts()
  const e = v.effective
  const row = (label: string, value: string, source?: string) => (
    <tr>
      <td className="py-1.5 pr-4 text-muted-foreground">{label}</td>
      <td className="py-1.5 pr-4">{value}</td>
      <td className="py-1.5 text-xs text-muted-foreground">{source ? SOURCE[source] : '各層合併'}</td>
    </tr>
  )
  return (
    <table className="w-full text-sm">
      <tbody className="divide-y">
        {row('已學過的概念', e.learned_concepts.map((c) => conceptLabel(concepts.data, c)).join('、') || '（無）')}
        {row('可看到的題目集', e.visible_set_ids.value ? `只限 ${e.visible_set_ids.value.length} 個指定題目集` : '所有公開列出的題目集', e.visible_set_ids.source)}
        {row('額外開放的題目集', `${e.extra_set_ids.length} 個`)}
        {row('預設題目集', e.default_set_id.source === 'site' ? '目前啟用的題目集' : '指定的題目集', e.default_set_id.source)}
        {row('一開始顯示的提示層數', String(e.auto_reveal_hints.value), e.auto_reveal_hints.source)}
        {row('最多可展開的提示層數', e.max_hint_levels.value == null ? '不限制' : String(e.max_hint_levels.value), e.max_hint_levels.source)}
        {row('原始訊息', e.raw_expanded.value ? '預設展開' : '預設收合', e.raw_expanded.source)}
        {row('每分鐘執行／提交上限', `${e.runs_per_minute.value}／${e.submits_per_minute.value}`, e.runs_per_minute.source)}
        {row('排行榜', e.leaderboard.value ? '開啟' : '關閉', e.leaderboard.source)}
      </tbody>
    </table>
  )
}

export function StudentDetailPage() {
  const { id = '' } = useParams()
  const qc = useQueryClient()
  const cohorts = useCohorts()
  const user = useQuery({ queryKey: ['admin-user', id], queryFn: () => findUser(id) })
  const policy = useQuery({ queryKey: ['policy', id], queryFn: () => api.get<PolicyView>(`/admin/users/${id}/policy`) })
  const progress = useQuery({ queryKey: ['user-progress', id], queryFn: () => api.get<UserProblemProgress[]>(`/admin/users/${id}/progress`) })
  const [overrides, setOverrides] = useState<PolicyPatch | null>(null)
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [newPassword, setNewPassword] = useState<string | null>(null)
  useEffect(() => {
    if (policy.data) setOverrides(policy.data.overrides)
  }, [policy.data])

  const patch = useMutation({
    mutationFn: (body: Record<string, unknown>) => api.patch<AdminUser>(`/admin/users/${id}`, body),
    onSuccess: (u) => {
      qc.setQueryData(['admin-user', id], u)
      qc.invalidateQueries({ queryKey: ['policy', id] })
      qc.invalidateQueries({ queryKey: ['admin-users'] })
      toast.success('已更新')
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const savePolicy = useMutation({
    mutationFn: () => api.put<PolicyView>(`/admin/users/${id}/policy`, overrides),
    onSuccess: (v) => {
      qc.setQueryData(['policy', id], v)
      setErrors({})
      toast.success('個人設定已儲存，立即生效')
    },
    onError: (e) => {
      setErrors(e instanceof ApiError ? errorsByPointer(e.errors) : {})
      toast.error(errorMessage(e))
    },
  })
  const action = useMutation({
    mutationFn: async (kind: 'reset' | 'revoke') => {
      if (kind === 'reset') {
        const r = await api.post<{ password: string }>(`/admin/users/${id}/reset-password`)
        setNewPassword(r.password)
      } else await api.post(`/admin/users/${id}/revoke-sessions`)
    },
    onSuccess: (_, kind) => toast.success(kind === 'reset' ? '已產生新密碼，學生的所有登入已登出' : '已登出這位學生的所有裝置'),
    onError: (e) => toast.error(errorMessage(e)),
  })
  const credit = useMutation({
    mutationFn: ({ problemId, on }: { problemId: string; on: boolean }) =>
      on ? api.put(`/admin/users/${id}/credits/${problemId}`, { note: '' }) : api.del(`/admin/users/${id}/credits/${problemId}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['user-progress', id] }),
    onError: (e) => toast.error(errorMessage(e)),
  })

  if (user.isLoading) return <Loading />
  if (user.error) return <Page><ErrorState error={user.error} /></Page>
  const u = user.data!
  return (
    <Page wide>
      <PageHeader
        back={
          <Link to="/admin/users" className="text-sm text-muted-foreground hover:text-foreground">
            學生帳號
          </Link>
        }
        title={u.display_name || u.username}
        description={
          <span className="text-sm">
            帳號 <span className="font-mono">{u.username}</span>，建立於 {formatTime(u.created_at)}
            {u.disabled && '，已停用'}
          </span>
        }
        actions={
          <>
            <Select value={u.cohort_id ?? NONE} onValueChange={(v) => patch.mutate(v === NONE ? { clear_cohort: true } : { cohort_id: v })}>
              <SelectTrigger className="w-44" aria-label="組別">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NONE}>不分組</SelectItem>
                {cohorts.data?.map((c) => (
                  <SelectItem key={c.id} value={c.id}>
                    {c.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button variant="outline" onClick={() => patch.mutate({ disabled: !u.disabled })}>
              {u.disabled ? <UserCheck aria-hidden /> : <UserX aria-hidden />}
              {u.disabled ? '啟用帳號' : '停用帳號'}
            </Button>
            <Button variant="outline" onClick={() => window.confirm('產生新密碼並登出這位學生？') && action.mutate('reset')}>
              <KeyRound aria-hidden />
              重設密碼
            </Button>
            <Button variant="ghost" onClick={() => action.mutate('revoke')}>
              <LogOut aria-hidden />
              強制登出
            </Button>
          </>
        }
      />
      {newPassword && (
        <div role="status" className="mb-6 rounded-lg border border-warning/30 bg-warning-soft p-4">
          新密碼（只顯示這一次）：<code className="ml-2 font-mono text-lg">{newPassword}</code>
        </div>
      )}
      <div className="grid gap-6 xl:grid-cols-2">
        <SectionCard title="目前生效的設定" description="個人設定 > 組別設定 > 全站預設。">
          {policy.data ? <Effective v={policy.data} /> : <Loading />}
        </SectionCard>
        <SectionCard
          title="個人設定"
          description="只套用在這位學生。沒有設定的項目沿用組別或全站設定。"
          actions={
            <Button size="sm" onClick={() => savePolicy.mutate()} disabled={savePolicy.isPending || !overrides}>
              <Save aria-hidden />
              儲存
            </Button>
          }
        >
          {overrides && <PolicyEditor level="student" idPrefix="sp" value={overrides} onChange={setOverrides} errors={errors} />}
        </SectionCard>
      </div>
      <section className="mt-6">
        <div className="mb-2 flex items-center gap-3">
          <h2 className="font-medium">各題進度</h2>
          <Link className="text-sm text-primary underline" to={`/admin/submissions?user_id=${id}`}>
            查看所有提交
          </Link>
        </div>
        {progress.data?.length === 0 ? (
          <p className="text-sm text-muted-foreground">這位學生還沒有執行或提交過程式。</p>
        ) : (
          <div className="overflow-x-auto rounded-lg border bg-card">
            <table className="w-full text-sm">
              <thead className="border-b text-left text-muted-foreground">
                <tr>
                  <th className="px-3 py-2 font-normal">題目</th>
                  <th className="px-3 py-2 font-normal">狀態</th>
                  <th className="px-3 py-2 font-normal">執行／提交</th>
                  <th className="px-3 py-2 font-normal">常見錯誤</th>
                  <th className="px-3 py-2 font-normal">展開提示</th>
                  <th className="px-3 py-2 font-normal">手動認列</th>
                </tr>
              </thead>
              <tbody className="divide-y">
                {progress.data?.map((p) => {
                  const v = verdictInfo(p.last_verdict)
                  return (
                    <tr key={p.problem_id}>
                      <td className="px-3 py-2">{p.title}</td>
                      <td className="px-3 py-2">
                        {p.solved ? <StatusPill tone="success" label="已通過" /> : p.last_verdict ? <StatusPill tone={v.tone} label={`最近：${v.label}`} /> : <StatusPill tone="neutral" label="只有執行" />}
                      </td>
                      <td className="px-3 py-2 tabular-nums">
                        {p.runs}／{p.submissions}
                      </td>
                      <td className="px-3 py-2 font-mono text-xs">{p.frequent_rules.join('、') || '—'}</td>
                      <td className="px-3 py-2 tabular-nums">{p.hints_revealed}</td>
                      <td className="px-3 py-2">
                        <Button size="sm" variant={p.credited ? 'secondary' : 'ghost'} onClick={() => credit.mutate({ problemId: p.problem_id, on: !p.credited })}>
                          {p.credited ? '已認列（取消）' : '認列為完成'}
                        </Button>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </Page>
  )
}
