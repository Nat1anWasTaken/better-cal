import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useEffect, useRef, useState } from 'react'

import { api } from '@/api/client'
import type { SubmissionView } from '@/api/types'

const GIVE_UP_MS = 90_000

/** Polls a submission until it finishes (with gentle backoff) and gives up after 90s. */
export function useSubmission(id: string | null) {
  const qc = useQueryClient()
  const started = useRef<number>(0)
  const [timedOut, setTimedOut] = useState(false)
  useEffect(() => {
    started.current = Date.now()
    setTimedOut(false)
  }, [id])

  const q = useQuery({
    queryKey: ['submission', id],
    queryFn: () => api.get<SubmissionView>(`/submissions/${id}`),
    enabled: !!id,
    refetchInterval: (query) => {
      const s = query.state.data
      if (!s || s.status === 'done' || s.status === 'system_error') return false
      const elapsed = Date.now() - started.current
      if (elapsed > GIVE_UP_MS) {
        setTimedOut(true)
        return false
      }
      return elapsed < 10_000 ? 700 : 2000
    },
  })

  const revealHint = useCallback(
    async (findingId: number) => {
      if (!id) return
      await api.post(`/submissions/${id}/findings/${findingId}/hints`)
      await qc.invalidateQueries({ queryKey: ['submission', id] })
    },
    [id, qc],
  )

  const rejudge = useCallback(async () => {
    if (!id) return
    const s = await api.post<SubmissionView>(`/submissions/${id}/rejudge`)
    started.current = Date.now()
    setTimedOut(false)
    qc.setQueryData(['submission', id], s)
  }, [id, qc])

  return { submission: q.data ?? null, error: q.error, timedOut, revealHint, rejudge }
}

export function useMediaQuery(query: string): boolean {
  const [match, setMatch] = useState(() => window.matchMedia(query).matches)
  useEffect(() => {
    const mq = window.matchMedia(query)
    const on = () => setMatch(mq.matches)
    mq.addEventListener('change', on)
    return () => mq.removeEventListener('change', on)
  }, [query])
  return match
}
