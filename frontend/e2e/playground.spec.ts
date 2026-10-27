import { expect, test } from '@playwright/test'

import { setCode } from './helpers'

test.use({ storageState: 'e2e/.auth/student.json' })

test('student: playground explains a compile error, then runs the fixed program with stdin', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('link', { name: '練習場' }).click()
  await expect(page).toHaveURL(/\/playground$/)

  await setCode(page, '#include <stdio.h>\n\nint main(void) {\n    int a, b;\n    scanf("%d %d", &a, &b)\n    printf("%d\\n", a + b);\n    return 0;\n}\n')
  await page.getByTestId('playground-run').click()
  const result = page.getByTestId('playground-result')
  await expect(result.getByText('無法編譯')).toBeVisible({ timeout: 30_000 })
  const primary = page.getByTestId('primary-card')
  await expect(primary).toHaveAttribute('data-rule', 'syntax.missing_semicolon')

  // Hints are revealed one at a time, as on problems.
  await expect(primary.getByText('提示 1', { exact: true })).toHaveCount(0)
  await primary.getByTestId('reveal-hint').click()
  await expect(primary.getByText('提示 1', { exact: true })).toBeVisible()

  // The card jumps to the line it points at.
  await primary.getByTestId('jump-button').click()
  await expect(page.locator('.monaco-editor .active-line-number')).toHaveText('5')

  await setCode(page, '#include <stdio.h>\n\nint main(void) {\n    int a, b;\n    scanf("%d %d", &a, &b);\n    printf("%d\\n", a + b);\n    return 0;\n}\n')
  await page.getByLabel('輸入（stdin）').fill('2 40')
  await page.getByTestId('playground-run').click()
  await expect(page.getByTestId('playground-stdout')).toHaveText('42', { timeout: 30_000 })
  await expect(result.getByText('執行完成')).toBeVisible()

  // The program and input survive a reload.
  await page.reload()
  await expect(page.locator('.monaco-editor .view-lines')).toContainText('scanf("%d %d", &a, &b);')
  await expect(page.getByLabel('輸入（stdin）')).toHaveValue('2 40')
})
