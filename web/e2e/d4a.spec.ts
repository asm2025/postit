import { expect, test, type Page } from '@playwright/test'
import { STACK } from '../playwright.config.ts'
import { login } from './support.ts'

test.skip(!process.env.POSTIT_E2E, 'opt-in: needs the dev stack (cargo dev, for the mock platforms) and Zitadel')

// One owner and one delegate, in two browsers, walking the D4a slice from the admin switching the
// mock platforms on to the owner ending the grant. The steps share the dev stack's state, so they
// run in order.
test.describe.configure({ mode: 'serial' })

const OWNER = 'admin@postit.com'
const DELEGATE = 'member@postit.com'

async function signedIn(browser: import('@playwright/test').Browser, email: string, password: string) {
  const page = await (await browser.newContext(STACK)).newPage()
  await login(page, email, password)
  await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible()
  return page
}

const ownerPassword = () => process.env.POSTIT_ADMIN_PASSWORD ?? ''
const delegatePassword = () => process.env.POSTIT_MEMBER_PASSWORD ?? ''

async function connectMockByCredentials(page: Page, name: string) {
  await page.goto('/accounts/connect/mock')
  await page.getByLabel(/account name/i).fill(name)
  await page.getByLabel(/token/i).fill(`token-${name}`)
  await page.getByRole('button', { name: 'Check credentials' }).click()
  await expect(page.getByRole('heading', { name: 'Choose the accounts to connect' })).toBeVisible()
  await page.getByRole('button', { name: 'Confirm' }).click()
  await expect(page).toHaveURL(/\/accounts$/)
  await expect(page.getByRole('status').filter({ hasText: /connected/i })).toBeVisible()
}

test('the admin switches the mock platforms on', async ({ browser }) => {
  const admin = await signedIn(browser, OWNER, ownerPassword())
  await admin.goto('/admin/platforms')
  for (const name of ['Mock', 'Mock OAuth']) {
    const toggle = admin.getByRole('switch', { name: `Offer ${name} to people` })
    if ((await toggle.getAttribute('aria-checked')) !== 'true') await toggle.click()
    await expect(toggle).toHaveAttribute('aria-checked', 'true')
  }
})

test('the owner connects accounts, imports a file, and makes a group', async ({ browser }) => {
  const owner = await signedIn(browser, OWNER, ownerPassword())

  // By credentials, through the checklist.
  await connectMockByCredentials(owner, 'e2e-shop')

  // Through the mock platform's OAuth sign-in, which comes back to /oauth-done.
  await owner.goto('/accounts/connect')
  await owner.getByRole('listitem').filter({ hasText: 'Mock OAuth' }).getByRole('button', { name: 'Connect' }).click()
  await owner.waitForURL(/\/oauth-done|\/accounts/)
  await expect(owner.getByRole('heading', { name: 'Choose the accounts to connect' })).toBeVisible()
  expect(owner.url()).not.toContain('connect_id')
  await owner.getByRole('button', { name: 'Confirm' }).click()
  await expect(owner).toHaveURL(/\/accounts$/)

  // A three-row file: checked first, then connected.
  await owner.goto('/accounts/import')
  await owner.getByRole('combobox', { name: 'Platform' }).click()
  await owner.getByRole('option', { name: 'Mock', exact: true }).click()
  await owner.getByLabel('CSV file').setInputFiles({
    name: 'accounts.csv',
    mimeType: 'text/csv',
    buffer: Buffer.from('account_name,token\ne2e-a,tok-a\ne2e-b,tok-b\ne2e-c,tok-c\n'),
  })
  await owner.getByRole('button', { name: 'Check file' }).click()
  await expect(owner.getByRole('table', { name: 'Report' })).toBeVisible()
  await owner.getByRole('button', { name: 'Connect valid rows' }).click()
  await expect(owner.getByText('Ready to connect: 3')).toBeVisible()

  // A group of two of them.
  await owner.goto('/accounts/groups')
  await owner.getByRole('button', { name: 'New group' }).click()
  const dialog = owner.getByRole('dialog')
  await dialog.getByLabel('Name').fill('e2e group')
  await dialog.getByRole('checkbox', { name: 'e2e-a' }).check()
  await dialog.getByRole('checkbox', { name: 'e2e-b' }).check()
  await dialog.getByRole('button', { name: 'Save group' }).click()
  await expect(owner.getByRole('table', { name: 'Target groups' }).getByText('e2e group')).toBeVisible()
})

test('the owner grants publish on one account; the delegate sees only that one, read-only', async ({ browser }) => {
  const owner = await signedIn(browser, OWNER, ownerPassword())
  await owner.goto('/delegations')
  await owner.getByRole('button', { name: 'Give access' }).click()
  const dialog = owner.getByRole('dialog')
  await dialog.getByLabel('Their email').fill(DELEGATE)
  await expect(dialog.getByText(/Found:/)).toBeVisible()
  await dialog.getByRole('combobox', { name: 'Access level' }).click()
  await owner.getByRole('option', { name: 'Publish' }).click()
  await dialog.getByLabel('Only these accounts').check()
  await dialog.getByRole('checkbox', { name: 'e2e-shop' }).check()
  await dialog.getByRole('button', { name: 'Give access' }).click()
  await expect(owner.getByRole('table', { name: 'Granted' })).toContainText('1 account')

  const delegate = await signedIn(browser, DELEGATE, delegatePassword())
  await delegate.getByRole('combobox', { name: 'Workspace' }).click()
  await delegate.getByRole('option', { name: /workspace · Can publish · 1 account/ }).click()
  await delegate.goto('/accounts')
  await expect(delegate.getByRole('status').filter({ hasText: 'You are working in' })).toBeVisible()
  await expect(delegate.getByRole('link', { name: 'e2e-shop' })).toBeVisible()
  await expect(delegate.getByRole('link', { name: 'e2e-a' })).toHaveCount(0)
  await expect(delegate.getByRole('link', { name: 'Connect an account' })).toHaveCount(0)
})

test('the owner revokes the grant; the delegate lands back in their own workspace with the reason', async ({
  browser,
}) => {
  const delegate = await signedIn(browser, DELEGATE, delegatePassword())
  await delegate.getByRole('combobox', { name: 'Workspace' }).click()
  await delegate.getByRole('option', { name: /workspace · Can publish · 1 account/ }).click()

  const owner = await signedIn(browser, OWNER, ownerPassword())
  await owner.goto('/delegations')
  await owner.getByRole('button', { name: /Edit access for/ }).click()
  await owner.getByRole('dialog').getByRole('button', { name: 'Revoke access' }).click()
  await owner.getByRole('alertdialog').getByRole('button', { name: 'Revoke access' }).click()
  await expect(owner.getByText('You have not given anyone access.')).toBeVisible()

  // The delegate's next request in that workspace is refused, and the tab leaves it once.
  await delegate.goto('/accounts')
  await expect(delegate.getByRole('alert').filter({ hasText: /access to that workspace ended/i })).toBeVisible()
  await expect(delegate.getByRole('status').filter({ hasText: 'You are working in' })).toHaveCount(0)
})
