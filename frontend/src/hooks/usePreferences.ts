import { useQueryClient } from '@tanstack/react-query'
import { useCallback } from 'react'

import { api } from '@/api/client'
import type { Me, UserPreferences } from '@/api/types'
import { useAuth } from '@/hooks/auth'
import { DEFAULT_PREFERENCES } from '@/lib/preferences'

/** The signed-in user's preferences, and a saver that updates every mounted editor. */
export function usePreferences(): { preferences: UserPreferences; save: (p: UserPreferences) => Promise<UserPreferences> } {
  const { me } = useAuth()
  const qc = useQueryClient()
  const save = useCallback(
    async (p: UserPreferences) => {
      const saved = await api.put<UserPreferences>('/auth/preferences', p)
      qc.setQueryData<Me | null>(['me'], (m) => (m ? { ...m, user: { ...m.user, preferences: saved } } : m))
      return saved
    },
    [qc],
  )
  return { preferences: me?.user.preferences ?? DEFAULT_PREFERENCES, save }
}
