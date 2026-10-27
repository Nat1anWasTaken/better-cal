import { ArrowDown, ArrowUp, Plus, Trash2 } from 'lucide-react'

import { FieldMessage } from '@/components/Page'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'

/** Editable ordered list of text items (next steps, hints). */
export function StringList({
  items,
  onChange,
  label,
  addLabel,
  errors = {},
  basePointer = '',
  rows = 2,
}: {
  items: string[]
  onChange: (v: string[]) => void
  label: (i: number) => string
  addLabel: string
  errors?: Record<string, string>
  basePointer?: string
  rows?: number
}) {
  const move = (i: number, d: number) => {
    const n = [...items]
    const [x] = n.splice(i, 1)
    n.splice(i + d, 0, x)
    onChange(n)
  }
  return (
    <div className="space-y-2">
      {items.map((s, i) => (
        <div key={i} className="space-y-1">
          <div className="flex items-center gap-1">
            <span className="text-xs text-muted-foreground">{label(i)}</span>
            <Button variant="ghost" size="icon" className="ml-auto size-7" aria-label={`${label(i)} 上移`} disabled={i === 0} onClick={() => move(i, -1)}>
              <ArrowUp aria-hidden />
            </Button>
            <Button variant="ghost" size="icon" className="size-7" aria-label={`${label(i)} 下移`} disabled={i === items.length - 1} onClick={() => move(i, 1)}>
              <ArrowDown aria-hidden />
            </Button>
            <Button variant="ghost" size="icon" className="size-7" aria-label={`刪除${label(i)}`} onClick={() => onChange(items.filter((_, j) => j !== i))}>
              <Trash2 aria-hidden />
            </Button>
          </div>
          <Textarea aria-label={label(i)} rows={rows} value={s} onChange={(e) => onChange(items.map((x, j) => (j === i ? e.target.value : x)))} />
          <FieldMessage message={errors[`${basePointer}/${i}`]} />
        </div>
      ))}
      <Button variant="outline" size="sm" onClick={() => onChange([...items, ''])}>
        <Plus aria-hidden />
        {addLabel}
      </Button>
    </div>
  )
}
