import { useQuery } from '@tanstack/react-query'
import { Plus, Puzzle } from 'lucide-react'
import { Link, useSearchParams } from 'react-router'

import { api, qs } from '@/api/client'
import type { Page as PageT, ProblemSummary } from '@/api/types'
import { Empty, ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { STATUS_LABELS, formatTime } from '@/lib/format'

export function problemStatusTone(s: string) {
  return s === 'published' ? 'success' : s === 'archived' ? 'neutral' : 'warning'
}

export function ProblemsPage() {
  const [params, setParams] = useSearchParams()
  const q = params.get('q') ?? ''
  const status = params.get('status') ?? ''
  const cursor = params.get('cursor') ?? undefined
  const list = useQuery({
    queryKey: ['admin-problems', q, status, cursor],
    queryFn: () => api.get<PageT<ProblemSummary>>(`/admin/problems${qs({ q, status, cursor, limit: 50 })}`),
  })
  const set = (k: string, v: string) => {
    const n = new URLSearchParams(params)
    if (v) n.set(k, v)
    else n.delete(k)
    n.delete('cursor')
    setParams(n, { replace: true })
  }
  return (
    <Page wide>
      <PageHeader
        title="題目庫"
        description="題目可以被多個題目集共用。修改題目會產生新版本，學生在你發布新版本前看到的仍是舊版本。"
        actions={
          <Button asChild>
            <Link to="/admin/problems/new">
              <Plus aria-hidden />
              新增題目
            </Link>
          </Button>
        }
      />
      <div className="mb-4 flex flex-wrap gap-2">
        <Input aria-label="搜尋題目" placeholder="搜尋名稱或 slug" className="max-w-xs" defaultValue={q} onChange={(e) => set('q', e.target.value)} />
        <Select value={status || 'all'} onValueChange={(v) => set('status', v === 'all' ? '' : v)}>
          <SelectTrigger className="w-36" aria-label="狀態篩選">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">所有狀態</SelectItem>
            <SelectItem value="draft">草稿</SelectItem>
            <SelectItem value="published">已發布</SelectItem>
            <SelectItem value="archived">已封存</SelectItem>
          </SelectContent>
        </Select>
      </div>
      {list.isLoading ? (
        <Loading />
      ) : list.error ? (
        <ErrorState error={list.error} />
      ) : list.data!.items.length === 0 ? (
        <Empty>沒有符合的題目。</Empty>
      ) : (
        <div className="overflow-x-auto rounded-lg border bg-card">
          <table className="w-full text-sm">
            <thead className="border-b text-left text-muted-foreground">
              <tr>
                <th className="px-4 py-2 font-normal">題目</th>
                <th className="px-4 py-2 font-normal">狀態</th>
                <th className="px-4 py-2 font-normal">版本</th>
                <th className="px-4 py-2 font-normal">題目集</th>
                <th className="px-4 py-2 font-normal">更新時間</th>
              </tr>
            </thead>
            <tbody className="divide-y">
              {list.data!.items.map((p) => (
                <tr key={p.id} className="hover:bg-accent/50">
                  <td className="px-4 py-2">
                    <Link to={`/admin/problems/${p.slug}`} className="font-medium hover:underline">
                      {p.title}
                    </Link>
                    {p.mode === 'fill_in' && <Puzzle aria-label="局部補寫" className="ml-1.5 inline size-3.5 text-muted-foreground" />}
                    <div className="font-mono text-xs text-muted-foreground">{p.slug}</div>
                  </td>
                  <td className="px-4 py-2">
                    <StatusPill tone={problemStatusTone(p.status)} label={STATUS_LABELS[p.status] ?? p.status} />
                    {p.has_unpublished_changes && p.status === 'published' && <div className="mt-1 text-xs text-warning">有尚未發布的修改</div>}
                  </td>
                  <td className="px-4 py-2 tabular-nums">
                    最新 {p.head_revision_number}
                    {p.published_revision_number != null && <span className="text-muted-foreground">，學生看到 {p.published_revision_number}</span>}
                  </td>
                  <td className="px-4 py-2 tabular-nums">{p.set_count}</td>
                  <td className="px-4 py-2 text-muted-foreground">{formatTime(p.updated_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {list.data?.next_cursor && (
        <Button variant="outline" className="mt-4" onClick={() => set('cursor', list.data!.next_cursor!)}>
          下一頁
        </Button>
      )}
    </Page>
  )
}
