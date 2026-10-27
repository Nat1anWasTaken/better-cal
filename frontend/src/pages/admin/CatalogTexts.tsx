import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Save } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { toast } from 'sonner'

import { ApiError, api } from '@/api/client'
import type { TextsEditor } from '@/api/types'
import { Field, SectionCard } from '@/components/admin/Field'
import { ErrorState, Loading, Page, PageHeader } from '@/components/Page'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { useUnsavedGuard } from '@/hooks/useUnsaved'
import { errorMessage, errorsByPointer } from '@/lib/errors'

const LABELS: Record<string, string> = {
  'confidence.confirmed': '信心標籤：編譯器確認',
  'confidence.likely': '信心標籤：很可能',
  'confidence.possible': '信心標籤：推測',
  'precision.near': '位置說明：編譯器在附近發現',
  'precision.approximate': '位置說明：只能推測位置',
  fixed_region: '局部補寫：位置在固定程式內',
  cascade: '連鎖反應說明',
  hidden_test: '隱藏測資說明',
  'compare.trim_trailing_whitespace': '比對方式說明：忽略行尾空白',
  'compare.exact': '比對方式說明：完全相同',
}

export function CatalogTextsPage() {
  const qc = useQueryClient()
  const q = useQuery({ queryKey: ['catalog-texts'], queryFn: () => api.get<TextsEditor>('/admin/catalog/texts') })
  const [texts, setTexts] = useState<Record<string, string> | null>(null)
  const [baseline, setBaseline] = useState('')
  const [errors, setErrors] = useState<Record<string, string>>({})
  useEffect(() => {
    if (q.data) {
      const t = q.data.draft ?? q.data.published
      setTexts(t)
      setBaseline(JSON.stringify(t))
    }
  }, [q.data])
  const dirty = !!texts && JSON.stringify(texts) !== baseline
  useUnsavedGuard(dirty)
  const save = useMutation({
    mutationFn: () => api.put<TextsEditor>('/admin/catalog/texts', texts),
    onSuccess: (r) => {
      qc.setQueryData(['catalog-texts'], r)
      qc.invalidateQueries({ queryKey: ['catalog'] })
      setErrors({})
      toast.success('草稿已儲存，到「教學文案」按「發布全部修改」後生效')
    },
    onError: (e) => {
      setErrors(e instanceof ApiError ? errorsByPointer(e.errors) : {})
      toast.error(errorMessage(e))
    },
  })
  if (q.isLoading || (!texts && !q.error)) return <Loading />
  if (q.error) return <Page><ErrorState error={q.error} /></Page>
  return (
    <Page>
      <PageHeader
        back={
          <Link to="/admin/catalog" className="text-sm text-muted-foreground hover:text-foreground">
            教學文案
          </Link>
        }
        title="共用文字"
        description="出現在所有說明卡片上的標籤與註解。"
        actions={
          <Button onClick={() => save.mutate()} disabled={!dirty || save.isPending}>
            <Save aria-hidden />
            儲存草稿
          </Button>
        }
      />
      <SectionCard title="文字">
        {Object.keys(texts!).map((k) => (
          <Field key={k} label={LABELS[k] ?? k} hint={<code className="font-mono">{k}</code>} error={errors[`/${k}`]}>
            <Textarea rows={2} value={texts![k]} onChange={(e) => setTexts({ ...texts!, [k]: e.target.value })} />
          </Field>
        ))}
      </SectionCard>
    </Page>
  )
}
