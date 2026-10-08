import { defineConfig } from '@playwright/test'

export const STACK = { baseURL: 'https://postit.local:44315', ignoreHTTPSErrors: true }

export default defineConfig({
  testDir: 'e2e',
  use: STACK,
})
