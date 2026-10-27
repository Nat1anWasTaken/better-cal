import { useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { Link, useParams } from 'react-router'

import { api, qs } from '@/api/client'
import type { Page as PageT, SubmissionSummary, SubmissionView } from '@/api/types'
import { Empty, ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { SubmissionResult } from '@/components/SubmissionResult'
import { Button } from '@/components/ui/button'
import { CodeEditor } from '@/components/CodeEditor'
import { formatTime, verdictInfo } from '@/lib/format'

export function SubmissionRow({ s, to }: { s: SubmissionSummary & { username?: string }; to: string }) {
  const v = s.status === 'done' || s.status === 'system_error' ? verdictInfo(s.verdict) : verdictInfo(null)
  return (
    <li>
      <Link to={to} className="flex flex-wrap items-center gap-x-4 gap-y-1 px-4 py-3 hover:bg-accent/60">
        <span className="w-28 text-sm text-muted-foreground tabular-nums">{formatTime(s.created_at)}</span>
        <span className="min-w-40 flex-1">
          {s.username && <span className="mr-2 text-sm text-muted-foreground">{s.username}</span>}
          <span className="font-medium">{s.problem_title}</span>
          <span className="ml-2 text-xs text-muted-foreground">版本 {s.revision_number}</span>
        </span>
        <span className="text-sm text-muted-foreground">{s.kind === 'submit' ? '提交' : s.kind === 'run' ? '執行' : '測試'}</span>
        {s.total != null && s.total > 0 && s.verdict !== 'compile_error' && (
          <span className="text-sm tabular-nums">
            {s.passed}/{s.total}
          </span>
        )}
        <StatusPill tone={v.tone} label={v.label} />
      </Link>
    </li>
  )
}

export function MySubmissionsPage() {
  const q = useInfiniteQuery({
    queryKey: ['my-submissions'],
    queryFn: ({ pageParam }) => api.get<PageT<SubmissionSummary>>(`/me/submissions${qs({ limit: 30, cursor: pageParam })}`),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.next_cursor ?? undefined,
  })
  const items = q.data?.pages.flatMap((p) => p.items) ?? []
  return (
    <Page>
      <PageHeader title="我的提交" description="每一次執行與提交都會留下紀錄，點進去可以看到當時的程式和說明。" />
      {q.isLoading ? (
        <Loading />
      ) : q.error ? (
        <ErrorState error={q.error} />
      ) : items.length === 0 ? (
        <Empty>還沒有任何紀錄。打開一題、寫好程式後按「執行」試試看。</Empty>
      ) : (
        <>
          <ul className="divide-y overflow-hidden rounded-lg border bg-card">
            {items.map((s) => (
              <SubmissionRow key={s.id} s={s} to={`/me/submissions/${s.id}`} />
            ))}
          </ul>
          {q.hasNextPage && (
            <Button variant="outline" className="mt-4" onClick={() => q.fetchNextPage()} disabled={q.isFetchingNextPage}>
              載入更多
            </Button>
          )}
        </>
      )}
    </Page>
  )
}

export function SubmissionDetailPage({ admin = false }: { admin?: boolean }) {
  const { id = '' } = useParams()
  const q = useQuery({
    queryKey: ['submission', id, admin],
    queryFn: () => api.get<SubmissionView>(admin ? `/admin/submissions/${id}` : `/submissions/${id}`),
    refetchInterval: (query) => (query.state.data && ['done', 'system_error'].includes(query.state.data.status) ? false : 1500),
  })
  if (q.isLoading) return <Loading />
  if (q.error)
    return (
      <Page>
        <ErrorState error={q.error} />
      </Page>
    )
  const s = q.data!
  return (
    <Page wide>
      <PageHeader
        back={
          <Link to={admin ? '/admin/submissions' : '/me/submissions'} className="text-sm text-muted-foreground hover:text-foreground">
            {admin ? '所有提交' : '我的提交'}
          </Link>
        }
        title={s.problem_title}
        description={`${formatTime(s.created_at)} 送出，依據題目版本 ${s.revision_number} 評測。`}
        actions={
          !admin && (
            <Button asChild variant="outline" size="sm">
              <Link to={`/problems/${s.problem_slug}`}>回到這一題</Link>
            </Button>
          )
        }
      />
      <div className="grid gap-6 lg:grid-cols-2">
        <section aria-label="當時的程式" className="space-y-2">
          <h2 className="font-medium">當時的程式</h2>
          <div className="h-112 overflow-hidden rounded-lg border">
            <CodeEditor initialDoc={s.assembled_source ?? s.code} readOnly className="h-full" ariaLabel="當時的程式（唯讀）" />
          </div>
          {s.custom_stdin != null && (
            <div>
              <div className="mb-1 text-sm text-muted-foreground">自訂輸入</div>
              <pre className="rounded-md border bg-code p-2 font-mono text-sm whitespace-pre-wrap">{s.custom_stdin}</pre>
            </div>
          )}
          {admin && s.last_error && (
            <p className="text-sm text-muted-foreground">
              評測服務訊息（第 {s.attempts} 次嘗試）：{s.last_error}
            </p>
          )}
        </section>
        <section aria-label="結果" className="space-y-2">
          <h2 className="font-medium">結果與說明</h2>
          <SubmissionResult submission={s} />
        </section>
      </div>
    </Page>
  )
}
