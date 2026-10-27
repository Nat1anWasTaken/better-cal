import { useMutation } from '@tanstack/react-query'
import { Download, FileUp } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { ApiError, api } from '@/api/client'
import type { ImportReport } from '@/api/types'
import { Field, SectionCard } from '@/components/admin/Field'
import { Page, PageHeader } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { errorMessage } from '@/lib/errors'

const ACTION_LABEL: Record<string, { label: string; tone: 'success' | 'warning' | 'neutral' }> = {
  create: { label: '新增', tone: 'success' },
  update: { label: '更新（新版本）', tone: 'warning' },
  unchanged: { label: '沒有變更', tone: 'neutral' },
}

function Report({ r }: { r: ImportReport }) {
  return (
    <div className="space-y-3" role="status">
      <p className={r.ok ? 'text-success' : 'text-destructive'}>
        {r.ok ? (r.dry_run ? '檢查通過，可以匯入。下面是匯入後會發生的事：' : '匯入完成。匯入的內容都是草稿，記得到題目庫驗證並發布。') : '文件有問題，沒有匯入任何內容：'}
      </p>
      {r.errors.length > 0 && (
        <ul className="list-disc space-y-1 pl-5 text-sm">
          {r.errors.map((e, i) => (
            <li key={i}>
              <code className="font-mono text-xs">{e.pointer || '/'}</code>：{e.message}
            </li>
          ))}
        </ul>
      )}
      {[...r.problems.map((x) => ({ ...x, kind: '題目' })), ...r.problem_sets.map((x) => ({ ...x, kind: '題目集' }))].length > 0 && (
        <ul className="divide-y rounded-md border text-sm">
          {[...r.problems.map((x) => ({ ...x, kind: '題目' })), ...r.problem_sets.map((x) => ({ ...x, kind: '題目集' }))].map((x) => {
            const a = ACTION_LABEL[x.action] ?? { label: x.action, tone: 'neutral' as const }
            return (
              <li key={x.pointer} className="flex items-center gap-3 px-3 py-1.5">
                <span className="w-12 text-muted-foreground">{x.kind}</span>
                <span className="flex-1 font-mono text-xs">{x.slug}</span>
                <StatusPill tone={a.tone} label={a.label} />
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}

export function ImportExportPage() {
  const [text, setText] = useState('')
  const [report, setReport] = useState<ImportReport | null>(null)
  const [checked, setChecked] = useState<string | null>(null)
  const run = useMutation({
    mutationFn: async (dry: boolean) => {
      let doc: unknown
      try {
        doc = JSON.parse(text)
      } catch (e) {
        throw new Error(`JSON 格式錯誤：${(e as Error).message}`, { cause: e })
      }
      return api.post<ImportReport>(`/admin/import${dry ? '?dry_run=true' : ''}`, doc, dry ? undefined : { headers: { 'Idempotency-Key': crypto.randomUUID() } })
    },
    onSuccess: (r, dry) => {
      setReport(r)
      if (dry && r.ok) setChecked(text)
      if (!dry) setChecked(null)
    },
    onError: (e) => {
      // 422 is problem+json; the full ImportReport is in `data`.
      if (e instanceof ApiError && e.errors.length) setReport((e.data as ImportReport | null) ?? { dry_run: true, ok: false, errors: e.errors, problems: [], problem_sets: [] })
      else toast.error(errorMessage(e))
    },
  })
  const exportAll = async () => {
    try {
      const doc = await api.get('/admin/export')
      const url = URL.createObjectURL(new Blob([JSON.stringify(doc, null, 2)], { type: 'application/json' }))
      const a = document.createElement('a')
      a.href = url
      a.download = `cycu-cal-export-${new Date().toISOString().slice(0, 10)}.json`
      a.click()
      URL.revokeObjectURL(url)
    } catch (e) {
      toast.error(errorMessage(e))
    }
  }
  return (
    <Page wide>
      <PageHeader title="匯入／匯出" description="使用 cycu-cal.import/v1 JSON 格式。以 slug 對應既有內容：相同內容不會重複建立，內容不同會產生新版本。匯入的內容一律是草稿。" />
      <div className="space-y-6">
        <SectionCard
          title="匯入"
          actions={
            <label className="inline-flex cursor-pointer items-center gap-1.5 rounded-md border px-3 py-1.5 text-sm hover:bg-accent">
              <FileUp aria-hidden className="size-4" />
              選擇檔案
              <input
                type="file"
                accept="application/json,.json"
                className="sr-only"
                onChange={async (e) => {
                  const f = e.target.files?.[0]
                  if (f) {
                    setText(await f.text())
                    setReport(null)
                  }
                }}
              />
            </label>
          }
        >
          <Field label="JSON 內容" htmlFor="import-json">
            <Textarea id="import-json" rows={14} variant="code-sm" value={text} onChange={(e) => (setText(e.target.value), setReport(null))} placeholder='{"format_version": "cycu-cal.import/v1", "problems": [...], "problem_sets": [...]}' />
          </Field>
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" disabled={!text || run.isPending} onClick={() => run.mutate(true)}>
              先檢查（不會寫入）
            </Button>
            <Button disabled={checked !== text || run.isPending} onClick={() => run.mutate(false)}>
              確認匯入
            </Button>
            {checked !== text && text && <span className="self-center text-sm text-muted-foreground">請先檢查，通過後才能匯入。</span>}
          </div>
          {report && <Report r={report} />}
        </SectionCard>
        <SectionCard title="匯出" description="匯出所有題目（最新版本）與題目集，可以直接再匯入或交給其他工具。">
          <Button variant="outline" onClick={exportAll}>
            <Download aria-hidden />
            下載 JSON
          </Button>
        </SectionCard>
      </div>
    </Page>
  )
}
