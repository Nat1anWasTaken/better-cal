import { useQuery } from '@tanstack/react-query'
import { Check, Snowflake } from 'lucide-react'
import { Link, useNavigate, useParams } from 'react-router'

import { ApiError, api } from '@/api/client'
import type { BoardRow, Leaderboard, Round } from '@/api/types'
import { Empty, ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useAuth } from '@/hooks/auth'
import { BADGES, badgeInfo } from '@/lib/badges'
import { formatTime } from '@/lib/format'

const CURRENT = '__current__'

export function useRounds() {
  return useQuery({ queryKey: ['rounds'], queryFn: () => api.get<Round[]>('/rounds') })
}

/** The open round's board (`/leaderboard`) or a past round's (`/rounds/:roundId`). */
export function LeaderboardPage() {
  const { roundId } = useParams()
  const nav = useNavigate()
  const rounds = useRounds()
  const q = useQuery({
    queryKey: ['leaderboard', roundId ?? CURRENT],
    queryFn: () => api.get<Leaderboard>(roundId ? `/rounds/${roundId}/leaderboard` : '/leaderboard'),
    refetchInterval: 30_000,
    retry: (n, e) => !(e instanceof ApiError && e.status === 404) && n < 3,
  })
  const noOpenRound = !roundId && q.error instanceof ApiError && q.error.status === 404

  const picker = rounds.data && rounds.data.length > 0 && (
    <Select value={roundId ?? CURRENT} onValueChange={(v) => nav(v === CURRENT ? '/leaderboard' : `/rounds/${v}`)}>
      <SelectTrigger className="w-60" aria-label="選擇輪次">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={CURRENT}>進行中的一輪</SelectItem>
        {rounds.data.map((r) => (
          <SelectItem key={r.id} value={r.id}>
            {r.title}（{formatTime(r.started_at)}）
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )

  if (q.isLoading) return <Loading />
  if (noOpenRound)
    return (
      <Page>
        <PageHeader title="排行榜" actions={picker || undefined} />
        <Empty>目前沒有進行中的一輪。{rounds.data?.length ? '可以從右上角查看之前的輪次。' : '等管理者開始新一輪後，這裡就會出現排名。'}</Empty>
      </Page>
    )
  if (q.error) return <Page><ErrorState error={q.error}><Link className="underline" to="/leaderboard">回到排行榜</Link></ErrorState></Page>
  const b = q.data!
  const r = b.round
  return (
    <Page wide>
      <PageHeader
        title={r.title || '排行榜'}
        description={
          <span className="text-sm">
            <Link to={`/sets/${r.set_slug}`} className="hover:underline">
              {r.set_title}
            </Link>
            {' · '}開始於 {formatTime(r.started_at)}
            {r.ended_at && <> · 已於 {formatTime(r.ended_at)} 結束</>}
          </span>
        }
        actions={picker || undefined}
      />
      {r.frozen_at && (
        <p className="-mt-3 mb-5 inline-flex items-center gap-1.5 text-sm text-primary">
          <Snowflake aria-hidden className="size-4" />
          {b.frozen
            ? `排行已在 ${formatTime(r.frozen_at)} 凍結，之後的結果會在揭曉時公布。`
            : `已凍結：學生看到的是 ${formatTime(r.frozen_at)} 的排名。`}
        </p>
      )}
      {b.rows.length === 0 ? (
        <Empty>這一輪還沒有人提交。第一個通過的人會拿到「首殺」！</Empty>
      ) : (
        <Board board={b} />
      )}
      <Legend penalty={b.penalty_per_wrong} />
    </Page>
  )
}

function Board({ board }: { board: Leaderboard }) {
  const { me } = useAuth()
  const myId = me?.user.id
  return (
    <div className="overflow-x-auto rounded-lg border bg-card">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b text-muted-foreground">
            <th className="w-12 px-3 py-2 text-right font-normal">名次</th>
            <th className="sticky left-0 bg-card px-3 py-2 text-left font-normal">名稱</th>
            <th className="px-3 py-2 font-normal">通過</th>
            <th className="px-3 py-2 font-normal" title="分鐘">罰時</th>
            {board.problems.map((p, i) => (
              <th key={p.id} className="min-w-16 px-2 py-2 text-center font-normal" title={p.title}>
                {i + 1}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y">
          {board.rows.map((row) => {
            const mine = row.user_id === myId
            return (
              <tr key={row.user_id} className={mine ? 'bg-accent/60' : undefined} aria-current={mine ? 'true' : undefined}>
                <td className="px-3 py-2 text-right font-semibold tabular-nums">{row.rank}</td>
                <td className={`sticky left-0 px-3 py-2 ${mine ? 'bg-accent font-medium' : 'bg-card'}`}>
                  {row.name}
                  {mine && <span className="ml-1 text-xs text-muted-foreground">（你）</span>}
                </td>
                <td className="px-3 py-2 text-center tabular-nums">{row.solved}</td>
                <td className="px-3 py-2 text-center tabular-nums">{row.penalty}</td>
                {row.cells.map((c, i) => (
                  <td key={c.problem_id} className="px-2 py-2 text-center">
                    <CellView cell={c} badges={row.badges} title={board.problems[i].title} />
                  </td>
                ))}
              </tr>
            )
          })}
        </tbody>
      </table>
      <ol className="border-t px-3 py-2 text-xs text-muted-foreground">
        {board.problems.map((p, i) => (
          <li key={p.id} className="inline-block pr-4">
            {i + 1}. {p.title}
          </li>
        ))}
      </ol>
    </div>
  )
}

function CellView({ cell, badges, title }: { cell: BoardRow['cells'][number]; badges: BoardRow['badges']; title: string }) {
  const mine = badges.filter((b) => b.problem_id === cell.problem_id)
  const tries = cell.wrong_tries > 0 ? `，錯誤 ${cell.wrong_tries} 次` : ''
  const pending = cell.pending > 0 && (
    <span className="text-xs text-warning tabular-nums" title={`${title}：凍結後提交 ${cell.pending} 次，結果揭曉時公布`}>
      ?{cell.pending}
    </span>
  )
  if (cell.solved)
    return (
      <span className="inline-flex flex-col items-center leading-tight" title={`${title}：第 ${cell.minutes} 分鐘通過${tries}`}>
        <span className="inline-flex items-center gap-0.5 text-success">
          <Check aria-hidden className="size-4" />
          <span className="text-xs tabular-nums">{cell.minutes}′</span>
        </span>
        <span className="inline-flex items-center gap-0.5">
          {cell.wrong_tries > 0 && <span className="text-xs text-muted-foreground tabular-nums">+{cell.wrong_tries}</span>}
          {mine.map((b) => {
            const info = badgeInfo(b.kind)
            return <info.icon key={b.kind} aria-label={info.label} className={`size-3.5 ${info.cls}`} />
          })}
        </span>
      </span>
    )
  if (cell.wrong_tries > 0)
    return (
      <span className="inline-flex flex-col items-center leading-tight">
        <span className="text-xs text-destructive tabular-nums" title={`${title}：尚未通過${tries}`}>
          −{cell.wrong_tries}
        </span>
        {pending}
      </span>
    )
  return pending || null
}

function Legend({ penalty }: { penalty: number }) {
  return (
    <div className="mt-4 space-y-2 text-sm text-muted-foreground">
      <div className="flex flex-wrap gap-4">
        {Object.entries(BADGES).map(([kind, b]) => (
          <span key={kind} className="inline-flex items-center gap-1" title={b.desc}>
            <b.icon aria-hidden className={`size-4 ${b.cls}`} />
            {b.label}：{b.desc}
          </span>
        ))}
      </div>
      <p>
        排名先比通過題數，再比罰時。罰時是每題從本輪開始到通過所花的分鐘數，加上通過前每次錯誤提交 {penalty} 分鐘；編譯失敗與「執行」不算錯誤。
      </p>
    </div>
  )
}
