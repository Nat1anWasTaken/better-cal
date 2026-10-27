import { type Page, expect } from '@playwright/test'

export const STUDENT = { username: process.env.E2E_STUDENT ?? 's1', password: process.env.E2E_STUDENT_PASSWORD ?? 'student123' }
export const OWNER = { username: process.env.E2E_OWNER ?? 'owner', password: process.env.E2E_OWNER_PASSWORD ?? 'ownerpass123' }

export async function login(page: Page, who: { username: string; password: string }) {
  await page.goto('/login')
  await page.getByLabel('帳號').fill(who.username)
  await page.getByLabel('密碼').fill(who.password)
  await page.getByRole('button', { name: '登入' }).click()
  await expect(page).not.toHaveURL(/\/login/)
}

/** Replaces the whole CodeMirror document. */
export async function setCode(page: Page, code: string) {
  const editor = page.locator('.cm-content').first()
  await editor.click()
  await page.keyboard.press('Control+a')
  await page.keyboard.press('Delete')
  await page.keyboard.insertText(code)
}

export async function waitForResult(page: Page) {
  const result = page.getByTestId('submission-result')
  await expect(result).toHaveAttribute('data-status', /^(done|system_error)$/, { timeout: 60_000 })
  return result
}
