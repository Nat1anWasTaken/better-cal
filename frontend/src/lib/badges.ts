import { Flame, Sparkles, type LucideIcon } from 'lucide-react'

/** Leaderboard badge kinds from the server. Add an entry here when the server adds a kind. */
export const BADGES: Record<string, { label: string; desc: string; icon: LucideIcon; cls: string }> = {
  first_solve: { label: '首殺', desc: '本輪第一位通過這一題', icon: Flame, cls: 'text-warning' },
  no_hints: { label: '無提示通過', desc: '沒有查看任何提示就通過這一題', icon: Sparkles, cls: 'text-primary' },
}

export function badgeInfo(kind: string) {
  return BADGES[kind] ?? { label: kind, desc: kind, icon: Sparkles, cls: 'text-muted-foreground' }
}
