import { expect, test } from '@playwright/test'

import { setCode } from './helpers'

test.use({ storageState: 'e2e/.auth/student.json' })

test('draft is autosaved and restored after reload', async ({ page }) => {
  await page.goto('/problems/sum-1-to-n')
  const marker = `// draft ${Date.now()}`
  await setCode(page, `${marker}\n#include <stdio.h>\nint main(void) { return 0; }\n`)
  await expect(page.getByText(/^已儲存/)).toBeVisible({ timeout: 10_000 })
  await page.reload()
  await expect(page.locator('.cm-content')).toContainText(marker)
})

test('switching the active set shows a banner and keeps the code', async ({ page, browser }) => {
  const owner = await browser.newContext({ storageState: 'e2e/.auth/owner.json' })
  const api = owner.request
  const me = await (await api.get('/api/v1/auth/me')).json()
  const headers = { 'X-CSRF-Token': me.csrf_token as string }
  const before = (await (await api.get('/api/v1/site')).json()).active_set_id as string
  const sets = (await (await api.get('/api/v1/admin/problem-sets')).json()) as Array<{ id: string; slug: string; status: string }>
  const target = sets.find((s) => s.status === 'published' && s.id !== before && s.slug !== 'week-1-io')!

  try {
    await page.goto('/?all=1')
    await page.getByRole('link', { name: /第 1 週：輸入與輸出/ }).click()
    await page.getByRole('link', { name: /A \+ B/ }).click()
    const marker = `// keep me ${Date.now()}`
    await setCode(page, `${marker}\n`)

    const r = await api.put('/api/v1/admin/site/active-problem-set', { data: { problem_set: target.slug }, headers })
    expect(r.ok()).toBeTruthy()

    // Site info is polled every 30 s.
    await expect(page.getByTestId('active-set-banner')).toBeVisible({ timeout: 45_000 })
    await expect(page.getByTestId('active-set-banner')).toContainText('你的程式碼已保留')
    await expect(page.locator('.cm-content')).toContainText(marker)
  } finally {
    const prev = sets.find((s) => s.id === before)
    if (prev) await api.put('/api/v1/admin/site/active-problem-set', { data: { problem_set: prev.slug }, headers })
    await owner.close()
  }
})
