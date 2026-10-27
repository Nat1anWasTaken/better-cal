import { Check, CircleDot, PartyPopper, RotateCcw, ServerCrash } from 'lucide-react'
import type { ReactNode } from 'react'

import type { Span, SubmissionView } from '@/api/types'
import { CardList } from '@/components/CardList'
import { StatusPill } from '@/components/StatusPill'
import { TestResults } from '@/components/TestResults'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { PHASES, PHASE_LABELS, verdictInfo } from '@/lib/format'
import { cn } from '@/lib/utils'

export function PhaseSteps({ status }: { status: string }) {
  const current = PHASES.indexOf(status as (typeof PHASES)[number])
  return (
    <ol className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm" aria-label="評測進度">
      {PHASES.map((p, i) => {
        const done = current > i || status === 'done'
        const active = current === i && status !== 'done'
        return (
          <li key={p} className="flex items-center gap-1.5" aria-current={active ? 'step' : undefined}>
            {i > 0 && <span aria-hidden className="h-px w-4 bg-border" />}
            {done ? (
              <Check aria-hidden className="size-4 text-success" />
            ) : (
              <CircleDot aria-hidden className={cn('size-4', active ? 'animate-pulse text-primary' : 'text-muted-foreground/50')} />
            )}
            <span className={cn(active ? 'font-medium text-primary' : done ? '' : 'text-muted-foreground')}>
              {PHASE_LABELS[p]}
              {active && <span className="sr-only">（進行中）</span>}
            </span>
          </li>
        )
      })}
    </ol>
  )
}

interface Props {
  submission: SubmissionView
  onJump?: (span: Span) => void
  onRevealHint?: (findingId: number) => Promise<void>
  onRejudge?: () => void
  /** Polling gave up: the service did not answer in time. */
  timedOut?: boolean
  /** Shown with an accepted submit: where to go next. */
  completion?: { message?: string; action?: ReactNode }
}

export function SubmissionResult({ submission: s, onJump, onRevealHint, onRejudge, timedOut, completion }: Props) {
  const finished = s.status === 'done' || s.status === 'system_error'
  const v = verdictInfo(finished ? s.verdict : null)
  const kindLabel = s.kind === 'submit' ? '正式提交' : s.kind === 'test_run' ? '測試執行' : '執行'
  return (
    <div className="space-y-4" data-testid="submission-result" data-status={s.status} data-verdict={s.verdict ?? ''}>
      <div className="space-y-2" aria-live="polite">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-sm text-muted-foreground">
            {kindLabel} · 題目版本 {s.revision_number}
          </span>
          {finished && <StatusPill tone={v.tone} label={v.label} size="lg" />}
          {finished && s.total != null && s.total > 0 && s.verdict !== 'compile_error' && (
            <span className="text-sm">
              通過 {s.passed ?? 0} / {s.total} 筆測資
            </span>
          )}
        </div>
        {!finished && <PhaseSteps status={s.status} />}
        {!finished && <p className="text-sm text-muted-foreground">{s.status_text}…</p>}
      </div>

      {(s.status === 'system_error' || timedOut) && (
        <Alert>
          <ServerCrash aria-hidden />
          <AlertTitle>{timedOut ? '評測服務沒有回應' : '評測服務暫時發生問題'}</AlertTitle>
          <AlertDescription>
            <p>這不是你的程式造成的問題，你的程式碼已經保存。{timedOut ? '請稍後重新整理，或再送出一次。' : '可以稍等一下再重新評測。'}</p>
            {s.retryable && onRejudge && (
              <Button size="sm" variant="outline" className="mt-2" onClick={onRejudge}>
                <RotateCcw aria-hidden />
                重新評測
              </Button>
            )}
          </AlertDescription>
        </Alert>
      )}

      {finished && s.verdict === 'accepted' && (
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2 rounded-lg border border-success/30 bg-success-soft px-4 py-3 text-success" data-testid="accepted-banner">
          <p className="flex flex-1 items-center gap-2">
            {s.kind === 'submit' && <PartyPopper aria-hidden className="size-4 shrink-0" />}
            {s.kind === 'submit' ? (completion?.message ?? '全部測資都通過了，做得好！') : '公開測資都通過了。準備好就按「提交」用全部測資評測吧。'}
          </p>
          {s.kind === 'submit' && completion?.action}
        </div>
      )}

      {s.cards.length > 0 && <CardList cards={s.cards} onJump={onJump} onRevealHint={onRevealHint} rawExpanded={s.raw_expanded} />}
      {s.tests.length > 0 && (
        <section aria-label="測資結果" className="space-y-2">
          <h3 className="font-medium">測資結果</h3>
          <TestResults tests={s.tests} compareDescription={s.compare_description} />
        </section>
      )}
    </div>
  )
}
