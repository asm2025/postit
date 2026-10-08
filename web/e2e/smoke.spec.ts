import { expect, test, type Page } from '@playwright/test'
import { STACK } from '../playwright.config.ts'

test.skip(!process.env.POSTIT_E2E, 'opt-in: needs the dev stack and Zitadel')

async function login(page: Page, email: string, password: string) {
  await page.goto('/')
  await page.getByRole('button', { name: 'Sign in' }).click()
  await page.getByLabel(/login name|email/i).fill(email)
  await page.getByRole('button', { name: /next/i }).click()
  await page.getByLabel(/password/i).fill(password)
  await page.getByRole('button', { name: /next/i }).click()
}

test('admin reaches the dashboard and a reload keeps the session', async ({ page }) => {
  await login(page, 'admin@postit.com', process.env.POSTIT_ADMIN_PASSWORD ?? '')
  await expect(page.getByRole('heading', { name: 'Home' })).toBeVisible()
  await page.reload()
  await expect(page.getByRole('heading', { name: 'Home' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Sign in' })).toHaveCount(0)
})

test('member waits, admin approves from Home, member gets in', async ({ browser }) => {
  const member = await (await browser.newContext(STACK)).newPage()
  await login(member, 'member@postit.com', process.env.POSTIT_MEMBER_PASSWORD ?? '')
  await expect(member.getByRole('heading', { name: 'Waiting for approval' })).toBeVisible()

  const admin = await (await browser.newContext(STACK)).newPage()
  await login(admin, 'admin@postit.com', process.env.POSTIT_ADMIN_PASSWORD ?? '')
  const row = admin.getByRole('listitem').filter({ hasText: 'member@postit.com' })
  await row.getByRole('button', { name: 'Approve' }).click()
  await expect(row).toHaveCount(0)

  await member.getByRole('button', { name: 'Refresh' }).click()
  await expect(member.getByRole('heading', { name: 'Home' })).toBeVisible()
})
