import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router'

import { api } from '@/api/client'
import type { Progress } from '@/api/types'
import { Empty, ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { PROGRESS, formatTime } from '@/lib/format'

export function MyProgressPage() {
  const q = useQuery({ queryKey: ['my-progress'], queryFn: () => api.get<Progress>('/me/progress') })
  if (q.isLoading) return <Loading />
  if (q.error) return <Page><ErrorState error={q.error} /></Page>
  const p = q.data!
  return (
    <Page>
      <PageHeader title="解題進度" description={`已通過 ${p.solved} 題、嘗試中 ${p.attempted} 題，共 ${p.total} 題。`} />
      {p.problems.length === 0 ? (
        <Empty>目前沒有開放的題目。</Empty>
      ) : (
        <ul className="divide-y overflow-hidden rounded-lg border bg-card">
          {p.problems.map((x) => {
            const pr = PROGRESS[x.status] ?? PROGRESS.not_started
            return (
              <li key={x.problem_id}>
                <Link to={`/problems/${x.slug}`} className="flex flex-wrap items-center gap-4 px-4 py-3 hover:bg-accent/60">
                  <span className="flex-1 font-medium">{x.title}</span>
                  {x.credited && <span className="text-xs text-muted-foreground">（管理者認列）</span>}
                  <span className="text-sm text-muted-foreground">提交 {x.submissions} 次</span>
                  <span className="hidden w-28 text-sm text-muted-foreground sm:inline">{formatTime(x.last_submitted_at)}</span>
                  <StatusPill tone={pr.tone} label={pr.label} />
                </Link>
              </li>
            )
          })}
        </ul>
      )}
    </Page>
  )
}
