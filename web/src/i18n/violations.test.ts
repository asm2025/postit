/// <reference types="node" />
import { readFileSync } from 'node:fs'
import path from 'node:path'
import i18next from 'i18next'
import enViolations from './en-US/violations.json'

const dir = import.meta.dirname
const spec = JSON.parse(readFileSync(path.join(dir, '../../../backend/openapi.json'), 'utf8')) as {
  components: { schemas: { ViolationCode: { enum: string[] } } }
}
const codes = spec.components.schemas.ViolationCode.enum

it('every violation code has an English entry', () => {
  expect(codes.length).toBeGreaterThan(0)
  for (const code of codes) {
    expect(Object.hasOwn(enViolations, code), `violations.${code}`).toBe(true)
    expect(enViolations[code as keyof typeof enViolations].trim(), code).not.toBe('')
  }
})

it('has no entry for a code the API does not send', () => {
  for (const key of Object.keys(enViolations)) expect(codes, key).toContain(key)
})

it('fills a message from the violation parameters', async () => {
  const i18n = i18next.createInstance()
  await i18n.init({
    lng: 'en-US',
    resources: { 'en-US': { violations: enViolations } },
    interpolation: { escapeValue: false },
  })
  expect(i18n.t('text_too_long', { ns: 'violations', actual: 301, max: 280 })).toBe(
    'The text is 301 characters; this platform allows 280.',
  )
  expect(i18n.t('agent_rule', { ns: 'violations', message: { key: 'x', en: 'Use fewer tags.' } })).toBe(
    'Use fewer tags.',
  )
})
