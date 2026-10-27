import { expect, test } from '@playwright/test'

test.use({ storageState: 'e2e/.auth/owner.json' })

// Needs a stack with PUBLIC_URL set (OAuth issuer). The client's redirect is
// intercepted, so no real client is involved.
const REDIRECT = 'https://client.example/cb'

test('owner: enable MCP, approve an OAuth client with narrowed scopes, then disconnect it', async ({ page, request, baseURL }) => {
  await page.goto('/admin/tokens')
  const toggle = page.getByRole('switch')
  if ((await toggle.getAttribute('aria-checked')) !== 'true') {
    await toggle.click()
    await expect(page.getByText('MCP 已啟用')).toBeVisible()
  }
  await expect(page.getByTestId('mcp-url')).toHaveValue(/\/mcp$/)

  const name = `E2E Agent ${Date.now()}`
  const reg = await request.post('/oauth/register', { data: { client_name: name, redirect_uris: [REDIRECT], token_endpoint_auth_method: 'none' } })
  expect(reg.status()).toBe(201)
  const { client_id } = (await reg.json()) as { client_id: string }

  const q = new URLSearchParams({
    response_type: 'code',
    client_id,
    redirect_uri: REDIRECT,
    code_challenge: 'E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM',
    code_challenge_method: 'S256',
    state: 'e2e-state',
    scope: 'problems:read problems:write publish',
  })
  let landed = ''
  await page.route('https://client.example/**', async (route) => {
    landed = route.request().url()
    await route.fulfill({ status: 200, contentType: 'text/html', body: '<p>client</p>' })
  })
  await page.goto(`/oauth/authorize?${q}`)
  await expect(page).toHaveURL(/\/oauth\/consent\?request=/)
  await expect(page.getByRole('heading', { name: `「${name}」想要存取管理功能` })).toBeVisible()
  await expect(page.getByTestId('redirect-host')).toHaveText('client.example')
  await expect(page.getByLabel(/發布題目/)).toBeChecked()
  await page.getByLabel(/發布題目/).uncheck()
  await page.getByRole('button', { name: '允許' }).click()
  await expect.poll(() => landed).toContain('code=')
  const back = new URL(landed)
  expect(back.searchParams.get('state')).toBe('e2e-state')
  expect(back.searchParams.get('iss')).toBe(new URL(baseURL!).origin)

  await page.goto('/admin/tokens')
  const row = page.getByTestId('mcp-connections').getByRole('listitem').filter({ hasText: name })
  await expect(row).toContainText('problems:read problems:write')
  await expect(row).not.toContainText('publish')
  page.once('dialog', (d) => d.accept())
  await row.getByRole('button', { name: '中斷連線' }).click()
  await expect(page.getByTestId('mcp-connections').getByText(name)).toHaveCount(0)
})
