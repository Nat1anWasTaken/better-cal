import { useQuery } from '@tanstack/react-query'
import { ServerCrash } from 'lucide-react'

import { api } from '@/api/client'
import type { SystemStatus } from '@/api/types'

export function SystemBanner() {
  const q = useQuery({
    queryKey: ['system-status'],
    queryFn: () => api.get<SystemStatus>('/system/status'),
    refetchInterval: 30_000,
  })
  if (q.data?.judge !== 'down') return null
  return (
    <div role="status" className="flex items-start gap-2 border-b border-warning/30 bg-warning-soft px-4 py-2 text-sm text-warning">
      <ServerCrash aria-hidden className="mt-0.5 size-4 shrink-0" />
      <p>{q.data.message ?? '評測服務暫時無法連線。你的程式碼與提交都已保存，服務恢復後會自動繼續評測。'}</p>
    </div>
  )
}
