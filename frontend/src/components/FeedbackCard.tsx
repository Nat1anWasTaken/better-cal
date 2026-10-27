import { ChevronDown, CircleAlert, CircleHelp, Info, Lightbulb, LocateFixed, Lock, ShieldCheck, TriangleAlert } from 'lucide-react'
import { useState } from 'react'

import type { FeedbackCard as Card, Span } from '@/api/types'
import { InlineMarkdown, Markdown } from '@/components/Markdown'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { confidenceTone } from '@/lib/cards'
import { cn } from '@/lib/utils'

const SEVERITY = {
  error: { label: '錯誤', icon: CircleAlert, spine: 'border-l-destructive', text: 'text-destructive' },
  warning: { label: '建議檢查', icon: TriangleAlert, spine: 'border-l-warning', text: 'text-warning' },
  info: { label: '小提醒', icon: Info, spine: 'border-l-primary', text: 'text-primary' },
} as const

function severityOf(c: Card) {
  return SEVERITY[(c.severity as keyof typeof SEVERITY) in SEVERITY ? (c.severity as keyof typeof SEVERITY) : 'info']
}

function ConfidenceBadge({ card }: { card: Card }) {
  const tone = confidenceTone(card)
  if (tone === 'confirmed') {
    return (
      <span className="inline-flex items-center gap-1 rounded-md border border-foreground/25 bg-card px-1.5 py-0.5 text-xs font-medium">
        <ShieldCheck aria-hidden className="size-3.5" />
        {card.confidence_label}
      </span>
    )
  }
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 rounded-md border border-dashed px-1.5 py-0.5 text-xs font-medium',
        tone === 'likely' ? 'border-primary/50 text-primary' : 'border-muted-foreground/60 text-muted-foreground',
      )}
    >
      <CircleHelp aria-hidden className="size-3.5" />
      {tone === 'likely' ? '系統判斷：' : ''}
      {card.confidence_label}
    </span>
  )
}

function Section({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid gap-1 @xl:grid-cols-label @xl:gap-3">
      <dt className="text-sm font-medium text-muted-foreground">{label}</dt>
      <dd className="text-base leading-relaxed">{children}</dd>
    </div>
  )
}

function spanText(s: Span) {
  return s.column > 1 ? `第 ${s.line} 行第 ${s.column} 個字` : `第 ${s.line} 行`
}

interface Props {
  card: Card
  onJump?: (span: Span) => void
  /** Reveals the next hint level (student) — omitted in previews, where all hints are shown. */
  onRevealHint?: (findingId: number) => Promise<void>
  defaultRawOpen?: boolean
  compact?: boolean
}

