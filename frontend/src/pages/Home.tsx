import { useQuery } from '@tanstack/react-query'
import { Link, Navigate, useSearchParams } from 'react-router'

import { api } from '@/api/client'
import type { SetSummary } from '@/api/types'
import { Markdown } from '@/components/Markdown'
import { Empty, ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { useSite } from '@/hooks/useSite'

function Meter({ value, max }: { value: number; max: number }) {
  const pct = max ? Math.round((value / max) * 100) : 0
  return (
    <div className="flex items-center gap-3">
      <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted" aria-hidden>
        <div className="h-full w-(--meter) rounded-full bg-success" style={{ '--meter': `${pct}%` } as React.CSSProperties} />
      </div>
      <span className="text-sm text-muted-foreground tabular-nums">
        已通過 {value} / {max} 題
      </span>
    </div>
  )
}

/** Landing: jump straight into the default set; `?all=1` shows every visible set. */
export function HomePage() {
  const site = useSite()
  const [params] = useSearchParams()
  const sets = useQuery({ queryKey: ['sets'], queryFn: () => api.get<SetSummary[]>('/problem-sets') })
  if (site.isLoading || sets.isLoading) return <Loading />
  if (sets.error) return <ErrorState error={sets.error} />
  const target = site.data?.default_set_id
  if (!params.get('all') && target && sets.data?.some((s) => s.id === target)) {
    return <Navigate to={`/sets/${target}`} replace />
  }
  return (
    <Page>
      <PageHeader title="題目集" description="選一個題目集開始練習。" />
      {site.data?.announcement_md && (
        <section aria-label="公告" className="mb-6 rounded-lg border bg-card p-4">
          <Markdown>{site.data.announcement_md}</Markdown>
        </section>
      )}
      {sets.data?.length === 0 ? (
        <Empty>目前還沒有開放的題目集。讀書會的管理者發布題目後就會出現在這裡。</Empty>
      ) : (
        <ul className="grid gap-3 sm:grid-cols-2">
          {sets.data?.map((s) => (
            <li key={s.id}>
              <Link to={`/sets/${s.id}`} className="block h-full rounded-lg border bg-card p-4 hover:border-primary/50">
                <div className="mb-1 flex items-center gap-2">
                  <h2 className="font-medium">{s.title}</h2>
                  {s.active && <span className="rounded-md bg-primary-soft px-1.5 py-0.5 text-xs text-primary">本週進行中</span>}
                </div>
                {s.description_md && <Markdown className="mb-3 line-clamp-2 text-sm text-muted-foreground">{s.description_md}</Markdown>}
                <Meter value={s.solved_count} max={s.problem_count} />
              </Link>
            </li>
          ))}
        </ul>
      )}
    </Page>
  )
}
