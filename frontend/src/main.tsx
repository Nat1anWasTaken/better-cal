import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { RouterProvider, createBrowserRouter } from 'react-router'

import { App } from '@/App'
import { ApiError } from '@/api/client'
import { Toaster } from '@/components/ui/sonner'
import { AuthProvider } from '@/hooks/auth'

import './index.css'

// Apply the saved theme before first paint (no inline scripts: CSP is 'self' only).
try {
  const t = localStorage.getItem('ccal-theme') ?? 'system'
  const dark = t === 'dark' || (t === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)
  document.documentElement.classList.toggle('dark', dark)
} catch {
  // storage unavailable
}

// A data router (so useBlocker works for unsaved-changes prompts) wrapping the declarative routes.
const router = createBrowserRouter([
  {
    path: '*',
    element: (
      <>
        <App />
        <Toaster richColors closeButton position="bottom-right" />
      </>
    ),
  },
])

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      retry: (count, e) => !(e instanceof ApiError && e.status >= 400 && e.status < 500) && count < 2,
    },
  },
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <RouterProvider router={router} />
      </AuthProvider>
    </QueryClientProvider>
  </StrictMode>,
)
