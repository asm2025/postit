import { expect, test } from '@playwright/test'
import { STACK } from '../playwright.config.ts'
import { login } from './support.ts'

test.skip(!process.env.POSTIT_E2E, 'opt-in: needs the dev stack and Zitadel')

test('admin reaches the dashboard and a reload keeps the session', async ({ page }) => {
  await login(page, 'admin@postit.com', process.env.POSTIT_ADMIN_PASSWORD ?? '')
  await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible()
  await page.reload()
  await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible()
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
  await expect(member.getByRole('navigation', { name: 'Main' })).toBeVisible()
})

test('sign-in page is in English, left to right', async ({ page }) => {
  await page.goto('/login')
  await expect(page.locator('html')).toHaveAttribute('dir', 'ltr')
  await expect(page.locator('html')).toHaveAttribute('lang', 'en-US')
})
