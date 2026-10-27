import { useQuery } from '@tanstack/react-query'
import { Check, CircleDashed, Clock, Stamp } from 'lucide-react'
import { useState } from 'react'
import { Link } from 'react-router'

import { api, qs } from '@/api/client'
import type { AdminSet, ProgressMatrix } from '@/api/types'
import { Empty, ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

import { useCohorts } from './Students'

const ALL = '__all__'
const CELL = {
  solved: { icon: Check, label: '通過', cls: 'text-success' },
  credited: { icon: Stamp, label: '認列', cls: 'text-success' },
  attempted: { icon: Clock, label: '嘗試中', cls: 'text-warning' },
  not_started: { icon: CircleDashed, label: '未開始', cls: 'text-muted-foreground/60' },
} as const

export function ProgressMatrixPage() {
  const sets = useQuery({ queryKey: ['admin-sets'], queryFn: () => api.get<AdminSet[]>('/admin/problem-sets') })
  const cohorts = useCohorts()
  const [setSlug, setSetSlug] = useState<string | null>(null)
  const [cohort, setCohort] = useState(ALL)
  const chosen = setSlug ?? sets.data?.find((s) => s.active)?.slug ?? sets.data?.[0]?.slug
  const m = useQuery({
    queryKey: ['matrix', chosen, cohort],
    queryFn: () => api.get<ProgressMatrix>(`/admin/progress${qs({ problem_set: chosen, cohort_id: cohort === ALL ? null : cohort })}`),
    enabled: !!chosen,
  })
  return (
    <Page wide>
      <PageHeader title="進度表" description="每位學生在一個題目集裡各題的狀態。" />
      <div className="mb-4 flex flex-wrap gap-2">
        <Select value={chosen ?? ''} onValueChange={setSetSlug}>
          <SelectTrigger className="w-64" aria-label="題目集">
            <SelectValue placeholder="選擇題目集" />
          </SelectTrigger>
          <SelectContent>
            {sets.data?.map((s) => (
              <SelectItem key={s.id} value={s.slug}>
                {s.title}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select value={cohort} onValueChange={setCohort}>
          <SelectTrigger className="w-44" aria-label="組別">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>所有學生</SelectItem>
            {cohorts.data?.map((c) => (
              <SelectItem key={c.id} value={c.id}>
                {c.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="mb-3 flex flex-wrap gap-4 text-sm">
        {Object.values(CELL).map((c) => (
          <span key={c.label} className={`inline-flex items-center gap-1 ${c.cls}`}>
            <c.icon aria-hidden className="size-4" />
            {c.label}
          </span>
        ))}
      </div>
      {m.isLoading ? (
        <Loading />
      ) : m.error ? (
        <ErrorState error={m.error} />
      ) : !m.data || m.data.rows.length === 0 ? (
        <Empty>沒有學生資料。</Empty>
      ) : (
        <div className="overflow-x-auto rounded-lg border bg-card">
          <table className="text-sm">
            <thead>
              <tr className="border-b">
                <th className="sticky left-0 bg-card px-3 py-2 text-left font-normal text-muted-foreground">學生</th>
                {m.data.problems.map((p, i) => (
                  <th key={p.id} className="px-2 py-2 text-center font-normal text-muted-foreground" title={p.title}>
                    {i + 1}
                  </th>
                ))}
                <th className="px-3 py-2 font-normal text-muted-foreground">通過</th>
              </tr>
            </thead>
            <tbody className="divide-y">
              {m.data.rows.map((r) => (
                <tr key={r.user_id}>
                  <td className="sticky left-0 bg-card px-3 py-1.5">
                    <Link to={`/admin/users/${r.user_id}`} className="hover:underline">
                      {r.display_name || r.username}
                    </Link>
                  </td>
                  {r.cells.map((c, i) => {
                    const cell = CELL[c as keyof typeof CELL] ?? CELL.not_started
                    return (
                      <td key={i} className="px-2 py-1.5 text-center" title={`${m.data!.problems[i].title}：${cell.label}`}>
                        <cell.icon aria-label={cell.label} className={`mx-auto size-4 ${cell.cls}`} />
                      </td>
                    )
                  })}
                  <td className="px-3 py-1.5 text-center tabular-nums">
                    {r.solved}/{m.data!.problems.length}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <ol className="border-t px-3 py-2 text-xs text-muted-foreground">
            {m.data.problems.map((p, i) => (
              <li key={p.id} className="inline-block pr-4">
                {i + 1}. {p.title}
              </li>
            ))}
          </ol>
        </div>
      )}
    </Page>
  )
}
