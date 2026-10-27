import { useMutation } from '@tanstack/react-query'
import { RotateCcw, Save } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { ApiError } from '@/api/client'
import type { UserPreferences } from '@/api/types'
import { SectionCard } from '@/components/admin/Field'
import { CodeEditor } from '@/components/CodeEditor'
import { EditorPreferencesFields } from '@/components/EditorPreferencesFields'
import { Page, PageHeader } from '@/components/Page'
import { Button } from '@/components/ui/button'
import { usePreferences } from '@/hooks/usePreferences'
import { useUnsavedGuard } from '@/hooks/useUnsaved'
import { errorMessage, errorsByPointer } from '@/lib/errors'
import { DEFAULT_PREFERENCES } from '@/lib/preferences'

const sample = `#include <stdio.h>

int main(void) {
    int n, sum = 0;
    scanf("%d", &n);
    for (int i = 1; i <= n; i++) {
        sum += i;  // 試試看在這裡打字
    }
    printf("%d\\n", sum);
    return 0;
}
`

export function PreferencesPage() {
  const { preferences, save } = usePreferences()
  const [draft, setDraft] = useState<UserPreferences>(preferences)
  const [errors, setErrors] = useState<Record<string, string>>({})
  const dirty = JSON.stringify(draft) !== JSON.stringify(preferences)
  useUnsavedGuard(dirty)
  const m = useMutation({
    mutationFn: () => save(draft),
    onSuccess: (saved) => {
      setDraft(saved)
      setErrors({})
      toast.success('偏好設定已儲存，換一台電腦登入也會套用')
    },
    onError: (e) => {
      setErrors(e instanceof ApiError ? errorsByPointer(e.errors) : {})
      toast.error(errorMessage(e))
    },
  })
  return (
    <Page wide>
      <PageHeader
        title="偏好設定"
        description="設定會存在你的帳號裡，在任何一台電腦登入都會套用。"
        actions={
          <>
            <Button variant="ghost" onClick={() => setDraft(DEFAULT_PREFERENCES)} disabled={JSON.stringify(draft) === JSON.stringify(DEFAULT_PREFERENCES)}>
              <RotateCcw aria-hidden />
              還原預設
            </Button>
            <Button onClick={() => m.mutate()} disabled={!dirty || m.isPending}>
              <Save aria-hidden />
              {m.isPending ? '儲存中…' : '儲存設定'}
            </Button>
          </>
        }
      />
      <div className="grid gap-6 lg:grid-cols-2">
        <SectionCard title="程式編輯器">
          <EditorPreferencesFields idPrefix="pref" value={draft} onChange={setDraft} errors={errors} />
        </SectionCard>
        <SectionCard title="預覽" description="可以直接在這裡試打，內容不會被儲存。">
          <CodeEditor initialDoc={sample} preferences={draft} className="h-72 overflow-hidden rounded-md border" ariaLabel="編輯器預覽" />
        </SectionCard>
      </div>
    </Page>
  )
}
