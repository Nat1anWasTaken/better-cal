import { useQuery, useQueryClient } from '@tanstack/react-query'
import { createContext, useCallback, useContext, useMemo, type ReactNode } from 'react'

import { ApiError, api, setCsrfToken } from '@/api/client'
import type { Me } from '@/api/types'

interface AuthValue {
  me: Me | null
  loading: boolean
  isOwner: boolean
  login: (username: string, password: string) => Promise<Me>
  logout: () => Promise<void>
  refresh: () => Promise<void>
}

const AuthContext = createContext<AuthValue | null>(null)

async function fetchMe(): Promise<Me | null> {
  try {
    const me = await api.get<Me>('/auth/me')
    setCsrfToken(me.csrf_token)
    return me
  } catch (e) {
    if (e instanceof ApiError && e.status === 401) {
      setCsrfToken(null)
      return null
    }
    throw e
  }
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const qc = useQueryClient()
  const q = useQuery({ queryKey: ['me'], queryFn: fetchMe, staleTime: 60_000, retry: 1 })

  const login = useCallback(
    async (username: string, password: string) => {
      const me = await api.post<Me>('/auth/login', { username, password })
      setCsrfToken(me.csrf_token)
      // Drop the previous user's cached data but keep the observed `me` query alive.
      qc.removeQueries({ predicate: (q) => q.queryKey[0] !== 'me' })
      qc.setQueryData(['me'], me)
      return me
    },
    [qc],
  )
  const logout = useCallback(async () => {
    try {
      await api.post('/auth/logout')
    } finally {
      setCsrfToken(null)
      qc.removeQueries({ predicate: (q) => q.queryKey[0] !== 'me' })
      qc.setQueryData(['me'], null)
    }
  }, [qc])
  const refresh = useCallback(async () => {
    await qc.invalidateQueries({ queryKey: ['me'] })
  }, [qc])

  const value = useMemo<AuthValue>(
    () => ({ me: q.data ?? null, loading: q.isLoading, isOwner: q.data?.user.role === 'owner', login, logout, refresh }),
    [q.data, q.isLoading, login, logout, refresh],
  )
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth(): AuthValue {
  const v = useContext(AuthContext)
  if (!v) throw new Error('useAuth outside AuthProvider')
  return v
}
