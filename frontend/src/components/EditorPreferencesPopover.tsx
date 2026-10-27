import { Settings2 } from 'lucide-react'
import { Link } from 'react-router'
import { toast } from 'sonner'

import { EditorPreferencesFields } from '@/components/EditorPreferencesFields'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { usePreferences } from '@/hooks/usePreferences'
import { errorMessage } from '@/lib/errors'

/** Quick editor settings for the problem page toolbar; each change is saved right away. */
export function EditorPreferencesPopover() {
  const { preferences, save } = usePreferences()
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="sm" aria-label="編輯器設定" title="編輯器設定">
          <Settings2 aria-hidden />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-72">
        <EditorPreferencesFields idPrefix="quick-pref" quick value={preferences} onChange={(p) => save(p).catch((e) => toast.error(errorMessage(e)))} />
        <Link to="/settings" className="mt-3 block text-sm text-muted-foreground underline-offset-4 hover:underline">
          更多設定…
        </Link>
      </PopoverContent>
    </Popover>
  )
}
