import { expect, test } from '@playwright/test'

import { setCode, waitForResult } from './helpers'

test.use({ storageState: 'e2e/.auth/student.json' })

const BROKEN = `#include <stdio.h>

int main(void) {
    int a, b;
    scanf("%d %d", &a, &b)
    printf("%d\\n", a + b);
    return 0;
}
`
const FIXED = BROKEN.replace('&a, &b)', '&a, &b);')

test('student: run broken code, read the card, reveal a hint, jump, fix and pass', async ({ page }) => {
  await page.goto('/?all=1')
  await page.getByRole('link', { name: /第 1 週：輸入與輸出/ }).click()
  await page.getByRole('link', { name: /A \+ B/ }).click()
  await expect(page.getByRole('heading', { name: 'A + B' })).toBeVisible()

  await setCode(page, BROKEN)
  await page.getByTestId('run-button').click()
  const result = await waitForResult(page)
  await expect(result).toHaveAttribute('data-verdict', 'compile_error')

  const primary = page.getByTestId('primary-card')
  await expect(primary).toHaveAttribute('data-rule', 'syntax.missing_semicolon')
  await expect(primary.getByText('建議先處理這一個')).toBeVisible()
  await expect(primary.getByText('發生什麼事')).toBeVisible()

  // Progressive hints: none shown at first, one more per click.
  await expect(primary.getByText('提示 1', { exact: true })).toHaveCount(0)
  await primary.getByTestId('reveal-hint').click()
  await expect(primary.getByText('提示 1', { exact: true })).toBeVisible()

  // Jump to the reported location (GCC points at the end of the scanf line).
  await primary.getByTestId('jump-button').click()
  await expect(page.locator('.cm-jump-line')).toBeVisible()
  await expect(page.locator('.cm-jump-line')).toContainText('scanf')

  // Original compiler message is available but collapsed.
  await primary.getByText('原始訊息（給進階學習者）').click()
  await expect(primary.getByText(/expected/)).toBeVisible()

  await setCode(page, FIXED)
  await page.getByTestId('submit-button').click()
  const final = await waitForResult(page)
  await expect(final).toHaveAttribute('data-verdict', 'accepted')
  await expect(final.getByText('全部測資都通過了')).toBeVisible()
  await expect(final.getByText('隱藏測資 #3')).toBeVisible()
})

test('student: fill-in problem keeps fixed code read-only and passes with blanks', async ({ page }) => {
  await page.goto('/?all=1')
  await page.getByRole('link', { name: /第 2 週/ }).click()
  await page.getByRole('link', { name: /補寫函式/ }).click()
  const editor = page.locator('.cm-content')
  await expect(editor).toContainText('int max(int a, int b)')
  // Start from the template (a previous run may have left a draft).
  await page.getByRole('button', { name: '恢復起始程式' }).click()
  await page.getByRole('button', { name: '恢復起始程式' }).last().click()
  await expect(editor).toContainText('return 0;')

  // Typing inside fixed code is rejected.
  await page.locator('.cm-line', { hasText: 'int main(void)' }).click()
  await page.keyboard.press('End')
  await page.keyboard.type('XYZ')
  await expect(editor).not.toContainText('XYZ')

  // Replace the blank's content (line "return 0;" inside max).
  const blankLine = page.locator('.cm-line', { hasText: 'return 0;' }).first()
  await blankLine.click()
  await page.keyboard.press('End')
  await page.keyboard.press('Shift+Home')
  await page.keyboard.insertText('    return a > b ? a : b;')
  await expect(editor).toContainText('return a > b ? a : b;')

  await page.getByTestId('submit-button').click()
  const result = await waitForResult(page)
  await expect(result).toHaveAttribute('data-verdict', 'accepted')
})
