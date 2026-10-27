import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router'

import { api } from '@/api/client'
import type { AuditEntry, Overview, Page as PageT } from '@/api/types'
import { ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { formatTime } from '@/lib/format'

function Stat({ label, value, to }: { label: string; value: number | string; to?: string }) {
  const body = (
    <>
      <div className="text-2xl font-medium tabular-nums">{value}</div>
      <div className="text-sm text-muted-foreground">{label}</div>
    </>
  )
  return to ? (
    <Link to={to} className="rounded-lg border bg-card p-4 hover:border-primary/50">
      {body}
    </Link>
  ) : (
    <div className="rounded-lg border bg-card p-4">{body}</div>
  )
}

export function OverviewPage() {
  const q = useQuery({ queryKey: ['admin-overview'], queryFn: () => api.get<Overview>('/admin/overview'), refetchInterval: 30_000 })
  const audit = useQuery({ queryKey: ['audit', 'recent'], queryFn: () => api.get<PageT<AuditEntry>>('/admin/audit?limit=8') })
  if (q.isLoading) return <Loading />
  if (q.error) return <Page><ErrorState error={q.error} /></Page>
  const o = q.data!
  const runner = o.runner as { compiler_version?: string; isolate_version?: string; concurrency?: number } | null | undefined
  return (
    <Page wide>
      <PageHeader title="總覽" />
      <div className="mb-6 flex flex-wrap items-center gap-3 rounded-lg border bg-card p-4">
        <span className="font-medium">評測服務</span>
        <StatusPill tone={o.judge === 'ok' ? 'success' : 'error'} label={o.judge === 'ok' ? '正常運作' : '無法連線'} />
        {runner && (
          <span className="text-sm text-muted-foreground">
            {runner.compiler_version}，{runner.isolate_version}，同時評測 {runner.concurrency} 筆
          </span>
        )}
        <span className="ml-auto text-sm">排隊中 {o.queue_length} 筆，24 小時內服務錯誤 {o.system_errors_24h} 筆</span>
      </div>
      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <Stat label="學生帳號" value={o.students} to="/admin/users" />
        <Stat label="今天的執行與提交" value={o.submissions_today} to="/admin/submissions" />
        <Stat label="今天通過的提交" value={o.accepted_today} />
        <Stat label="草稿題目" value={o.draft_problems} to="/admin/problems?status=draft" />
        <Stat label="已發布但有新修改的題目" value={o.problems_with_unpublished_changes} to="/admin/problems" />
        <Stat label="尚未發布的文案修改" value={o.catalog_pending_changes} to="/admin/catalog" />
      </div>
      <section className="mt-8">
        <h2 className="mb-2 font-medium">最近的操作</h2>
        <ul className="divide-y rounded-lg border bg-card text-sm">
          {audit.data?.items.map((a) => (
            <li key={a.id} className="flex flex-wrap gap-3 px-4 py-2">
              <span className="w-28 text-muted-foreground">{formatTime(a.at)}</span>
              <span>{a.actor ?? '系統'}{a.via_token ? `（token：${a.via_token}）` : ''}</span>
              <span className="font-mono text-xs">{a.action}</span>
              <span className="text-muted-foreground">{a.target}</span>
            </li>
          ))}
          {audit.data?.items.length === 0 && <li className="px-4 py-3 text-muted-foreground">還沒有紀錄。</li>}
        </ul>
      </section>
    </Page>
  )
}
