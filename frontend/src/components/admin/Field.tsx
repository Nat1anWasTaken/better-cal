import { useQuery } from '@tanstack/react-query'
import type { ReactNode } from 'react'

import { api } from '@/api/client'
import type { Concept } from '@/api/types'
import { FieldMessage } from '@/components/Page'
import { Checkbox } from '@/components/ui/checkbox'
import { Label } from '@/components/ui/label'
import { cn } from '@/lib/utils'

export function Field({ label, htmlFor, error, hint, children, className }: { label: ReactNode; htmlFor?: string; error?: string; hint?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <div className={cn('space-y-1.5', className)}>
      <Label htmlFor={htmlFor}>{label}</Label>
      {children}
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
      <FieldMessage message={error} />
    </div>
  )
}

export function useConcepts() {
  return useQuery({ queryKey: ['concepts'], queryFn: () => api.get<Concept[]>('/concepts'), staleTime: Infinity })
}

export function conceptLabel(concepts: Concept[] | undefined, id: string) {
  return concepts?.find((c) => c.id === id)?.label ?? id
}

export function ConceptPicker({ value, onChange, idPrefix }: { value: string[]; onChange: (v: string[]) => void; idPrefix: string }) {
  const q = useConcepts()
  return (
    <fieldset className="grid gap-1.5 sm:grid-cols-2">
      {q.data?.map((c) => {
        const id = `${idPrefix}-${c.id}`
        return (
          <div key={c.id} className="flex items-center gap-2">
            <Checkbox id={id} checked={value.includes(c.id)} onCheckedChange={(on) => onChange(on ? [...value, c.id] : value.filter((x) => x !== c.id))} />
            <Label htmlFor={id} className="font-normal">
              {c.label}
            </Label>
          </div>
        )
      })}
    </fieldset>
  )
}

export function SectionCard({ title, description, children, actions }: { title: ReactNode; description?: ReactNode; children: ReactNode; actions?: ReactNode }) {
  return (
    <section className="rounded-lg border bg-card">
      <div className="flex flex-wrap items-start gap-2 border-b px-4 py-3">
        <div>
          <h2 className="font-medium">{title}</h2>
          {description && <p className="text-sm text-muted-foreground">{description}</p>}
        </div>
        {actions && <div className="ml-auto flex flex-wrap gap-2">{actions}</div>}
      </div>
      <div className="space-y-4 p-4">{children}</div>
    </section>
  )
}
