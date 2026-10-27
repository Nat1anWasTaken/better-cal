import { useQuery } from '@tanstack/react-query'
import { Plus, Star } from 'lucide-react'
import { Link } from 'react-router'

import { api } from '@/api/client'
import type { AdminSet } from '@/api/types'
import { Empty, ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { Button } from '@/components/ui/button'
import { STATUS_LABELS } from '@/lib/format'

import { problemStatusTone } from './Problems'

export function SetsPage() {
  const q = useQuery({ queryKey: ['admin-sets'], queryFn: () => api.get<AdminSet[]>('/admin/problem-sets') })
  return (
    <Page wide>
      <PageHeader
        title="題目集"
        description="題目集是從題目庫挑選題目並排好順序。同一題可以放進多個題目集；從題目集移除題目不會刪除題目。"
        actions={
          <Button asChild>
            <Link to="/admin/sets/new">
              <Plus aria-hidden />
              新增題目集
            </Link>
          </Button>
        }
      />
      {q.isLoading ? (
        <Loading />
      ) : q.error ? (
        <ErrorState error={q.error} />
      ) : q.data!.length === 0 ? (
        <Empty>還沒有題目集。</Empty>
      ) : (
        <ul className="divide-y rounded-lg border bg-card">
          {q.data!.map((s) => (
            <li key={s.id}>
              <Link to={`/admin/sets/${s.slug}`} className="flex flex-wrap items-center gap-3 px-4 py-3 hover:bg-accent/50">
                <span className="flex-1">
                  <span className="font-medium">{s.title}</span>
                  <span className="ml-2 font-mono text-xs text-muted-foreground">{s.slug}</span>
                </span>
                {s.active && (
                  <span className="inline-flex items-center gap-1 text-sm font-medium text-primary">
                    <Star aria-hidden className="size-4" />
                    目前啟用
                  </span>
                )}
                {!s.listed && <span className="text-xs text-muted-foreground">不公開列出</span>}
                <span className="text-sm text-muted-foreground">{s.items.length} 題</span>
                <StatusPill tone={problemStatusTone(s.status)} label={STATUS_LABELS[s.status] ?? s.status} />
              </Link>
            </li>
          ))}
        </ul>
      )}
    </Page>
  )
}
