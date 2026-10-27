import { ChevronDown } from 'lucide-react'

import type { FeedbackCard as Card, Span } from '@/api/types'
import { FeedbackCard } from '@/components/FeedbackCard'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { groupCards } from '@/lib/cards'

interface Props {
  cards: Card[]
  onJump?: (span: Span) => void
  onRevealHint?: (findingId: number) => Promise<void>
  rawExpanded?: boolean
}

function Group({ title, cards, defaultOpen, ...rest }: Omit<Props, 'cards'> & { title: string; cards: Card[]; defaultOpen: boolean }) {
  if (cards.length === 0) return null
  return (
    <Collapsible defaultOpen={defaultOpen}>
      <CollapsibleTrigger className="group flex w-full items-center gap-2 rounded-md py-1.5 text-left font-medium hover:text-primary">
        <ChevronDown aria-hidden className="size-4 transition-transform group-data-[state=open]:rotate-180" />
        {title}
      </CollapsibleTrigger>
      <CollapsibleContent className="space-y-3 pt-2">
        {cards.map((c) => (
          <FeedbackCard key={c.finding_id} card={c} onJump={rest.onJump} onRevealHint={rest.onRevealHint} defaultRawOpen={rest.rawExpanded} compact />
        ))}
      </CollapsibleContent>
    </Collapsible>
  )
}

/** Primary card first, then other errors and results; knock-on errors and
 * warnings are grouped (collapsed) so the student is not overwhelmed. */
export function CardList({ cards, onJump, onRevealHint, rawExpanded }: Props) {
  const g = groupCards(cards)
  const shared = { onJump, onRevealHint, rawExpanded }
  return (
    <div className="space-y-4">
      {g.primary && <FeedbackCard card={g.primary} onJump={onJump} onRevealHint={onRevealHint} defaultRawOpen={rawExpanded} />}
      {[...g.errors, ...g.results].map((c) => (
        <FeedbackCard key={c.finding_id} card={c} onJump={onJump} onRevealHint={onRevealHint} defaultRawOpen={rawExpanded} compact />
      ))}
      <Group {...shared} title={`可能是連鎖反應的訊息（${g.cascades.length} 則）`} cards={g.cascades} defaultOpen={false} />
      <Group {...shared} title={`建議檢查（${g.warnings.length} 則）`} cards={g.warnings} defaultOpen={!g.primary || g.primary.severity !== 'error'} />
    </div>
  )
}
