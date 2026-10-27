import { useQuery } from '@tanstack/react-query'
import { useSearchParams } from 'react-router'

import { api, qs } from '@/api/client'
import type { AdminSubmission, Page as PageT } from '@/api/types'
import { Empty, ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { VERDICTS } from '@/lib/format'
import { SubmissionRow } from '@/pages/MySubmissions'

const ALL = '__all__'

export function AdminSubmissionsPage() {
  const [params, setParams] = useSearchParams()
  const filters = { user_id: params.get('user_id') ?? '', problem_id: params.get('problem_id') ?? '', verdict: params.get('verdict') ?? '', kind: params.get('kind') ?? '' }
  const cursor = params.get('cursor') ?? undefined
  const q = useQuery({
    queryKey: ['admin-submissions', filters, cursor],
    queryFn: () => api.get<PageT<AdminSubmission>>(`/admin/submissions${qs({ ...filters, cursor, limit: 50 })}`),
    refetchInterval: 10_000,
  })
  const set = (k: string, v: string) => {
    const n = new URLSearchParams(params)
    if (v && v !== ALL) n.set(k, v)
    else n.delete(k)
    n.delete('cursor')
    setParams(n, { replace: true })
  }
  return (
    <Page wide>
      <PageHeader title="提交紀錄" description="所有學生的執行與提交。點進去可以看到隱藏測資的結果、實際編譯的程式和評測時使用的題目版本。" />
      <div className="mb-4 flex flex-wrap gap-2">
        <Select value={filters.verdict || ALL} onValueChange={(v) => set('verdict', v)}>
          <SelectTrigger className="w-44" aria-label="結果篩選">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>所有結果</SelectItem>
            {Object.entries(VERDICTS)
              .filter(([k]) => k !== 'skipped')
              .map(([k, v]) => (
                <SelectItem key={k} value={k}>
                  {v.label}
                </SelectItem>
              ))}
          </SelectContent>
        </Select>
        <Select value={filters.kind || ALL} onValueChange={(v) => set('kind', v)}>
          <SelectTrigger className="w-32" aria-label="種類篩選">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>全部種類</SelectItem>
            <SelectItem value="run">執行</SelectItem>
            <SelectItem value="submit">提交</SelectItem>
            <SelectItem value="test_run">管理者試跑</SelectItem>
          </SelectContent>
        </Select>
        {(filters.user_id || filters.problem_id) && (
          <Button variant="ghost" onClick={() => setParams({}, { replace: true })}>
            清除學生／題目篩選
          </Button>
        )}
      </div>
      {q.isLoading ? (
        <Loading />
      ) : q.error ? (
        <ErrorState error={q.error} />
      ) : q.data!.items.length === 0 ? (
        <Empty>沒有符合的紀錄。</Empty>
      ) : (
        <ul className="divide-y overflow-hidden rounded-lg border bg-card">
          {q.data!.items.map((s) => (
            <SubmissionRow key={s.id} s={{ ...s, problem_id: '' }} to={`/admin/submissions/${s.id}`} />
          ))}
        </ul>
      )}
      {q.data?.next_cursor && (
        <Button variant="outline" className="mt-4" onClick={() => set('cursor', q.data!.next_cursor!)}>
          下一頁
        </Button>
      )}
    </Page>
  )
}
