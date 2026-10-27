import { ChevronDown, EyeOff } from 'lucide-react'

import type { TestView } from '@/api/types'
import { StatusPill } from '@/components/StatusPill'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { diffSummary, renderDiffLine, showWhitespace } from '@/lib/diff'
import { verdictInfo } from '@/lib/format'
import { cn } from '@/lib/utils'

function Block({ label, text, testid }: { label: string; text: string | null | undefined; testid?: string }) {
  return (
    <div className="min-w-0">
      <div className="mb-1 text-xs font-medium text-muted-foreground">{label}</div>
      <pre data-testid={testid} className="max-h-48 overflow-auto rounded-md border bg-code p-2 font-mono text-xs leading-relaxed whitespace-pre-wrap">
        {text ? showWhitespace(text) : <span className="text-muted-foreground italic">（沒有內容）</span>}
      </pre>
    </div>
  )
}

function Diff({ t }: { t: TestView }) {
  if (!t.diff) return null
  const s = diffSummary(t.diff)
  return (
    <div>
      <p className="mb-1.5 text-sm">
        比較結果：{s.firstLine ? `從第 ${s.firstLine} 行開始不同。` : ''}
        <span className="text-muted-foreground">
          「−」是預期有、你沒有的行；「+」是你多出來的行。· 代表空白、↵ 代表換行。
        </span>
      </p>
      <div role="table" aria-label="預期輸出與你的輸出比較" className="overflow-x-auto rounded-md border bg-code font-mono text-xs">
        {t.diff.map((l, i) => {
          const r = renderDiffLine(l)
          return (
            <div
              role="row"
              key={i}
              className={cn(
                'flex gap-2 px-2 py-0.5',
                r.kind === 'missing' && 'bg-danger-soft',
                r.kind === 'extra' && 'bg-warning-soft',
              )}
            >
              <span role="cell" aria-label={r.label} className="w-4 shrink-0 text-center font-bold">
                {r.marker}
              </span>
              <span role="cell" className="w-8 shrink-0 text-right text-muted-foreground">
                {r.expected ?? r.actual ?? ''}
              </span>
              <span role="cell" className="whitespace-pre">
                {r.text}
              </span>
              {r.kind !== 'same' && <span className="sr-only">（{r.label}）</span>}
            </div>
          )
        })}
        {t.diff_truncated && <div className="px-2 py-1 text-muted-foreground">…內容太長，只顯示前面的部分</div>}
      </div>
    </div>
  )
}

export function TestResults({ tests, compareDescription }: { tests: TestView[]; compareDescription?: string }) {
  if (tests.length === 0) return null
  return (
    <div className="space-y-2">
      {compareDescription && <p className="text-sm text-muted-foreground">{compareDescription}</p>}
      <ul className="space-y-2">
        {tests.map((t) => {
          const v = verdictInfo(t.verdict)
          const title = t.custom ? '自訂輸入' : t.hidden ? `隱藏測資 #${t.number}` : `測資 #${t.number}${t.name ? `：${t.name}` : ''}`
          const failed = t.verdict !== 'accepted' && t.verdict !== 'completed'
          return (
            <li key={t.number} className="rounded-lg border bg-card" data-testid="test-result">
              <Collapsible defaultOpen={!t.hidden && (failed || t.custom)}>
                <CollapsibleTrigger disabled={t.hidden} className="group flex w-full flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-left">
                  {!t.hidden && <ChevronDown aria-hidden className="size-4 transition-transform group-data-[state=open]:rotate-180" />}
                  {t.hidden && <EyeOff aria-hidden className="size-4 text-muted-foreground" />}
                  <span className="font-medium">{title}</span>
                  <StatusPill tone={v.tone} label={v.label} />
                  <span className="ml-auto text-xs text-muted-foreground">
                    {t.time_ms} ms · {Math.round(t.memory_kb / 1024)} MB
                  </span>
                </CollapsibleTrigger>
                {!t.hidden && (
                  <CollapsibleContent className="space-y-3 border-t px-3 py-3">
                    <div className={cn('grid gap-3', t.custom ? 'md:grid-cols-2' : 'md:grid-cols-3')}>
                      <Block label="輸入" text={t.input} />
                      {!t.custom && <Block label="預期輸出" text={t.expected_output} />}
                      <Block label={t.custom ? '程式的輸出' : '你的輸出'} text={t.actual_output} testid="actual-output" />
                    </div>
                    {t.output_truncated && <p className="text-sm text-warning">輸出太長，只顯示前面的部分。</p>}
                    <Diff t={t} />
                    {t.stderr && <Block label="錯誤輸出（stderr）" text={t.stderr} />}
                  </CollapsibleContent>
                )}
              </Collapsible>
            </li>
          )
        })}
      </ul>
    </div>
  )
}
