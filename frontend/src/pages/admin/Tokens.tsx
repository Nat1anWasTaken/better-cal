import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Bot, Copy, KeyRound, Plus } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { ApiError, api } from '@/api/client'
import type { CreatedToken, McpStatus, SettingsView, TokenInfo } from '@/api/types'
import { Field, SectionCard } from '@/components/admin/Field'
import { Empty, ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { errorMessage, errorsByPointer } from '@/lib/errors'
import { formatTime } from '@/lib/format'
import { AUTHORING_SCOPES, SCOPES } from '@/lib/scopes'

const OPENAPI_URL = `${window.location.origin}/api/v1/openapi.json`

function copy(text: string, what: string) {
  navigator.clipboard.writeText(text).then(() => toast.success(`已複製${what}`))
}

export function TokensPage() {
  const qc = useQueryClient()
  const q = useQuery({ queryKey: ['tokens'], queryFn: () => api.get<TokenInfo[]>('/admin/tokens') })
  const revoke = useMutation({
    mutationFn: (id: string) => api.del(`/admin/tokens/${id}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['tokens'] })
      toast.success('已撤銷，立即失效')
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  const onRevoke = (t: TokenInfo) => window.confirm(`撤銷「${t.name}」？`) && revoke.mutate(t.id)
  return (
    <Page wide>
      <PageHeader
        title="API 與 MCP"
        description="讓 Agent 存取管理功能，不需要保存你的密碼。Claude、ChatGPT 等雲端 Agent 透過 MCP 連線並由你授權；腳本與本機 Agent 可以使用 API token。"
        actions={
          <Button size="sm" variant="outline" onClick={() => copy(OPENAPI_URL, ' OpenAPI 網址')}>
            <Copy aria-hidden />
            複製 OpenAPI 網址
          </Button>
        }
      />
      <div className="space-y-6">
        <McpServerCard />
        <ConnectionsCard tokens={q} onRevoke={onRevoke} />
        <ApiTokensCard tokens={q} onRevoke={onRevoke} />
      </div>
    </Page>
  )
}

function McpServerCard() {
  const qc = useQueryClient()
  const status = useQuery({ queryKey: ['mcp'], queryFn: () => api.get<McpStatus>('/admin/mcp') })
  const toggle = useMutation({
    // Settings are saved as a whole; start from the latest copy.
    mutationFn: async (on: boolean) => {
      const cur = await api.get<SettingsView>('/admin/settings')
      return api.put<SettingsView>('/admin/settings', { ...cur.settings, mcp_enabled: on })
    },
    onSuccess: (v) => {
      qc.setQueryData(['settings'], v)
      qc.invalidateQueries({ queryKey: ['mcp'] })
      toast.success(v.settings.mcp_enabled ? 'MCP 已啟用' : 'MCP 已停用，所有 MCP 連線立即失效')
    },
    onError: (e) => toast.error(errorMessage(e)),
  })
  if (status.isLoading) return <Loading />
  if (status.error) return <ErrorState error={status.error} />
  const s = status.data!
  const curated = s.tools.filter((t) => t.curated)
  return (
    <SectionCard
      title="MCP 伺服器"
      description="以 MCP（Model Context Protocol）提供管理 API。雲端 Agent 第一次連線時會帶你到授權頁，由你決定給哪些權限。"
      actions={
        <div className="flex items-center gap-2">
          <Label htmlFor="mcp-on" variant="option">
            {s.enabled ? '已啟用' : '已停用'}
          </Label>
          <Switch id="mcp-on" checked={s.enabled} disabled={!s.public_url || toggle.isPending} onCheckedChange={(on) => toggle.mutate(on)} />
        </div>
      }
    >
      {!s.public_url ? (
        <div role="alert" className="rounded-lg border border-warning/30 bg-warning-soft p-3 text-sm">
          伺服器沒有設定 <code className="font-mono">PUBLIC_URL</code>（網站對外的網址，例如 <code className="font-mono">https://cal.example.edu</code>）。OAuth 需要它，設定並重新啟動後才能啟用 MCP。
        </div>
      ) : (
        <>
          <Field label="伺服器網址" hint="貼到 Agent 的連線設定。">
            <div className="flex max-w-xl gap-2">
              <Input readOnly variant="code" value={s.server_url ?? ''} data-testid="mcp-url" onFocus={(e) => e.target.select()} />
              <Button variant="outline" onClick={() => copy(s.server_url ?? '', 'MCP 網址')}>
                <Copy aria-hidden />
                複製
              </Button>
            </div>
          </Field>
          <div className="grid gap-4 text-sm md:grid-cols-3">
            <div className="space-y-1">
              <div className="font-medium">Claude</div>
              <p className="text-muted-foreground">設定 → Connectors → Add custom connector，貼上網址後按 Connect，再到這個網站授權。</p>
            </div>
            <div className="space-y-1">
              <div className="font-medium">ChatGPT</div>
              <p className="text-muted-foreground">設定 → Apps → 進階設定開啟 Developer mode → Create app，貼上網址，驗證方式選 OAuth。</p>
            </div>
            <div className="space-y-1">
              <div className="font-medium">Claude Code 等本機 Agent</div>
              <code className="block overflow-x-auto rounded bg-muted p-2 font-mono text-xs whitespace-nowrap">claude mcp add --transport http ccal {s.server_url}</code>
              <p className="text-muted-foreground">也可以改用下方的 API token：加上標頭 Authorization: Bearer &lt;token&gt;。</p>
            </div>
          </div>
        </>
      )}
      <details className="text-sm">
        <summary className="cursor-pointer text-muted-foreground">
          提供 {s.tools.length} 個工具（{curated.length} 個流程工具，其餘對應 REST API）；Agent 只看得到授權範圍內的工具
        </summary>
        <ul className="mt-2 grid gap-x-6 gap-y-1 sm:grid-cols-2 lg:grid-cols-3">
          {s.tools.map((t) => (
            <li key={t.name} className="min-w-0 truncate" title={`${t.title}${t.scopes.length ? `（${t.scopes.join(', ')}）` : ''}`}>
              <code className="font-mono text-xs">{t.name}</code>
              {t.curated && <span className="ml-1 text-xs text-primary">流程</span>}
              {t.read_only && <span className="ml-1 text-xs text-muted-foreground">唯讀</span>}
            </li>
          ))}
        </ul>
      </details>
    </SectionCard>
  )
}

type TokensQuery = ReturnType<typeof useQuery<TokenInfo[]>>

function isActive(t: TokenInfo) {
  return !t.revoked_at && !(t.expires_at && new Date(t.expires_at) < new Date())
}

function ConnectionsCard({ tokens, onRevoke }: { tokens: TokensQuery; onRevoke: (t: TokenInfo) => void }) {
  const [showAll, setShowAll] = useState(false)
  const all = tokens.data?.filter((t) => t.kind === 'oauth') ?? []
  const shown = showAll ? all : all.filter(isActive)
  return (
    <SectionCard
      title="已連線的應用"
      description="透過 MCP 授權的 Agent。重新授權同一個應用會取代舊的連線。"
      actions={
        all.length > shown.length || showAll ? (
          <Button size="sm" variant="ghost" onClick={() => setShowAll(!showAll)}>
            {showAll ? '只顯示有效連線' : `顯示已中斷的（${all.length - shown.length}）`}
          </Button>
        ) : undefined
      }
    >
      {tokens.isLoading ? (
        <Loading />
      ) : tokens.error ? (
        <ErrorState error={tokens.error} />
      ) : shown.length === 0 ? (
        <Empty>還沒有應用透過 MCP 連線。</Empty>
      ) : (
        <ul className="divide-y rounded-lg border" data-testid="mcp-connections">
          {shown.map((t) => (
            <li key={t.id} className="flex flex-wrap items-center gap-3 px-4 py-3">
              <Bot aria-hidden className="size-4 text-muted-foreground" />
              <span className="font-medium">{t.name}</span>
              {t.client && <span className="font-mono text-xs text-muted-foreground">{t.client.redirect_hosts.join(', ')}</span>}
              <span className="font-mono text-xs text-muted-foreground">{t.scopes.join(' ')}</span>
              <span className="text-sm text-muted-foreground">
                授權 {formatTime(t.created_at)}，最後使用 {formatTime(t.last_used_at)}
              </span>
              <span className="ml-auto">
                {isActive(t) ? (
                  <Button size="sm" variant="ghost-destructive" onClick={() => onRevoke(t)}>
                    中斷連線
                  </Button>
                ) : (
                  <StatusPill tone="neutral" label="已中斷" />
                )}
              </span>
            </li>
          ))}
        </ul>
      )}
    </SectionCard>
  )
}

function ApiTokensCard({ tokens, onRevoke }: { tokens: TokensQuery; onRevoke: (t: TokenInfo) => void }) {
  const qc = useQueryClient()
  const [name, setName] = useState('')
  const [scopes, setScopes] = useState<string[]>(AUTHORING_SCOPES)
  const [days, setDays] = useState('90')
  const [created, setCreated] = useState<CreatedToken | null>(null)
  const [errors, setErrors] = useState<Record<string, string>>({})
  const create = useMutation({
    mutationFn: () => api.post<CreatedToken>('/admin/tokens', { name, scopes, expires_in_days: days ? Number(days) : null }),
    onSuccess: (t) => {
      setCreated(t)
      setName('')
      setErrors({})
      qc.invalidateQueries({ queryKey: ['tokens'] })
    },
    onError: (e) => {
      setErrors(e instanceof ApiError ? errorsByPointer(e.errors) : {})
      toast.error(errorMessage(e))
    },
  })
  const list = tokens.data?.filter((t) => t.kind === 'api') ?? []
  return (
    <SectionCard title="API token" description="給腳本或本機 Agent 使用。只勾選需要的權限；token 只會顯示一次，伺服器不保存明文。用法：Authorization: Bearer <token>。">
      {created && (
        <div role="status" className="space-y-2 rounded-lg border border-warning/30 bg-warning-soft p-4">
          <p className="font-medium">「{created.info.name}」已建立。這是唯一一次顯示完整 token：</p>
          <code className="block overflow-x-auto rounded bg-card p-2 font-mono text-sm" data-testid="new-token">
            {created.token}
          </code>
          <Button size="sm" variant="outline" onClick={() => copy(created.token, '')}>
            <Copy aria-hidden />
            複製
          </Button>
        </div>
      )}
      <div className="grid gap-4 md:grid-cols-2">
        <Field label="名稱（用途）" htmlFor="tn" error={errors['/name']}>
          <Input id="tn" value={name} onChange={(e) => setName(e.target.value)} placeholder="例如：出題 Agent" />
        </Field>
        <Field label="有效天數（1–365，留空表示不過期）" htmlFor="td" error={errors['/expires_in_days']}>
          <Input id="td" type="number" value={days} onChange={(e) => setDays(e.target.value)} />
        </Field>
      </div>
      <Field label="權限" error={errors['/scopes']}>
        <div className="grid gap-1.5 sm:grid-cols-2">
          {SCOPES.map((s) => (
            <div key={s.id} className="flex items-center gap-2">
              <Checkbox id={`sc-${s.id}`} checked={scopes.includes(s.id)} onCheckedChange={(on) => setScopes(on ? [...scopes, s.id] : scopes.filter((x) => x !== s.id))} />
              <Label htmlFor={`sc-${s.id}`} variant="option">
                {s.label} <code className="font-mono text-xs text-muted-foreground">{s.id}</code>
              </Label>
            </div>
          ))}
        </div>
      </Field>
      <Button onClick={() => create.mutate()} disabled={!name || scopes.length === 0 || create.isPending}>
        <Plus aria-hidden />
        建立 token
      </Button>
      {tokens.isLoading ? (
        <Loading />
      ) : tokens.error ? (
        <ErrorState error={tokens.error} />
      ) : list.length === 0 ? (
        <Empty>還沒有建立任何 token。</Empty>
      ) : (
        <ul className="divide-y rounded-lg border">
          {list.map((t) => {
            const expired = t.expires_at && new Date(t.expires_at) < new Date()
            return (
              <li key={t.id} className="flex flex-wrap items-center gap-3 px-4 py-3">
                <KeyRound aria-hidden className="size-4 text-muted-foreground" />
                <span className="font-medium">{t.name}</span>
                <span className="font-mono text-xs text-muted-foreground">{t.scopes.join(' ')}</span>
                <span className="text-sm text-muted-foreground">
                  建立 {formatTime(t.created_at)}，最後使用 {formatTime(t.last_used_at)}
                  {t.expires_at && `，到期 ${formatTime(t.expires_at)}`}
                </span>
                <span className="ml-auto">
                  {t.revoked_at ? (
                    <StatusPill tone="neutral" label="已撤銷" />
                  ) : expired ? (
                    <StatusPill tone="neutral" label="已過期" />
                  ) : (
                    <Button size="sm" variant="ghost-destructive" onClick={() => onRevoke(t)}>
                      撤銷
                    </Button>
                  )}
                </span>
              </li>
            )
          })}
        </ul>
      )}
    </SectionCard>
  )
}
