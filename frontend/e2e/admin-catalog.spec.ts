import { expect, test } from '@playwright/test'

test.use({ storageState: 'e2e/.auth/owner.json' })

const RULE = 'syntax.missing_semicolon'

async function publishAll(page: import('@playwright/test').Page) {
  await page.goto('/admin/catalog')
  const btn = page.getByTestId('publish-catalog')
  await expect(btn).toBeEnabled()
  await btn.click()
  await expect(page.getByText(/已發布 \d+ 項修改/)).toBeVisible()
}

test('owner: edit a rule title, preview it with the real compiler, publish, then restore', async ({ page }) => {
  const marker = `E2E 測試標題 ${Date.now()}`
  await page.goto(`/admin/catalog/rules/${RULE}`)
  const title = page.getByTestId('rule-title')
  await expect(title).toBeVisible()
  await title.fill(marker)
  await page.getByTestId('save-rule').click()
  await expect(page.getByText('草稿已儲存，尚未發布')).toBeVisible()

  // Preview (drafts included) compiles the sample with gcc and shows the new title.
  await page.getByTestId('run-preview').click()
  await expect(page.getByTestId('preview-result').getByRole('heading', { name: marker })).toBeVisible({ timeout: 30_000 })

  await publishAll(page)

  // Restore the built-in wording so the test is repeatable.
  await page.goto(`/admin/catalog/rules/${RULE}`)
  await page.getByRole('button', { name: '恢復內建文字' }).click()
  await expect(page.getByText('發布後將恢復成內建文字')).toBeVisible()
  await publishAll(page)
  await page.goto(`/admin/catalog/rules/${RULE}`)
  await expect(page.getByTestId('rule-title')).toHaveValue('指令結尾可能少了分號')
})
