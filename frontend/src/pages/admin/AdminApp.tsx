import {
  BookMarked,
  ChartColumn,
  ClipboardList,
  FileJson,
  Gauge,
  KeyRound,
  Library,
  Menu,
  MessageSquareText,
  ScrollText,
  Settings,
  Table2,
  Trophy,
  Users,
  UsersRound,
} from 'lucide-react'
import { useState } from 'react'
import { Link, NavLink, Outlet, Route, Routes } from 'react-router'

import { Brand, UserMenu } from '@/components/AppShell'
import { SystemBanner } from '@/components/Banners'
import { Button } from '@/components/ui/button'
import { Sheet, SheetContent, SheetTitle, SheetTrigger } from '@/components/ui/sheet'
import { cn } from '@/lib/utils'
import { SubmissionDetailPage } from '@/pages/MySubmissions'

import { AuditPage } from './Audit'
import { CatalogPage } from './Catalog'
import { CatalogTextsPage } from './CatalogTexts'
import { CohortsPage } from './Cohorts'
import { CustomRulePage } from './CustomRule'
import { ImportExportPage } from './ImportExport'
import { OverviewPage } from './Overview'
import { ProblemEditorPage } from './ProblemEditor'
import { ProblemsPage } from './Problems'
import { ProgressMatrixPage } from './ProgressMatrix'
import { RoundsPage } from './Rounds'
import { RuleEditorPage } from './RuleEditor'
import { RuleStatsPage } from './RuleStats'
import { SetEditorPage } from './SetEditor'
import { SetsPage } from './Sets'
import { SettingsPage } from './Settings'
import { StudentDetailPage } from './StudentDetail'
import { StudentsPage } from './Students'
import { AdminSubmissionsPage } from './Submissions'
import { TokensPage } from './Tokens'

const NAV = [
  { group: '', items: [{ to: '/admin', label: '總覽', icon: Gauge, end: true }] },
  {
    group: '題目',
    items: [
      { to: '/admin/problems', label: '題目庫', icon: Library },
      { to: '/admin/sets', label: '題目集', icon: BookMarked },
      { to: '/admin/import', label: '匯入／匯出', icon: FileJson },
    ],
  },
  {
    group: '教學',
    items: [
      { to: '/admin/catalog', label: '教學文案', icon: MessageSquareText },
      { to: '/admin/stats', label: '常見錯誤統計', icon: ChartColumn },
    ],
  },
  {
    group: '學生',
    items: [
      { to: '/admin/users', label: '學生帳號', icon: Users },
      { to: '/admin/cohorts', label: '組別', icon: UsersRound },
      { to: '/admin/progress', label: '進度表', icon: Table2 },
      { to: '/admin/rounds', label: '排行輪次', icon: Trophy },
      { to: '/admin/submissions', label: '提交紀錄', icon: ClipboardList },
    ],
  },
  {
    group: '系統',
    items: [
      { to: '/admin/settings', label: '全站設定', icon: Settings },
      { to: '/admin/tokens', label: 'API token', icon: KeyRound },
      { to: '/admin/audit', label: '操作紀錄', icon: ScrollText },
    ],
  },
]

function SideNav({ onNavigate }: { onNavigate?: () => void }) {
  return (
    <nav aria-label="管理後台" className="space-y-5 p-3">
      {NAV.map((g) => (
        <div key={g.group || 'main'} className="space-y-0.5">
          {g.group && <div className="px-2 pb-1 text-xs text-muted-foreground">{g.group}</div>}
          {g.items.map((it) => (
            <NavLink
              key={it.to}
              to={it.to}
              end={'end' in it ? it.end : false}
              onClick={onNavigate}
              className={({ isActive }) =>
                cn(
                  'flex items-center gap-2 rounded-md px-2 py-1.5 text-sm',
                  isActive ? 'bg-sidebar-accent font-medium text-sidebar-accent-foreground' : 'text-sidebar-foreground/80 hover:bg-sidebar-accent/60',
                )
              }
            >
              <it.icon aria-hidden className="size-4" />
              {it.label}
            </NavLink>
          ))}
        </div>
      ))}
    </nav>
  )
}

function AdminLayout() {
  const [open, setOpen] = useState(false)
  return (
    <div className="flex min-h-dvh">
      <aside className="sticky top-0 hidden h-dvh w-56 shrink-0 overflow-y-auto border-r bg-sidebar lg:block">
        <div className="flex h-12 items-center border-b px-4">
          <Brand />
        </div>
        <SideNav />
      </aside>
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-30 flex h-12 items-center gap-2 border-b bg-background/95 px-4 backdrop-blur">
          <Sheet open={open} onOpenChange={setOpen}>
            <SheetTrigger asChild>
              <Button variant="ghost" size="icon" className="lg:hidden" aria-label="開啟選單">
                <Menu aria-hidden />
              </Button>
            </SheetTrigger>
            <SheetContent side="left" variant="sidebar" className="w-64">
              <SheetTitle className="sr-only">管理後台選單</SheetTitle>
              <SideNav onNavigate={() => setOpen(false)} />
            </SheetContent>
          </Sheet>
          <span className="font-medium">管理後台</span>
          <Link to="/" className="ml-4 text-sm text-muted-foreground hover:text-foreground">
            回到學生畫面
          </Link>
          <div className="ml-auto">
            <UserMenu />
          </div>
        </header>
        <SystemBanner />
        <main id="main" className="min-w-0 flex-1">
          <Outlet />
        </main>
      </div>
    </div>
  )
}

export default function AdminApp() {
  return (
    <Routes>
      <Route element={<AdminLayout />}>
        <Route index element={<OverviewPage />} />
        <Route path="problems" element={<ProblemsPage />} />
        <Route path="problems/new" element={<ProblemEditorPage />} />
        <Route path="problems/:slug" element={<ProblemEditorPage />} />
        <Route path="sets" element={<SetsPage />} />
        <Route path="sets/new" element={<SetEditorPage />} />
        <Route path="sets/:slug" element={<SetEditorPage />} />
        <Route path="import" element={<ImportExportPage />} />
        <Route path="catalog" element={<CatalogPage />} />
        <Route path="catalog/texts" element={<CatalogTextsPage />} />
        <Route path="catalog/rules/:id" element={<RuleEditorPage />} />
        <Route path="catalog/custom/:id" element={<CustomRulePage />} />
        <Route path="stats" element={<RuleStatsPage />} />
        <Route path="users" element={<StudentsPage />} />
        <Route path="users/:id" element={<StudentDetailPage />} />
        <Route path="cohorts" element={<CohortsPage />} />
        <Route path="progress" element={<ProgressMatrixPage />} />
        <Route path="rounds" element={<RoundsPage />} />
        <Route path="submissions" element={<AdminSubmissionsPage />} />
        <Route path="submissions/:id" element={<SubmissionDetailPage admin />} />
        <Route path="settings" element={<SettingsPage />} />
        <Route path="tokens" element={<TokensPage />} />
        <Route path="audit" element={<AuditPage />} />
      </Route>
    </Routes>
  )
}
