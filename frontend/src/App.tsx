import { lazy, Suspense } from 'react'
import { Navigate, Outlet, Route, Routes, useLocation } from 'react-router'

import { AppShell } from '@/components/AppShell'
import { Loading } from '@/components/Page'
import { useAuth } from '@/hooks/auth'
import { AccountPage } from '@/pages/Account'
import { HomePage } from '@/pages/Home'
import { LoginPage } from '@/pages/Login'
import { MyProgressPage } from '@/pages/MyProgress'
import { OAuthConsentPage } from '@/pages/OAuthConsent'
import { SetPage } from '@/pages/SetPage'

const AdminApp = lazy(() => import('@/pages/admin/AdminApp'))
// The editor (Monaco) is only needed on these pages.
const ProblemPage = lazy(() => import('@/pages/ProblemPage').then((m) => ({ default: m.ProblemPage })))
const MySubmissionsPage = lazy(() => import('@/pages/MySubmissions').then((m) => ({ default: m.MySubmissionsPage })))
const SubmissionDetailPage = lazy(() => import('@/pages/MySubmissions').then((m) => ({ default: m.SubmissionDetailPage })))
const PreferencesPage = lazy(() => import('@/pages/Preferences').then((m) => ({ default: m.PreferencesPage })))
const LeaderboardPage = lazy(() => import('@/pages/Leaderboard').then((m) => ({ default: m.LeaderboardPage })))

const lazyPage = (el: React.ReactNode) => <Suspense fallback={<Loading />}>{el}</Suspense>

function RequireAuth() {
  const { me, loading } = useAuth()
  const loc = useLocation()
  if (loading) return <Loading />
  if (!me) return <Navigate to="/login" replace state={{ from: loc.pathname + loc.search }} />
  if (me.user.must_change_password && loc.pathname !== '/account') return <Navigate to="/account" replace />
  return <Outlet />
}

function AccountRoute() {
  const { me } = useAuth()
  return <AccountPage forced={!!me?.user.must_change_password} />
}

function RequireOwner() {
  const { isOwner } = useAuth()
  if (!isOwner) return <Navigate to="/" replace />
  return (
    <Suspense fallback={<Loading />}>
      <AdminApp />
    </Suspense>
  )
}

export function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route element={<RequireAuth />}>
        <Route path="/admin/*" element={<RequireOwner />} />
        <Route path="/oauth/consent" element={<OAuthConsentPage />} />
        <Route element={<AppShell />}>
          <Route index element={<HomePage />} />
          <Route path="/account" element={<AccountRoute />} />
          <Route path="/settings" element={lazyPage(<PreferencesPage />)} />
          <Route path="/sets/:setId" element={<SetPage />} />
          <Route path="/sets/:setId/problems/:slug" element={lazyPage(<ProblemPage />)} />
          <Route path="/problems/:slug" element={lazyPage(<ProblemPage />)} />
          <Route path="/me/submissions" element={lazyPage(<MySubmissionsPage />)} />
          <Route path="/me/submissions/:id" element={lazyPage(<SubmissionDetailPage />)} />
          <Route path="/me/progress" element={<MyProgressPage />} />
          <Route path="/leaderboard" element={lazyPage(<LeaderboardPage />)} />
          <Route path="/rounds/:roundId" element={lazyPage(<LeaderboardPage />)} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Route>
    </Routes>
  )
}