export function FeedbackCard({ card, onJump, onRevealHint, defaultRawOpen = false, compact = false }: Props) {
  const sev = severityOf(card)
  const Icon = sev.icon
  const [revealing, setRevealing] = useState(false)
  const canReveal = !!onRevealHint && card.hints.length < card.hints_allowed
  const reveal = async () => {
    if (!onRevealHint) return
    setRevealing(true)
    try {
      await onRevealHint(card.finding_id)
    } finally {
      setRevealing(false)
    }
  }
  const guess = confidenceTone(card) !== 'confirmed'

  return (
    <article
      aria-labelledby={`card-${card.finding_id}`}
      data-rule={card.rule}
      data-testid={card.primary ? 'primary-card' : 'feedback-card'}
      className={cn('@container rounded-lg border border-l-4 bg-card', sev.spine, card.primary ? 'shadow-sm ring-1 ring-foreground/10' : '')}
    >
      <header className="flex flex-wrap items-start gap-x-3 gap-y-2 px-4 pt-3">
        <span className={cn('inline-flex items-center gap-1 text-sm font-medium', sev.text)}>
          <Icon aria-hidden className="size-4" />
          {sev.label}
        </span>
        {card.primary && (
          <span className="inline-flex items-center rounded-md bg-primary px-1.5 py-0.5 text-xs font-medium text-primary-foreground">
            建議先處理這一個
          </span>
        )}
        <ConfidenceBadge card={card} />
        {card.test && (
          <span className="text-xs text-muted-foreground">{card.test.hidden ? `隱藏測資 #${card.test.number}` : `測資 #${card.test.number}`}</span>
        )}
      </header>
      <h3 id={`card-${card.finding_id}`} className={cn('px-4 pt-1.5 font-medium', compact ? 'text-base' : 'text-lg')}>
        <InlineMarkdown>{card.title}</InlineMarkdown>
      </h3>

      <dl className="space-y-2.5 px-4 pt-2.5 pb-3">
        <Section label="發生什麼事">
          <Markdown>{card.what}</Markdown>
        </Section>
        {card.cause && (
          <Section label={guess ? '可能原因（推測）' : '原因'}>
            <Markdown>{card.cause}</Markdown>
          </Section>
        )}
        {(card.where_text || card.span) && (
          <Section label="從哪裡看起">
            <div className="space-y-1.5">
              {card.where_text && <Markdown>{card.where_text}</Markdown>}
              {card.precision_note && <p className="text-sm text-muted-foreground">{card.precision_note}</p>}
              {card.span && onJump && (
                <Button variant="outline" size="sm" onClick={() => onJump(card.span!)} data-testid="jump-button">
                  <LocateFixed aria-hidden />
                  跳到{spanText(card.span)}
                </Button>
              )}
            </div>
          </Section>
        )}
        {card.fixed_region_note && (
          <Section label="注意">
            <p className="flex gap-1.5">
              <Lock aria-hidden className="mt-1 size-4 shrink-0 text-muted-foreground" />
              {card.fixed_region_note}
            </p>
          </Section>
        )}
        {card.cascade_note && (
          <Section label="連鎖反應">
            <p className="text-muted-foreground">{card.cascade_note}</p>
          </Section>
        )}
        {card.why && (
          <Section label="為什麼會這樣">
            <Markdown>{card.why}</Markdown>
          </Section>
        )}
        {card.next_steps.length > 0 && (
          <Section label="下一步">
            <ul className="list-disc space-y-1 pl-5">
              {card.next_steps.map((s, i) => (
                <li key={i}>
                  <InlineMarkdown>{s}</InlineMarkdown>
                </li>
              ))}
            </ul>
          </Section>
        )}
      </dl>

      {(card.hints.length > 0 || canReveal || card.hints_total > 0) && (
        <section aria-label="逐層提示" className="border-t bg-hint-soft/60 px-4 py-3">
          <ol className="space-y-2">
            {card.hints.map((h, i) => (
              <li key={i} className="flex gap-2.5">
                <span className="mt-0.5 inline-flex h-5 shrink-0 items-center gap-1 rounded-md border border-hint/40 bg-card px-1.5 text-xs font-medium text-hint">
                  <Lightbulb aria-hidden className="size-3" />
                  提示 {i + 1}
                </span>
                <div className="min-w-0 flex-1 text-base leading-relaxed">
                  <Markdown>{h}</Markdown>
                </div>
              </li>
            ))}
          </ol>
          {canReveal ? (
            <Button variant="outline" size="sm" className={cn(card.hints.length > 0 && 'mt-3')} onClick={reveal} disabled={revealing} data-testid="reveal-hint">
              <Lightbulb aria-hidden />
              {card.hints.length === 0 ? '給我一個提示' : '再給我一點提示'}（{card.hints.length + 1}/{card.hints_allowed}）
            </Button>
          ) : (
            onRevealHint &&
            card.hints_total > 0 && (
              <p className="mt-2 text-sm text-muted-foreground">
                {card.hints.length < card.hints_total ? '其餘提示目前沒有開放，先試著動手修改看看。' : '已經是最後一層提示了。'}
              </p>
            )
          )}
        </section>
      )}

      {card.raw.length > 0 && (
        <Collapsible defaultOpen={defaultRawOpen} className="border-t">
          <CollapsibleTrigger className="group flex w-full items-center gap-1.5 px-4 py-2 text-left text-sm text-muted-foreground hover:text-foreground">
            <ChevronDown aria-hidden className="size-4 transition-transform group-data-[state=open]:rotate-180" />
            原始訊息（給進階學習者）
          </CollapsibleTrigger>
          <CollapsibleContent>
            <pre className="mx-4 mb-3 overflow-x-auto rounded-md bg-muted p-3 font-mono text-xs leading-relaxed whitespace-pre-wrap">
              {card.raw
                .map((r) => `${r.line ? `main.c:${r.line}:${r.column ?? 1}: ` : ''}${r.kind}: ${r.message}${r.option ? ` [${r.option}]` : ''}`)
                .join('\n')}
            </pre>
          </CollapsibleContent>
        </Collapsible>
      )}
    </article>
  )
}
