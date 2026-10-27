import { useQuery } from '@tanstack/react-query'

import { api } from '@/api/client'
import type { SiteInfo } from '@/api/types'

/** Site info, polled every 30s so active-set switches are noticed without a reload. */
export function useSite() {
  return useQuery({ queryKey: ['site'], queryFn: () => api.get<SiteInfo>('/site'), refetchInterval: 30_000 })
}
