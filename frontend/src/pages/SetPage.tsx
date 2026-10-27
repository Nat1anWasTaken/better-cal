import { useQuery } from '@tanstack/react-query'
import { Puzzle } from 'lucide-react'
import { Link, useParams } from 'react-router'

import { api } from '@/api/client'
import type { SetDetail } from '@/api/types'
import { Markdown } from '@/components/Markdown'
import { Empty, ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { useSite } from '@/hooks/useSite'
import { PROGRESS } from '@/lib/format'

export function SetPage() {
  const { setId = '' } = useParams()
  const site = useSite()
  const q = useQuery({ queryKey: ['set', setId], queryFn: () => api.get<SetDetail>(`/problem-sets/${setId}`) })
  if (q.isLoading) return <Loading />
  if (q.error) return <Page><ErrorState error={q.error}><Link className="underline" to="/?all=1">回到題目集列表</Link></ErrorState></Page>
  const s = q.data!
  return (
    <Page>
      <PageHeader
        back={
          <Link to="/?all=1" className="text-sm text-muted-foreground hover:text-foreground">
            所有題目集
          </Link>
        }
        title={s.title}
        actions={
          <span className="text-sm text-muted-foreground">
            已通過 {s.solved_count} / {s.problem_count} 題
          </span>
        }
        description={s.description_md ? <Markdown>{s.description_md}</Markdown> : undefined}
      />
      {site.data?.active_set_id === s.id && <p className="-mt-3 mb-5 text-sm text-primary">這是讀書會目前進行中的題目集。</p>}
      {s.problems.length === 0 ? (
        <Empty>這個題目集還沒有題目。</Empty>
      ) : (
        <ol className="divide-y overflow-hidden rounded-lg border bg-card">
          {s.problems.map((p, i) => {
            const pr = PROGRESS[p.progress] ?? PROGRESS.not_started
            return (
              <li key={p.problem_id}>
                <Link to={`/sets/${s.id}/problems/${p.slug}`} className="flex items-center gap-4 px-4 py-3 hover:bg-accent/60">
                  <span className="w-6 text-right font-mono text-sm text-muted-foreground tabular-nums">{i + 1}</span>
                  <span className="flex-1">
                    <span className="font-medium">{p.title}</span>
                    {p.mode === 'fill_in' && (
                      <span className="ml-2 inline-flex items-center gap-1 text-xs text-muted-foreground">
                        <Puzzle aria-hidden className="size-3.5" />
                        局部補寫
                      </span>
                    )}
                  </span>
                  <StatusPill tone={pr.tone} label={pr.label} />
                </Link>
              </li>
            )
          })}
        </ol>
      )}
    </Page>
  )
}
