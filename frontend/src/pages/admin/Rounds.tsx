import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Eye, Flag, Snowflake } from 'lucide-react'
import { Link } from 'react-router'
import { toast } from 'sonner'

import { api } from '@/api/client'
import type { Round } from '@/api/types'
import { Empty, ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { Button } from '@/components/ui/button'
import { errorMessage } from '@/lib/errors'
import { formatTime } from '@/lib/format'

type Action = 'end' | 'freeze' | 'reveal'
const DONE: Record<Action, string> = { end: '這一輪已結束', freeze: '排行已凍結，學生看到的是現在的排名', reveal: '排行已揭曉' }

export function RoundsPage() {
  const qc = useQueryClient()
  const q = useQuery({ queryKey: ['admin-rounds'], queryFn: () => api.get<Round[]>('/admin/rounds') })
  const act = useMutation({
    mutationFn: ({ id, action }: { id: string; action: Action }) => api.post<Round>(`/admin/rounds/${id}/${action}`, {}),
    onSuccess: (_, { action }) => {
      qc.invalidateQueries({ queryKey: ['admin-rounds'] })
      qc.invalidateQueries({ queryKey: ['leaderboard'] })
      qc.invalidateQueries({ queryKey: ['site'] })
      toast.success(DONE[action])
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  return (
    <Page wide>
      <PageHeader
        title="排行輪次"
        description={
          <>
            在題目集頁按「設為目前啟用」時可以同時開始新一輪。凍結後學生只看得到凍結當下的排名，揭曉後才公布之後的結果。
          </>
        }
      />
      {q.isLoading ? (
        <Loading />
      ) : q.error ? (
        <ErrorState error={q.error} />
      ) : !q.data?.length ? (
        <Empty>還沒有任何一輪。到題目集頁按「設為目前啟用」並勾選「開始新一輪排行」。</Empty>
      ) : (
        <ul className="divide-y overflow-hidden rounded-lg border bg-card">
          {q.data.map((r) => (
            <li key={r.id} className="flex flex-wrap items-center gap-3 px-4 py-3">
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-medium">{r.title}</span>
                  <StatusPill tone={r.ended_at ? 'neutral' : 'progress'} label={r.ended_at ? '已結束' : '進行中'} />
                  {r.frozen_at && <StatusPill tone="warning" label={`凍結於 ${formatTime(r.frozen_at)}`} />}
                </div>
                <div className="text-sm text-muted-foreground">
                  {r.set_title} · {formatTime(r.started_at)} 開始{r.ended_at && ` · ${formatTime(r.ended_at)} 結束`}
                </div>
              </div>
              <Button variant="ghost" size="sm" asChild>
                <Link to={`/rounds/${r.id}`}>查看排行</Link>
              </Button>
              {!r.ended_at && !r.frozen_at && (
                <Button variant="outline" size="sm" disabled={act.isPending} onClick={() => act.mutate({ id: r.id, action: 'freeze' })}>
                  <Snowflake aria-hidden />
                  凍結
                </Button>
              )}
              {r.frozen_at && (
                <Button variant="outline" size="sm" disabled={act.isPending} onClick={() => act.mutate({ id: r.id, action: 'reveal' })}>
                  <Eye aria-hidden />
                  揭曉
                </Button>
              )}
              {!r.ended_at && (
                <Button
                  variant="ghost-destructive"
                  size="sm"
                  disabled={act.isPending}
                  onClick={() => window.confirm(`結束「${r.title}」？之後的提交不再計入這一輪。`) && act.mutate({ id: r.id, action: 'end' })}
                >
                  <Flag aria-hidden />
                  結束
                </Button>
              )}
            </li>
          ))}
        </ul>
      )}
    </Page>
  )
}
