import type { FieldError } from './types'

/** problem+json error from the API (RFC 9457). */
export class ApiError extends Error {
  status: number
  code: string
  errors: FieldError[]
  data: unknown

  constructor(status: number, code: string, detail: string, errors: FieldError[] = [], data: unknown = null) {
    super(detail)
    this.status = status
    this.code = code
    this.errors = errors
    this.data = data
  }
}

let csrfToken: string | null = null

export function setCsrfToken(token: string | null | undefined) {
  csrfToken = token ?? null
}

export interface RequestOptions {
  body?: unknown
  headers?: Record<string, string>
  signal?: AbortSignal
}

async function parseError(res: Response): Promise<ApiError> {
  let body: Record<string, unknown> = {}
  try {
    body = (await res.json()) as Record<string, unknown>
  } catch {
    // Non-JSON error body (proxy, network gateway).
  }
  const detail =
    typeof body.detail === 'string'
      ? body.detail
      : res.status >= 500
        ? '伺服器暫時無法處理請求，請稍後再試。'
        : `請求失敗（HTTP ${res.status}）`
  return new ApiError(
    res.status,
    typeof body.code === 'string' ? body.code : `http_${res.status}`,
    detail,
    Array.isArray(body.errors) ? (body.errors as FieldError[]) : [],
    body.data ?? null,
  )
}

export async function request<T>(method: string, path: string, opts: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = { Accept: 'application/json', ...opts.headers }
  if (opts.body !== undefined) headers['Content-Type'] = 'application/json'
  if (method !== 'GET' && method !== 'HEAD' && csrfToken) headers['X-CSRF-Token'] = csrfToken
  let res: Response
  try {
    res = await fetch(`/api/v1${path}`, {
      method,
      headers,
      credentials: 'include',
      body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
      signal: opts.signal,
    })
  } catch (e) {
    if ((e as Error).name === 'AbortError') throw e
    throw new ApiError(0, 'network', '無法連線到伺服器，請檢查網路連線。')
  }
  if (!res.ok) throw await parseError(res)
  if (res.status === 204) return undefined as T
  const text = await res.text()
  return (text ? JSON.parse(text) : undefined) as T
}

export const api = {
  get: <T>(path: string, opts?: RequestOptions) => request<T>('GET', path, opts),
  post: <T>(path: string, body?: unknown, opts?: RequestOptions) => request<T>('POST', path, { ...opts, body }),
  put: <T>(path: string, body?: unknown, opts?: RequestOptions) => request<T>('PUT', path, { ...opts, body }),
  patch: <T>(path: string, body?: unknown, opts?: RequestOptions) => request<T>('PATCH', path, { ...opts, body }),
  del: <T>(path: string, opts?: RequestOptions) => request<T>('DELETE', path, opts),
}

export function qs(params: Record<string, string | number | boolean | null | undefined>): string {
  const p = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== null && v !== '') p.set(k, String(v))
  const s = p.toString()
  return s ? `?${s}` : ''
}
