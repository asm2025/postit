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
  build: {
    rolldownOptions: {
      output: {
        // Vendor code in stable groups: each stays under the 500 kB warning and keeps its
        // cache across app releases. Unmatched modules stay with the code that imports them.
        codeSplitting: {
          groups: [
            { name: 'react', test: /[\\/]node_modules[\\/](react|react-dom|scheduler)[\\/]/ },
            { name: 'router', test: /[\\/]node_modules[\\/](react-router|@tanstack)[\\/]/ },
            { name: 'ui', test: /[\\/]node_modules[\\/](radix-ui|@radix-ui|lucide-react)[\\/]/ },
            { name: 'i18n', test: /[\\/]node_modules[\\/](i18next[^\\/]*|react-i18next)[\\/]/ },
            { name: 'auth', test: /[\\/]node_modules[\\/](oidc-client-ts|react-oidc-context)[\\/]/ },
          ],
        },
      },
    },
  },
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
