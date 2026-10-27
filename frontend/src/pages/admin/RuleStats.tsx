import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { Link } from 'react-router'

import { api } from '@/api/client'
import type { RuleStat } from '@/api/types'
import { Empty, ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

export function RuleStatsPage() {
  const [days, setDays] = useState('7')
  const q = useQuery({ queryKey: ['rule-stats', days], queryFn: () => api.get<RuleStat[]>(`/admin/stats/rules?days=${days}`) })
  const max = Math.max(1, ...(q.data ?? []).map((r) => r.count))
  return (
    <Page wide>
      <PageHeader
        title="常見錯誤統計"
        description="學生最常遇到哪些說明卡、展開了多少提示。可以用來決定先改善哪些教學文案。"
        actions={
          <Select value={days} onValueChange={setDays}>
            <SelectTrigger className="w-32" aria-label="統計期間">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="7">近 7 天</SelectItem>
              <SelectItem value="30">近 30 天</SelectItem>
              <SelectItem value="120">近 120 天</SelectItem>
            </SelectContent>
          </Select>
        }
      />
      {q.isLoading ? (
        <Loading />
      ) : q.error ? (
        <ErrorState error={q.error} />
      ) : q.data!.length === 0 ? (
        <Empty>這段期間還沒有資料。</Empty>
      ) : (
        <table className="w-full text-sm">
          <thead className="text-left text-muted-foreground">
            <tr>
              <th className="py-2 font-normal">規則</th>
              <th className="py-2 font-normal">出現次數</th>
              <th className="py-2 font-normal">學生數</th>
              <th className="py-2 font-normal">展開提示</th>
            </tr>
          </thead>
          <tbody className="divide-y">
            {q.data!.map((r) => (
              <tr key={r.rule}>
                <td className="py-2 pr-4">
                  <Link to={r.rule.startsWith('custom.') ? `/admin/catalog/custom/${r.rule}` : `/admin/catalog/rules/${r.rule}`} className="font-mono text-xs hover:underline">
                    {r.rule}
                  </Link>
                </td>
                <td className="w-1/3 py-2 pr-4">
                  <div className="flex items-center gap-2">
                    <div className="h-2 w-(--bar) rounded-sm bg-chart-1" style={{ '--bar': `${(r.count / max) * 100}%` } as React.CSSProperties} aria-hidden />
                    <span className="tabular-nums">{r.count}</span>
                  </div>
                </td>
                <td className="py-2 tabular-nums">{r.students}</td>
                <td className="py-2 tabular-nums">{r.hints_revealed}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Page>
  )
}
