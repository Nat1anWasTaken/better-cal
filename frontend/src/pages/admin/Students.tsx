import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Copy, UserPlus, Users } from 'lucide-react'
import { useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { toast } from 'sonner'

import { ApiError, api, qs } from '@/api/client'
import type { AdminUser, BatchResult, Cohort, CreatedUser, Page as PageT } from '@/api/types'
import { Field } from '@/components/admin/Field'
import { Empty, ErrorState, FieldMessage, Loading, Page, PageHeader } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { errorMessage, errorsByPointer } from '@/lib/errors'
import { formatTime } from '@/lib/format'

const NONE = '__none__'

export function useCohorts() {
  return useQuery({ queryKey: ['cohorts'], queryFn: () => api.get<Cohort[]>('/admin/cohorts') })
}

function PasswordList({ users }: { users: CreatedUser[] }) {
  const text = users.map((u) => `${u.user.username}\t${u.initial_password ?? '（自訂密碼）'}`).join('\n')
  return (
    <div className="space-y-2">
      <p className="text-sm text-warning">初始密碼只會顯示這一次，請現在複製下來交給學生。學生第一次登入時會被要求修改密碼。</p>
      <pre className="max-h-60 overflow-auto rounded-md bg-muted p-3 font-mono text-sm">{text}</pre>
      <Button size="sm" variant="outline" onClick={() => navigator.clipboard.writeText(text).then(() => toast.success('已複製'))}>
        <Copy aria-hidden />
        複製全部
      </Button>
    </div>
  )
}

function CreateDialog() {
  const qc = useQueryClient()
  const cohorts = useCohorts()
  const [open, setOpen] = useState(false)
  const [username, setUsername] = useState('')
  const [name, setName] = useState('')
  const [cohort, setCohort] = useState(NONE)
  const [created, setCreated] = useState<CreatedUser | null>(null)
  const [errors, setErrors] = useState<Record<string, string>>({})
  const m = useMutation({
    mutationFn: () => api.post<CreatedUser>('/admin/users', { username, display_name: name, cohort_id: cohort === NONE ? null : cohort }),
    onSuccess: (u) => {
      setCreated(u)
      setErrors({})
      qc.invalidateQueries({ queryKey: ['admin-users'] })
    },
    onError: (e) => setErrors(e instanceof ApiError && e.errors.length ? errorsByPointer(e.errors) : { '/': errorMessage(e) }),
  })
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o)
        if (!o) {
          setCreated(null)
          setUsername('')
          setName('')
        }
      }}
    >
      <DialogTrigger asChild>
        <Button variant="outline">
          <UserPlus aria-hidden />
          新增一位學生
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>新增學生</DialogTitle>
          <DialogDescription>系統會產生一組初始密碼。</DialogDescription>
        </DialogHeader>
        {created ? (
          <PasswordList users={[created]} />
        ) : (
          <div className="space-y-3">
            <Field label="帳號（例如學號）" htmlFor="nu" error={errors['/username']}>
              <Input id="nu" value={username} onChange={(e) => setUsername(e.target.value)} />
            </Field>
            <Field label="顯示名稱" htmlFor="nn">
              <Input id="nn" value={name} onChange={(e) => setName(e.target.value)} />
            </Field>
            <Field label="組別">
              <Select value={cohort} onValueChange={setCohort}>
                <SelectTrigger aria-label="組別">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={NONE}>不分組</SelectItem>
                  {cohorts.data?.map((c) => (
                    <SelectItem key={c.id} value={c.id}>
                      {c.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
            <FieldMessage message={errors['/']} />
            <Button onClick={() => m.mutate()} disabled={!username || m.isPending}>
              建立帳號
            </Button>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}

function BatchDialog() {
  const qc = useQueryClient()
  const cohorts = useCohorts()
  const [open, setOpen] = useState(false)
  const [csv, setCsv] = useState('')
  const [cohort, setCohort] = useState(NONE)
  const [result, setResult] = useState<BatchResult | null>(null)
  const m = useMutation({
    mutationFn: (dry: boolean) => api.post<BatchResult>('/admin/users/batch', { csv, cohort_id: cohort === NONE ? null : cohort, dry_run: dry }),
    onSuccess: (r) => {
      setResult(r)
      qc.invalidateQueries({ queryKey: ['admin-users'] })
    },
    onError: (e) => {
      if (e instanceof ApiError && e.errors.length) setResult({ ok: false, errors: e.errors, created: [] })
      else toast.error(errorMessage(e))
    },
  })
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o)
        if (!o) setResult(null)
      }}
    >
      <DialogTrigger asChild>
        <Button>
          <Users aria-hidden />
          批次新增
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>批次新增學生</DialogTitle>
          <DialogDescription>每行一位：帳號,顯示名稱（例如 11012345,王小明）。以 # 開頭的行會被忽略。</DialogDescription>
        </DialogHeader>
        {result?.created.length ? (
          <PasswordList users={result.created} />
        ) : (
          <div className="space-y-3">
            <Textarea aria-label="學生名單" rows={10} variant="code" value={csv} onChange={(e) => (setCsv(e.target.value), setResult(null))} />
            <Select value={cohort} onValueChange={setCohort}>
              <SelectTrigger aria-label="加入組別">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NONE}>不分組</SelectItem>
                {cohorts.data?.map((c) => (
                  <SelectItem key={c.id} value={c.id}>
                    {c.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {result && (
              <div role="status" className={result.ok ? 'text-success' : 'text-destructive'}>
                {result.ok ? '檢查通過，可以建立。' : '名單有問題，沒有建立任何帳號：'}
                <ul className="list-disc pl-5 text-sm">
                  {result.errors.map((e, i) => (
                    <li key={i}>{e.message}</li>
                  ))}
                </ul>
              </div>
            )}
            <div className="flex gap-2">
              <Button variant="outline" onClick={() => m.mutate(true)} disabled={!csv || m.isPending}>
                先檢查
              </Button>
              <Button onClick={() => m.mutate(false)} disabled={!result?.ok || m.isPending}>
                建立帳號
              </Button>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}

export function StudentsPage() {
  const [params, setParams] = useSearchParams()
  const q = params.get('q') ?? ''
  const cohort = params.get('cohort') ?? ''
  const cursor = params.get('cursor') ?? undefined
  const cohorts = useCohorts()
  const list = useQuery({
    queryKey: ['admin-users', q, cohort, cursor],
    queryFn: () => api.get<PageT<AdminUser>>(`/admin/users${qs({ q, cohort_id: cohort, cursor, limit: 50 })}`),
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
      <PageHeader title="學生帳號" actions={<><CreateDialog /><BatchDialog /></>} />
      <div className="mb-4 flex flex-wrap gap-2">
        <Input aria-label="搜尋帳號" placeholder="搜尋帳號或名稱" className="max-w-xs" defaultValue={q} onChange={(e) => set('q', e.target.value)} />
        <Select value={cohort || NONE} onValueChange={(v) => set('cohort', v === NONE ? '' : v)}>
          <SelectTrigger className="w-44" aria-label="依組別篩選">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={NONE}>所有組別</SelectItem>
            {cohorts.data?.map((c) => (
              <SelectItem key={c.id} value={c.id}>
                {c.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      {list.isLoading ? (
        <Loading />
      ) : list.error ? (
        <ErrorState error={list.error} />
      ) : list.data!.items.length === 0 ? (
        <Empty>沒有符合的帳號。</Empty>
      ) : (
        <ul className="divide-y rounded-lg border bg-card">
          {list.data!.items.map((u) => (
            <li key={u.id}>
              <Link to={`/admin/users/${u.id}`} className="flex flex-wrap items-center gap-3 px-4 py-2.5 hover:bg-accent/50">
                <span className="flex-1">
                  <span className="font-medium">{u.display_name || u.username}</span>
                  <span className="ml-2 font-mono text-xs text-muted-foreground">{u.username}</span>
                </span>
                {u.role === 'owner' && <span className="text-xs text-primary">管理者</span>}
                <span className="text-sm text-muted-foreground">{u.cohort_name ?? '不分組'}</span>
                <span className="w-32 text-sm text-muted-foreground">最近提交 {formatTime(u.last_submission_at)}</span>
                {u.disabled && <StatusPill tone="neutral" label="已停用" />}
              </Link>
            </li>
          ))}
        </ul>
      )}
      {list.data?.next_cursor && (
        <Button variant="outline" className="mt-4" onClick={() => set('cursor', list.data!.next_cursor!)}>
          下一頁
        </Button>
      )}
    </Page>
  )
}
