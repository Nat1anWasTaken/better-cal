import { CircleAlert, CircleCheck, CircleDashed, CircleMinus, LoaderCircle, TriangleAlert } from 'lucide-react'

import type { Tone } from '@/lib/format'
import { cn } from '@/lib/utils'

const TONE_CLASS: Record<Tone, string> = {
  success: 'bg-success-soft text-success border-success/30',
  error: 'bg-danger-soft text-destructive border-destructive/30',
  warning: 'bg-warning-soft text-warning border-warning/30',
  neutral: 'bg-muted text-muted-foreground border-border',
  progress: 'bg-primary-soft text-primary border-primary/30',
}

const TONE_ICON: Record<Tone, typeof CircleCheck> = {
  success: CircleCheck,
  error: CircleAlert,
  warning: TriangleAlert,
  neutral: CircleMinus,
  progress: LoaderCircle,
}

/** Status shown with an icon AND text, never colour alone. */
export function StatusPill({ tone, label, className, size = 'sm' }: { tone: Tone; label: string; className?: string; size?: 'sm' | 'lg' }) {
  const Icon = tone === 'neutral' && label === '尚未開始' ? CircleDashed : TONE_ICON[tone]
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1.5 rounded-full border font-medium whitespace-nowrap',
        size === 'lg' ? 'px-3 py-1 text-sm' : 'px-2 py-0.5 text-xs',
        TONE_CLASS[tone],
        className,
      )}
    >
      <Icon aria-hidden className={cn(size === 'lg' ? 'size-4' : 'size-3.5', tone === 'progress' && 'animate-spin')} />
      {label}
    </span>
  )
}
