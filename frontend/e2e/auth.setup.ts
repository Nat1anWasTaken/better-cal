import { test as setup } from '@playwright/test'

import { OWNER, STUDENT, login } from './helpers'

// Log in once per role and reuse the session cookie (the API rate-limits logins).
for (const [name, who] of [
  ['student', STUDENT],
  ['owner', OWNER],
] as const) {
  setup(`login as ${name}`, async ({ page }) => {
    await login(page, who)
    await page.context().storageState({ path: `e2e/.auth/${name}.json` })
  })
}
