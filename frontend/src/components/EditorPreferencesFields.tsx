import type { UserPreferences } from '@/api/types'
import { Field } from '@/components/admin/Field'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'

const themeLabels: Record<UserPreferences['editor_theme'], string> = {
  auto: '跟隨網站外觀',
  light: '淺色',
  dark: '深色',
  hc_dark: '高對比（深色）',
  hc_light: '高對比（淺色）',
}
const lineNumberLabels: Record<UserPreferences['line_numbers'], string> = { on: '顯示', relative: '相對行號', off: '不顯示' }
const fontSizes = [10, 11, 12, 13, 14, 15, 16, 18, 20, 22, 24, 28]
const tabSizes = [2, 4, 8]

interface Props {
  value: UserPreferences
  onChange: (p: UserPreferences) => void
  errors?: Record<string, string>
  idPrefix: string
  /** Only the settings people change most (editor toolbar popover). */
  quick?: boolean
}

export function EditorPreferencesFields({ value, onChange, errors = {}, idPrefix, quick }: Props) {
  const set = (p: Partial<UserPreferences>) => onChange({ ...value, ...p })
  const toggle = (key: 'word_wrap' | 'minimap', label: string) => (
    <div className="flex items-center gap-2">
      <Switch id={`${idPrefix}-${key}`} checked={value[key]} onCheckedChange={(on) => set({ [key]: on })} />
      <Label htmlFor={`${idPrefix}-${key}`} variant="option">
        {label}
      </Label>
    </div>
  )
  return (
    <div className={quick ? 'space-y-3' : 'grid gap-4 sm:grid-cols-2'}>
      <Field label="編輯器配色" error={errors['/editor_theme']}>
        <Select value={value.editor_theme} onValueChange={(v) => set({ editor_theme: v as UserPreferences['editor_theme'] })}>
          <SelectTrigger aria-label="編輯器配色" className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {Object.entries(themeLabels).map(([k, label]) => (
              <SelectItem key={k} value={k}>
                {label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>
      <Field label="字體大小" error={errors['/font_size']}>
        <Select value={String(value.font_size)} onValueChange={(v) => set({ font_size: Number(v) })}>
          <SelectTrigger aria-label="字體大小" className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {fontSizes.map((n) => (
              <SelectItem key={n} value={String(n)}>
                {n} px
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>
      <div className="space-y-1.5 sm:col-span-2">
        <div className="flex items-center gap-2">
          <Switch id={`${idPrefix}-vim`} checked={value.keymap === 'vim'} onCheckedChange={(on) => set({ keymap: on ? 'vim' : 'default' })} />
          <Label htmlFor={`${idPrefix}-vim`} variant="option">
            Vim 模式
          </Label>
        </div>
        {!quick && <p className="text-xs text-muted-foreground">用 Vim 的按鍵操作編輯器，編輯器下方會顯示目前的模式。Ctrl+Enter 執行、Ctrl+Shift+Enter 提交仍然可用。</p>}
      </div>
      {!quick && (
        <>
          <Field label="縮排寬度" error={errors['/tab_size']}>
            <Select value={String(value.tab_size)} onValueChange={(v) => set({ tab_size: Number(v) })}>
              <SelectTrigger aria-label="縮排寬度" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {tabSizes.map((n) => (
                  <SelectItem key={n} value={String(n)}>
                    {n} 個空白
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
          <Field label="行號" error={errors['/line_numbers']}>
            <Select value={value.line_numbers} onValueChange={(v) => set({ line_numbers: v as UserPreferences['line_numbers'] })}>
              <SelectTrigger aria-label="行號" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {Object.entries(lineNumberLabels).map(([k, label]) => (
                  <SelectItem key={k} value={k}>
                    {label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
          {toggle('word_wrap', '長的行自動換行')}
          {toggle('minimap', '顯示程式縮圖（minimap）')}
        </>
      )}
    </div>
  )
}
