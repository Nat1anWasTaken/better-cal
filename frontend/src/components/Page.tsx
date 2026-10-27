import { CircleAlert, LoaderCircle } from 'lucide-react'
import type { ReactNode } from 'react'

import { errorMessage } from '@/lib/errors'
import { cn } from '@/lib/utils'

export function Page({ children, className, wide }: { children: ReactNode; className?: string; wide?: boolean }) {
  return <div className={cn('mx-auto w-full px-4 py-6 sm:px-6', wide ? 'max-w-6xl' : 'max-w-4xl', className)}>{children}</div>
}

export function PageHeader({ title, description, actions, back }: { title: ReactNode; description?: ReactNode; actions?: ReactNode; back?: ReactNode }) {
  return (
    <div className="mb-6 space-y-1">
      {back}
      <div className="flex flex-wrap items-end gap-3">
        <h1 className="text-2xl font-medium tracking-tight">{title}</h1>
        {actions && <div className="ml-auto flex flex-wrap items-center gap-2">{actions}</div>}
      </div>
      {description && <div className="max-w-prose text-muted-foreground">{description}</div>}
    </div>
  )
}

export function Loading({ label = '載入中…' }: { label?: string }) {
  return (
    <div role="status" className="flex items-center gap-2 py-10 text-muted-foreground">
      <LoaderCircle aria-hidden className="size-4 animate-spin" />
      {label}
    </div>
  )
}

export function ErrorState({ error, children }: { error: unknown; children?: ReactNode }) {
  return (
    <div role="alert" className="flex items-start gap-2 rounded-lg border border-destructive/30 bg-danger-soft p-4 text-destructive">
      <CircleAlert aria-hidden className="mt-0.5 size-4 shrink-0" />
      <div>
        <p>{errorMessage(error)}</p>
        {children}
      </div>
    </div>
  )
}

export function Empty({ children }: { children: ReactNode }) {
  return <div className="rounded-lg border border-dashed p-8 text-center text-muted-foreground">{children}</div>
}

export function FieldMessage({ message, id }: { message?: string; id?: string }) {
  if (!message) return null
  return (
    <p id={id} className="text-sm text-destructive">
      {message}
    </p>
  )
}
