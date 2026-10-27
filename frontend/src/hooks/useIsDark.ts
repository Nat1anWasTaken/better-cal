import { useEffect, useState } from 'react'

const isDark = () => document.documentElement.classList.contains('dark')

/** Whether the site is currently in dark mode (the `dark` class on <html>). */
export function useIsDark(): boolean {
  const [dark, setDark] = useState(isDark)
  useEffect(() => {
    const obs = new MutationObserver(() => setDark(isDark()))
    obs.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] })
    return () => obs.disconnect()
  }, [])
  return dark
}
