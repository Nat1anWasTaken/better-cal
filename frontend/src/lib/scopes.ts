// Permissions for API tokens and MCP connections (server: `auth::Scope`).
export const SCOPES: Array<{ id: string; label: string }> = [
  { id: 'problems:read', label: '讀取題目、題目集與匯出' },
  { id: 'problems:write', label: '建立與修改題目、匯入' },
  { id: 'sets:write', label: '建立與修改題目集' },
  { id: 'publish', label: '發布題目／題目集、切換啟用的題目集' },
  { id: 'submissions:read', label: '讀取所有提交' },
  { id: 'content:write', label: '編輯教學文案' },
  { id: 'students:read', label: '讀取學生與進度' },
  { id: 'students:write', label: '管理學生帳號與組別' },
  { id: 'settings:write', label: '修改全站設定、讀取操作紀錄' },
]

/** What an agent typically needs to author and publish problems. */
export const AUTHORING_SCOPES = ['problems:read', 'problems:write', 'sets:write', 'publish']

export function scopeLabel(id: string) {
  return SCOPES.find((s) => s.id === id)?.label ?? id
}

/** Known scopes from `requested`, in display order. */
export function knownScopes(requested: string[]) {
  return SCOPES.map((s) => s.id).filter((id) => requested.includes(id))
}
