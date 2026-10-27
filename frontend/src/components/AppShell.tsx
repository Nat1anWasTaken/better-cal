import { LayoutDashboard, LogOut, Monitor, Moon, Sun, UserRound } from 'lucide-react'
import { NavLink, Outlet, useNavigate } from 'react-router'

import { SystemBanner } from '@/components/Banners'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useAuth } from '@/hooks/auth'
import { useSite } from '@/hooks/useSite'
import { useTheme } from '@/hooks/useTheme'
import { cn } from '@/lib/utils'

const navClass = ({ isActive }: { isActive: boolean }) =>
  cn('rounded-md px-2.5 py-1.5 text-sm hover:bg-accent', isActive ? 'font-medium text-foreground' : 'text-muted-foreground')

export function UserMenu() {
  const { me, isOwner, logout } = useAuth()
  const [theme, setTheme] = useTheme()
  const nav = useNavigate()
  if (!me) return null
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" aria-label="帳號選單">
          <UserRound aria-hidden />
          <span className="max-w-32 truncate">{me.user.display_name || me.user.username}</span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-52">
        <DropdownMenuLabel variant="body">{me.user.username}</DropdownMenuLabel>
        <DropdownMenuItem onSelect={() => nav('/account')}>
          <UserRound aria-hidden />
          帳號與密碼
        </DropdownMenuItem>
        {isOwner && (
          <DropdownMenuItem onSelect={() => nav('/admin')}>
            <LayoutDashboard aria-hidden />
            管理後台
          </DropdownMenuItem>
        )}
        <DropdownMenuSeparator />
        <DropdownMenuLabel variant="caption">外觀</DropdownMenuLabel>
        <DropdownMenuRadioGroup value={theme} onValueChange={(v) => setTheme(v as typeof theme)}>
          <DropdownMenuRadioItem value="light">
            <Sun aria-hidden /> 淺色
          </DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="dark">
            <Moon aria-hidden /> 深色
          </DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="system">
            <Monitor aria-hidden /> 跟隨系統
          </DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          onSelect={async () => {
            await logout()
            nav('/login')
          }}
        >
          <LogOut aria-hidden />
          登出
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

export function Brand() {
  const site = useSite()
  return (
    <NavLink to="/" className="flex min-w-0 items-center gap-2 font-medium">
      <span aria-hidden className="grid size-7 shrink-0 place-items-center rounded-md bg-primary font-mono text-sm text-primary-foreground">
        C
      </span>
      <span className="hidden truncate sm:inline">{site.data?.site_name ?? 'CYCU-CAL'}</span>
    </NavLink>
  )
}

/** Header + outlet for student pages. `fullHeight` pages (the workspace) manage their own scrolling. */
export function AppShell() {
  return (
    <div className="flex min-h-dvh flex-col">
      <a href="#main" className="sr-only focus:not-sr-only focus:absolute focus:top-2 focus:left-2 focus:z-50 focus:rounded-md focus:bg-card focus:px-3 focus:py-2">
        跳到主要內容
      </a>
      <header className="sticky top-0 z-30 border-b bg-background/95 backdrop-blur">
        <div className="flex h-12 items-center gap-3 px-4">
          <Brand />
          <nav aria-label="主要導覽" className="flex items-center gap-0.5">
            <NavLink to="/" end className={navClass}>
              題目集
            </NavLink>
            <NavLink to="/me/submissions" className={navClass}>
              我的提交
            </NavLink>
            <NavLink to="/me/progress" className={navClass}>
              進度
            </NavLink>
          </nav>
          <div className="ml-auto">
            <UserMenu />
          </div>
        </div>
      </header>
      <SystemBanner />
      <main id="main" className="flex min-h-0 flex-1 flex-col">
        <Outlet />
      </main>
    </div>
  )
}
