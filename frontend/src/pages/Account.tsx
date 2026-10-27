import { useState } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'

import { api } from '@/api/client'
import { FieldMessage, Page, PageHeader } from '@/components/Page'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useAuth } from '@/hooks/auth'
import { errorMessage, fieldErrors } from '@/lib/errors'

export function AccountPage({ forced = false }: { forced?: boolean }) {
  const { me, refresh, logout } = useAuth()
  const nav = useNavigate()
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const [repeat, setRepeat] = useState('')
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [busy, setBusy] = useState(false)

  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (next !== repeat) {
      setErrors({ '/repeat': '兩次輸入的新密碼不一樣' })
      return
    }
    setBusy(true)
    setErrors({})
    try {
      await api.post('/auth/password', { current_password: current, new_password: next })
      toast.success('密碼已更新，其他裝置上的登入已登出。')
      setCurrent('')
      setNext('')
      setRepeat('')
      await refresh()
      if (forced) nav('/', { replace: true })
    } catch (err) {
      const fe = fieldErrors(err)
      setErrors(Object.keys(fe).length ? fe : { '/': errorMessage(err) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <Page>
      <PageHeader
        title={forced ? '請先設定你的密碼' : '帳號與密碼'}
        description={forced ? '這是管理者為你產生的臨時密碼。設定一個只有你知道的新密碼後就可以開始練習。' : `目前登入：${me?.user.username ?? ''}`}
      />
      <form onSubmit={submit} className="max-w-sm space-y-4" noValidate>
        <div className="space-y-1.5">
          <Label htmlFor="cur">目前的密碼</Label>
          <Input id="cur" type="password" autoComplete="current-password" value={current} onChange={(e) => setCurrent(e.target.value)} />
          <FieldMessage message={errors['/current_password']} />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="new">新密碼（至少 8 個字元）</Label>
          <Input id="new" type="password" autoComplete="new-password" value={next} onChange={(e) => setNext(e.target.value)} />
          <FieldMessage message={errors['/new_password']} />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="rep">再輸入一次新密碼</Label>
          <Input id="rep" type="password" autoComplete="new-password" value={repeat} onChange={(e) => setRepeat(e.target.value)} />
          <FieldMessage message={errors['/repeat']} />
        </div>
        <FieldMessage message={errors['/']} />
        <div className="flex gap-2">
          <Button type="submit" disabled={busy || !current || !next}>
            {busy ? '更新中…' : '更新密碼'}
          </Button>
          {forced && (
            <Button type="button" variant="ghost" onClick={async () => (await logout(), nav('/login'))}>
              登出
            </Button>
          )}
        </div>
      </form>
    </Page>
  )
}
