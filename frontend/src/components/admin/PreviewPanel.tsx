import { useMutation } from '@tanstack/react-query'
import { Eye } from 'lucide-react'
import { useState } from 'react'

import { api } from '@/api/client'
import { type PreviewResult, previewToFeedback } from '@/api/types'
import { ConceptPicker, SectionCard } from '@/components/admin/Field'
import { CardList } from '@/components/CardList'
import { ErrorState } from '@/components/Page'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { SAMPLES } from '@/lib/samples'

/** Compiles code with the real toolchain and shows the cards students would see (all hints). */
export function PreviewPanel({ initialSample = 'semicolon' }: { initialSample?: string }) {
  const [sample, setSample] = useState(initialSample)
  const [code, setCode] = useState(SAMPLES.find((s) => s.id === initialSample)?.code ?? SAMPLES[0].code)
  const [concepts, setConcepts] = useState<string[]>(['io', 'variables'])
  const [drafts, setDrafts] = useState(true)
  const run = useMutation({ mutationFn: () => api.post<PreviewResult>('/admin/catalog/preview', { source: code, concepts, use_drafts: drafts }) })
  return (
    <SectionCard title="預覽學生看到的說明" description="用真實編譯器編譯下面的程式，套用文案（含草稿）後顯示卡片，所有提示層級都會展開。">
      <div className="flex flex-wrap items-end gap-3">
        <div className="space-y-1.5">
          <Label>範例程式</Label>
          <Select
            value={sample}
            onValueChange={(v) => {
              setSample(v)
              setCode(SAMPLES.find((s) => s.id === v)?.code ?? '')
            }}
          >
            <SelectTrigger className="w-52" aria-label="選擇範例程式">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {SAMPLES.map((s) => (
                <SelectItem key={s.id} value={s.id}>
                  {s.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex items-center gap-2 pb-2">
          <Switch id="use-drafts" checked={drafts} onCheckedChange={setDrafts} />
          <Label htmlFor="use-drafts" variant="option">
            包含尚未發布的草稿
          </Label>
        </div>
      </div>
      <Textarea aria-label="預覽用的程式" rows={9} variant="code" value={code} onChange={(e) => setCode(e.target.value)} />
      <div className="space-y-1.5">
        <Label>假設學生已學過</Label>
        <ConceptPicker idPrefix="pv" value={concepts} onChange={setConcepts} />
      </div>
      <Button onClick={() => run.mutate()} disabled={run.isPending} data-testid="run-preview">
        <Eye aria-hidden />
        {run.isPending ? '編譯中…' : '產生預覽'}
      </Button>
      {run.error && <ErrorState error={run.error} />}
      {run.data && (
        <div className="space-y-2" data-testid="preview-result">
          <p className="text-sm text-muted-foreground">{run.data.compile_success ? '程式可以編譯。' : '程式無法編譯。'}共 {run.data.cards.length} 張說明卡。</p>
          <CardList cards={run.data.cards.map(previewToFeedback)} />
        </div>
      )}
    </SectionCard>
  )
}
