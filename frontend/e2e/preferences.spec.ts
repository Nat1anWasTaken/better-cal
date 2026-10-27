import { expect, test } from '@playwright/test'

import { waitForResult } from './helpers'

test.use({ storageState: 'e2e/.auth/student.json' })

// The other specs share this student account and type into a plain editor.
test.afterEach(async ({ page }) => {
  const me = await (await page.request.get('/api/v1/auth/me')).json()
  await page.request.put('/api/v1/auth/preferences', { data: {}, headers: { 'X-CSRF-Token': me.csrf_token } })
})

test('preferences: Vim mode and editor theme are saved to the account and apply to the problem editor', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: '帳號選單' }).click()
  await page.getByRole('menuitem', { name: '偏好設定' }).click()
  await expect(page.getByRole('heading', { name: '偏好設定' })).toBeVisible()

  // The preview follows the unsaved form.
  await page.getByLabel('Vim 模式').click()
  await expect(page.getByTestId('vim-status')).toBeVisible()
  await page.getByRole('combobox', { name: '編輯器配色' }).click()
  await page.getByRole('option', { name: '高對比（深色）' }).click()
  await expect(page.locator('.monaco-editor.hc-black')).toBeVisible()
  await page.getByRole('button', { name: '儲存設定' }).click()
  await expect(page.getByText('偏好設定已儲存')).toBeVisible()

  await page.goto('/problems/a-plus-b')
  await expect(page.getByRole('heading', { name: 'A + B' })).toBeVisible()
  const status = page.getByTestId('vim-status')
  await expect(page.locator('.monaco-editor.hc-black')).toBeVisible()
  await page.locator('.monaco-editor .view-lines').first().click()
  await page.keyboard.press('Escape')
  await expect(status).toContainText('--NORMAL--')
  await page.keyboard.press('i')
  await expect(status).toContainText('--INSERT--')
  await page.keyboard.press('Escape')
  await expect(status).toContainText('--NORMAL--')

  // Ctrl+Enter still runs in Vim mode.
  await page.keyboard.press('Control+Enter')
  await waitForResult(page)

  // Saved on the server: survives a reload.
  await page.reload()
  await expect(page.getByTestId('vim-status')).toBeVisible()

  // The toolbar popover saves straight away.
  await page.getByRole('button', { name: '編輯器設定' }).click()
  await page.getByLabel('Vim 模式').click()
  await expect(page.getByTestId('vim-status')).toHaveCount(0)
  await page.reload()
  await expect(page.locator('.monaco-editor .view-lines').first()).toBeVisible()
  await expect(page.getByTestId('vim-status')).toHaveCount(0)
})

test('preferences: Vim commands cannot change the fixed code of a fill-in problem', async ({ page }) => {
  const me = await (await page.request.get('/api/v1/auth/me')).json()
  await page.request.put('/api/v1/auth/preferences', { data: { keymap: 'vim' }, headers: { 'X-CSRF-Token': me.csrf_token } })
  await page.goto('/?all=1')
  await page.getByRole('link', { name: /第 2 週/ }).click()
  await page.getByRole('link', { name: /補寫函式/ }).click()
  const editor = page.locator('.monaco-editor .view-lines')
  await expect(editor).toContainText('int main(void)')
  await expect(page.getByTestId('vim-status')).toBeVisible()

  await page.locator('.monaco-editor .view-line', { hasText: 'int main(void)' }).click()
  await page.keyboard.press('Escape')
  await page.keyboard.type('dd')
  await page.keyboard.type('x')
  await page.keyboard.type('AXYZ')
  await page.keyboard.press('Escape')
  await expect(editor).toContainText('int main(void)')
  await expect(editor).not.toContainText('XYZ')
})
