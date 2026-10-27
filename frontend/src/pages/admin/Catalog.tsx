import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { PencilLine, Plus, Send } from 'lucide-react'
import { useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { toast } from 'sonner'

import { api } from '@/api/client'
import type { CatalogOverview, HistoryEntry } from '@/api/types'
import { PreviewPanel } from '@/components/admin/PreviewPanel'
import { ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { errorMessage } from '@/lib/errors'
import { formatTime, templatePreview } from '@/lib/format'

const GROUPS: Record<string, string> = {
  syntax: '語法',
  ident: '名稱與宣告',
  func: '函式',
  format: 'printf / scanf',
  link: '連結',
  warn: '警告',
  lint: '程式習慣檢查',
  run: '執行期',
  judge: '評測結果',
  compile: '編譯',
  fallback: '通用備援',
  custom: '自訂規則',
}

export function CatalogPage() {
  const qc = useQueryClient()
  const nav = useNavigate()
  const [filter, setFilter] = useState('')
  const q = useQuery({ queryKey: ['catalog'], queryFn: () => api.get<CatalogOverview>('/admin/catalog') })
  const history = useQuery({ queryKey: ['catalog-history'], queryFn: () => api.get<HistoryEntry[]>('/admin/catalog/history') })
  const publish = useMutation({
    mutationFn: () => api.post<{ published_keys: string[] }>('/admin/catalog/publish'),
    onSuccess: (r) => {
      toast.success(`已發布 ${r.published_keys.length} 項修改，學生立即看到新的說明`)
      qc.invalidateQueries({ queryKey: ['catalog'] })
      qc.invalidateQueries({ queryKey: ['catalog-history'] })
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  if (q.isLoading) return <Loading />
  if (q.error) return <Page><ErrorState error={q.error} /></Page>
  const o = q.data!
  const rules = o.rules.filter((r) => !filter || r.id.includes(filter) || r.title.includes(filter))
  const grouped = Object.entries(GROUPS).map(([k, label]) => ({ label, rules: rules.filter((r) => r.id.split('.')[0] === k) })).filter((g) => g.rules.length)

  return (
    <Page wide>
      <PageHeader
        title="教學文案"
        description="這裡是錯誤說明卡片的文字。判斷「是哪一種錯誤」由系統的規則負責；每種錯誤要怎麼對學生說明，都可以在這裡修改。修改先存成草稿，按「發布全部修改」後立即生效。"
        actions={
          <>
            <Button variant="outline" asChild>
              <Link to="/admin/catalog/texts">共用文字</Link>
            </Button>
            <Button variant="outline" onClick={() => nav('/admin/catalog/custom/new')}>
              <Plus aria-hidden />
              自訂規則
            </Button>
            <Button onClick={() => publish.mutate()} disabled={o.pending_changes === 0 || publish.isPending} data-testid="publish-catalog">
              <Send aria-hidden />
              發布全部修改（{o.pending_changes}）
            </Button>
          </>
        }
      />
      <Input aria-label="搜尋規則" placeholder="搜尋規則 id 或標題" className="mb-4 max-w-xs" value={filter} onChange={(e) => setFilter(e.target.value)} />
      <div className="space-y-6">
        {grouped.map((g) => (
          <section key={g.label}>
            <h2 className="mb-2 font-medium">{g.label}</h2>
            <ul className="divide-y rounded-lg border bg-card">
              {g.rules.map((r) => (
                <li key={r.id}>
                  <Link to={r.state === 'custom' ? `/admin/catalog/custom/${r.id}` : `/admin/catalog/rules/${r.id}`} className="flex flex-wrap items-center gap-3 px-4 py-2.5 hover:bg-accent/50">
                    <span className="min-w-0 flex-1">
                      <span className="block truncate">{templatePreview(r.title)}</span>
                      <span className="font-mono text-xs text-muted-foreground">{r.id}</span>
                    </span>
                    {r.has_draft && (
                      <span className="inline-flex items-center gap-1 text-xs text-warning">
                        <PencilLine aria-hidden className="size-3.5" />
                        有草稿
                      </span>
                    )}
                    {r.state === 'edited' && <span className="text-xs text-muted-foreground">已修改</span>}
                    {!r.enabled && <StatusPill tone="neutral" label="停用" />}
                    <span className="w-24 text-right text-xs text-muted-foreground tabular-nums">近 7 天 {r.hits_7d} 次</span>
                  </Link>
                </li>
              ))}
            </ul>
          </section>
        ))}
        <PreviewPanel />
        <section>
          <h2 className="mb-2 font-medium">發布紀錄</h2>
          <ul className="divide-y rounded-lg border bg-card text-sm">
            {history.data?.slice(0, 30).map((h) => (
              <li key={h.id} className="flex flex-wrap gap-3 px-4 py-2">
                <span className="w-28 text-muted-foreground">{formatTime(h.created_at)}</span>
                <span className="font-mono text-xs">{h.key}</span>
                <span className="text-muted-foreground">{h.published == null ? '恢復成內建／移除' : '發布新內容'}</span>
                <span className="ml-auto">{h.created_by}</span>
              </li>
            ))}
            {history.data?.length === 0 && <li className="px-4 py-3 text-muted-foreground">還沒有發布過修改。</li>}
          </ul>
        </section>
      </div>
    </Page>
  )
}
