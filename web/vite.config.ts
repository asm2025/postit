/// <reference types="vitest/config" />
import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

const certDir = path.resolve(import.meta.dirname, '../docker/shared/nginx/certs')
const key = path.join(certDir, 'postit.local.key')
const cert = path.join(certDir, 'postit.local.crt')

/** The dev server only makes sense on https://postit.local:44315 (the registered redirect URI). */
function devHttps() {
  if (!existsSync(key) || !existsSync(cert)) {
    throw new Error(
      `Missing ${cert} or its key. Create the dev certificate with ./cert.ps1 (Windows) or ./cert.sh from the repo root.`,
    )
  }
  return { key: readFileSync(key), cert: readFileSync(cert) }
}

export default defineConfig(({ command, mode }) => ({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': path.resolve(import.meta.dirname, 'src') } },
  server:
    command === 'serve' && mode !== 'test'
      ? { host: 'postit.local', port: 44315, strictPort: true, https: devHttps() }
      : undefined,
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
    css: false,
  },
}))
