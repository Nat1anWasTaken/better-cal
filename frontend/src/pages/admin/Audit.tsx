import { useInfiniteQuery } from '@tanstack/react-query'

import { api, qs } from '@/api/client'
import type { AuditEntry, Page as PageT } from '@/api/types'
import { Empty, ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { Button } from '@/components/ui/button'
import { formatTime } from '@/lib/format'

export function AuditPage() {
  const q = useInfiniteQuery({
    queryKey: ['audit'],
    queryFn: ({ pageParam }) => api.get<PageT<AuditEntry>>(`/admin/audit${qs({ limit: 50, cursor: pageParam })}`),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (l) => l.next_cursor ?? undefined,
  })
  const items = q.data?.pages.flatMap((p) => p.items) ?? []
  return (
    <Page wide>
      <PageHeader title="操作紀錄" description="誰在什麼時候改了什麼，包含透過 API token 的操作。" />
      {q.isLoading ? (
        <Loading />
      ) : q.error ? (
        <ErrorState error={q.error} />
      ) : items.length === 0 ? (
        <Empty>還沒有紀錄。</Empty>
      ) : (
        <div className="overflow-x-auto rounded-lg border bg-card">
          <table className="w-full text-sm">
            <thead className="border-b text-left text-muted-foreground">
              <tr>
                <th className="px-3 py-2 font-normal">時間</th>
                <th className="px-3 py-2 font-normal">操作者</th>
                <th className="px-3 py-2 font-normal">動作</th>
                <th className="px-3 py-2 font-normal">對象</th>
                <th className="px-3 py-2 font-normal">細節</th>
              </tr>
            </thead>
            <tbody className="divide-y">
              {items.map((a) => (
                <tr key={a.id}>
                  <td className="px-3 py-2 whitespace-nowrap text-muted-foreground">{formatTime(a.at)}</td>
                  <td className="px-3 py-2">
                    {a.actor ?? '—'}
                    {a.via_token && <div className="text-xs text-muted-foreground">token：{a.via_token}</div>}
                  </td>
                  <td className="px-3 py-2 font-mono text-xs">{a.action}</td>
                  <td className="px-3 py-2">{a.target}</td>
                  <td className="max-w-sm truncate px-3 py-2 font-mono text-xs text-muted-foreground" title={JSON.stringify(a.detail)}>
                    {JSON.stringify(a.detail)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {q.hasNextPage && (
        <Button variant="outline" className="mt-4" onClick={() => q.fetchNextPage()}>
          載入更多
        </Button>
      )}
    </Page>
  )
}
