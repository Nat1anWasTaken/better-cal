import { useMutation, useQuery } from '@tanstack/react-query'
import { ShieldCheck } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useSearchParams } from 'react-router'

import { api } from '@/api/client'
import type { AuthDecisionResult, AuthRequestView } from '@/api/types'
import { ErrorState, FieldMessage, Loading } from '@/components/Page'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Label } from '@/components/ui/label'
import { useAuth } from '@/hooks/auth'
import { errorMessage } from '@/lib/errors'
import { SCOPES, knownScopes } from '@/lib/scopes'

function Shell({ children }: { children: React.ReactNode }) {
  return (
    <main className="mx-auto flex min-h-svh max-w-lg flex-col justify-center gap-6 px-4 py-10">
      <div className="space-y-6 rounded-xl border bg-card p-6 shadow-sm">{children}</div>
    </main>
  )
}

/** Where an MCP client (Claude, ChatGPT…) sends the owner to approve access. */
export function OAuthConsentPage() {
  const { isOwner } = useAuth()
  const [params] = useSearchParams()
  const id = params.get('request') ?? ''
  const q = useQuery({ queryKey: ['oauth-request', id], queryFn: () => api.get<AuthRequestView>(`/oauth/requests/${id}`), enabled: isOwner && !!id, retry: false })
  const [scopes, setScopes] = useState<string[]>([])
  useEffect(() => {
    if (q.data) setScopes(knownScopes(q.data.requested_scopes))
  }, [q.data])
  const decide = useMutation({
    mutationFn: (approve: boolean) => api.post<AuthDecisionResult>(`/oauth/requests/${id}`, { approve, scopes }),
    // A plain navigation: the CSP's form-action would block a cross-origin form redirect.
    onSuccess: (r) => window.location.assign(r.redirect_to),
  })

  if (!isOwner) {
    return (
      <Shell>
        <h1 className="text-xl font-semibold">需要擁有者帳號</h1>
        <p className="text-muted-foreground">只有網站擁有者可以授權應用程式存取管理功能。請登出後改用擁有者帳號登入。</p>
      </Shell>
    )
  }
  if (!id) {
    return (
      <Shell>
        <ErrorState error={new Error('缺少授權請求，請回到應用程式重新連線。')} />
      </Shell>
    )
  }
  if (q.isLoading) return <Loading />
  if (q.error) {
    return (
      <Shell>
        <ErrorState error={q.error} />
      </Shell>
    )
  }
  const r = q.data!
  const done = decide.isPending || decide.isSuccess
  return (
    <Shell>
      <div className="flex items-start gap-3">
        <ShieldCheck aria-hidden className="mt-1 size-6 shrink-0 text-primary" />
        <div className="space-y-1">
          <h1 className="text-xl font-semibold">「{r.client_name}」想要存取管理功能</h1>
          <p className="text-sm text-muted-foreground">
            它會透過 MCP 以你的身分操作，範圍限於下面勾選的權限。之後可以在「管理 → API 與 MCP」隨時中斷連線。
          </p>
        </div>
      </div>
      <dl className="grid grid-cols-1 gap-x-4 gap-y-1 text-sm sm:grid-cols-2">
        <dt className="text-muted-foreground">授權後返回</dt>
        <dd>
          <strong className="font-mono" data-testid="redirect-host">
            {r.redirect_host}
          </strong>
        </dd>
        <dt className="text-muted-foreground">應用程式 ID</dt>
        <dd className="truncate font-mono text-xs leading-5" title={r.client_id}>
          {r.client_id}
        </dd>
      </dl>
      <p className="text-sm">只有在你認得上面的網域時才允許。</p>
      <fieldset className="space-y-2">
        <legend className="mb-2 text-sm font-medium">允許的權限</legend>
        {SCOPES.map((s) => (
          <div key={s.id} className="flex items-center gap-2">
            <Checkbox id={`cs-${s.id}`} checked={scopes.includes(s.id)} onCheckedChange={(on) => setScopes(on ? [...scopes, s.id] : scopes.filter((x) => x !== s.id))} />
            <Label htmlFor={`cs-${s.id}`} variant="option">
              {s.label} <code className="font-mono text-xs text-muted-foreground">{s.id}</code>
              {r.requested_scopes.includes(s.id) && <span className="ml-1 text-xs text-muted-foreground">（應用程式要求）</span>}
            </Label>
          </div>
        ))}
      </fieldset>
      <FieldMessage message={decide.error ? errorMessage(decide.error) : undefined} />
      <div className="flex flex-wrap justify-end gap-2">
        <Button variant="outline" disabled={done} onClick={() => decide.mutate(false)}>
          拒絕
        </Button>
        <Button disabled={done || scopes.length === 0} onClick={() => decide.mutate(true)}>
          允許
        </Button>
      </div>
    </Shell>
  )
}
