import { ApiError } from '@/api/client'
import type { FieldError } from '@/api/types'

/** Maps problem+json field errors by JSON Pointer, optionally stripping a prefix
 * (e.g. `/content`) so form fields can look up `/title`. */
export function errorsByPointer(errors: FieldError[], prefix = ''): Record<string, string> {
  const out: Record<string, string> = {}
  for (const e of errors) {
    if (!e.pointer.startsWith(prefix)) continue
    const key = e.pointer.slice(prefix.length) || '/'
    out[key] = out[key] ? `${out[key]}；${e.message}` : e.message
  }
  return out
}

/** Errors at `pointer` or any child of it. */
export function errorsUnder(map: Record<string, string>, pointer: string): string[] {
  return Object.entries(map)
    .filter(([k]) => k === pointer || k.startsWith(`${pointer}/`))
    .map(([, v]) => v)
}

export function errorMessage(e: unknown): string {
  if (e instanceof ApiError) return e.message
  if (e instanceof Error) return e.message
  return '發生未預期的錯誤'
}

export function fieldErrors(e: unknown, prefix = ''): Record<string, string> {
  return e instanceof ApiError ? errorsByPointer(e.errors, prefix) : {}
}
