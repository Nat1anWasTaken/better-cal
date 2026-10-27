import { useState } from 'react'
import { Navigate, useLocation, useNavigate } from 'react-router'

import { FieldMessage } from '@/components/Page'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useAuth } from '@/hooks/auth'
import { useSite } from '@/hooks/useSite'
import { errorMessage } from '@/lib/errors'

export function LoginPage() {
  const { me, login } = useAuth()
  const site = useSite()
  const nav = useNavigate()
  const loc = useLocation()
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string>()
  const [busy, setBusy] = useState(false)
  const from = (loc.state as { from?: string } | null)?.from ?? '/'
  if (me) return <Navigate to={from} replace />

  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    setBusy(true)
    setError(undefined)
    try {
      await login(username, password)
      nav(from, { replace: true })
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <main className="grid min-h-dvh place-items-center px-4">
      <div className="w-full max-w-sm">
        <div className="mb-8 space-y-3">
          <span aria-hidden className="grid size-10 place-items-center rounded-lg bg-primary font-mono text-lg text-primary-foreground">
            C
          </span>
          <h1 className="text-2xl font-medium">{site.data?.site_name ?? 'CYCU-CAL 程式讀書會'}</h1>
          <p className="text-muted-foreground">登入後就可以開始寫 C 語言練習題。帳號由讀書會的管理者發給你。</p>
        </div>
        <form onSubmit={submit} className="space-y-4" noValidate>
          <div className="space-y-1.5">
            <Label htmlFor="username">帳號</Label>
            <Input id="username" autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} required />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="password">密碼</Label>
            <Input id="password" type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} required aria-describedby={error ? 'login-error' : undefined} />
          </div>
          <FieldMessage id="login-error" message={error} />
          <Button type="submit" className="w-full" disabled={busy || !username || !password}>
            {busy ? '登入中…' : '登入'}
          </Button>
        </form>
      </div>
    </main>
  )
}
