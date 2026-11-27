import type { Page } from '@playwright/test'

/** Signs in through the identity provider's own pages; the passwords come from the dev stack's vault. */
export async function login(page: Page, email: string, password: string) {
  await page.goto('/')
  await page.getByRole('button', { name: 'Sign in' }).click()
  await page.getByLabel(/login name|email/i).fill(email)
  await page.getByRole('button', { name: /next/i }).click()
  await page.getByLabel(/password/i).fill(password)
  await page.getByRole('button', { name: /next/i }).click()
}
